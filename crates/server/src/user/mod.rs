mod device;
pub use device::*;
mod password;
pub use password::*;
mod profile;
pub use profile::*;
mod access_token;
pub use access_token::*;
mod filter;
pub use filter::*;
mod refresh_token;
pub use refresh_token::*;
pub mod key;
pub mod pusher;
// pub mod push_rule;
pub use key::*;
pub mod key_backup;
pub mod session;
pub use key_backup::*;
pub use session::*;
pub mod presence;
pub use presence::*;

use std::collections::BTreeMap;
use std::mem;
use std::sync::{Arc, LazyLock, Mutex};

use diesel::prelude::*;

use crate::core::client::sync_events;
use crate::core::events::ignored_user_list::IgnoredUserListEvent;
use crate::core::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use crate::core::events::{GlobalAccountDataEventType, StateEventType};
use crate::core::identifiers::*;
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::data::schema::*;
use crate::data::user::{DbUser, NewDbUser};
use crate::data::{self, connect, diesel_exists};
use crate::{AppError, AppResult, MatrixError, PduBuilder, utils};

pub struct SlidingSyncCache {
    lists: BTreeMap<String, sync_events::v4::ReqList>,
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
    known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, i64>>, // For every room, the room_since_sn number
    extensions: sync_events::v4::ExtensionsConfig,
}

pub const CONNECTIONS: LazyLock<Mutex<BTreeMap<(OwnedUserId, OwnedDeviceId, String), Arc<Mutex<SlidingSyncCache>>>>> =
    LazyLock::new(|| Default::default());

pub fn create_user(user_id: impl Into<OwnedUserId>, password: Option<&str>) -> AppResult<DbUser> {
    let user_id = user_id.into();
    let new_user = NewDbUser {
        id: user_id.clone(),
        ty: None,
        is_admin: false,
        is_guest: password.is_none(),
        appservice_id: None,
        created_at: UnixMillis::now(),
    };
    let user = diesel::insert_into(users::table)
        .values(&new_user)
        .on_conflict(users::id)
        .do_update()
        .set(&new_user)
        .get_result::<DbUser>(&mut connect()?)?;
    if let Some(password) = password {
        let hash = crate::utils::hash_password(password)?;
        diesel::insert_into(user_passwords::table)
            .values(NewDbPassword {
                user_id,
                hash,
                created_at: UnixMillis::now(),
            })
            .execute(&mut connect()?)?;
    }
    Ok(user)
}

/// Ensure that a user only sees signatures from themselves and the target user
pub fn clean_signatures<F: Fn(&UserId) -> bool>(
    cross_signing_key: &mut serde_json::Value,
    sender_id: Option<&UserId>,
    user_id: &UserId,
    allowed_signatures: F,
) -> AppResult<()> {
    if let Some(signatures) = cross_signing_key.get_mut("signatures").and_then(|v| v.as_object_mut()) {
        // Don't allocate for the full size of the current signatures, but require
        // at most one resize if nothing is dropped
        let new_capacity = signatures.len() / 2;
        for (user, signature) in mem::replace(signatures, serde_json::Map::with_capacity(new_capacity)) {
            let sid =
                <&UserId>::try_from(user.as_str()).map_err(|_| AppError::internal("Invalid user ID in database."))?;
            if sender_id == Some(user_id) || sid == user_id || allowed_signatures(sid) {
                signatures.insert(user, signature);
            }
        }
    }

    Ok(())
}

/// Returns true/false based on whether the recipient/receiving user has
/// blocked the sender
pub fn user_is_ignored(sender_id: &UserId, recipient_id: &UserId) -> bool {
    if let Ok(Some(ignored)) = data::user::get_global_data::<IgnoredUserListEvent>(
        recipient_id,
        &GlobalAccountDataEventType::IgnoredUserList.to_string(),
    ) {
        ignored
            .content
            .ignored_users
            .keys()
            .any(|blocked_user| blocked_user == sender_id)
    } else {
        false
    }
}

/// Runs through all the deactivation steps:
///
/// - Mark as deactivated
/// - Removing display name
/// - Removing avatar URL and blurhash
/// - Removing all profile data
/// - Leaving all rooms (and forgets all of them)
pub async fn full_user_deactivate(user_id: &UserId, all_joined_rooms: &[OwnedRoomId]) -> AppResult<()> {
    data::user::deactivate(user_id).ok();
    data::user::set_display_name(user_id, None).ok();
    data::user::set_avatar_url(user_id, None).ok();

    // TODO: remove all user data
    // for profile_key in data::user::all_profile_keys(user_id) {
    //     data::user::set_profile_key(user_id, &profile_key, None);
    // }

    for room_id in all_joined_rooms {
        // let state_lock = crate::rooms::state.mutex.lock(room_id).await;

        let room_power_levels = crate::room::state::get_room_state_content::<RoomPowerLevelsEventContent>(
            room_id,
            &StateEventType::RoomPowerLevels,
            "",
        )
        .ok();

        let user_can_demote_self = room_power_levels.as_ref().is_some_and(|power_levels_content| {
            RoomPowerLevels::from(power_levels_content.clone()).user_can_change_user_power_level(user_id, user_id)
        }) || crate::room::state::get_room_state(room_id, &StateEventType::RoomCreate, "")
            .is_ok_and(|event| event.sender == user_id);

        if user_can_demote_self {
            let mut power_levels_content = room_power_levels.unwrap_or_default();
            power_levels_content.users.remove(user_id);

            // ignore errors so deactivation doesn't fail
            match crate::room::timeline::build_and_append_pdu(
                PduBuilder::state(String::new(), &power_levels_content),
                user_id,
                room_id,
                // &state_lock,
            ) {
                Err(e) => {
                    warn!(%room_id, %user_id, "Failed to demote user's own power level: {e}");
                }
                _ => {
                    info!("Demoted {user_id} in {room_id} as part of account deactivation");
                }
            }
        }
    }

    crate::membership::leave_all_rooms(user_id).await;

    Ok(())
}

/// Find out which user an OpenID access token belongs to.
pub async fn find_from_openid_token(token: &str) -> AppResult<OwnedUserId> {
    let Ok((user_id, expires_at)) = user_openid_tokens::table
        .filter(user_openid_tokens::token.eq(token))
        .select((user_openid_tokens::user_id, user_openid_tokens::expires_at))
        .first::<(OwnedUserId, UnixMillis)>(&mut connect()?)
    else {
        return Err(MatrixError::unauthorized("OpenID token is unrecognised").into());
    };
    if expires_at < UnixMillis::now() {
        tracing::warn!("OpenID token is expired, removing");
        diesel::delete(user_openid_tokens::table.filter(user_openid_tokens::token.eq(token)))
            .execute(&mut connect()?)?;

        return Err(MatrixError::unauthorized("OpenID token is expired").into());
    }

    Ok(user_id)
}

/// Creates a short-lived login token, which can be used to log in using the
/// `m.login.token` mechanism.
pub fn create_login_token(user_id: &UserId, token: &str) -> AppResult<u64> {
    use std::num::Saturating as Sat;

    let expires_in = crate::config().login_token_ttl;
    let expires_at = (Sat(UnixMillis::now().get()) + Sat(expires_in)).0 as i64;

    diesel::insert_into(user_login_tokens::table)
        .values((
            user_login_tokens::user_id.eq(user_id),
            user_login_tokens::token.eq(token),
            user_login_tokens::expires_at.eq(expires_at),
        ))
        .on_conflict(user_login_tokens::token)
        .do_update()
        .set(user_login_tokens::expires_at.eq(expires_at))
        .execute(&mut connect()?)?;

    Ok(expires_in)
}

/// Find out which user a login token belongs to.
/// Removes the token to prevent double-use attacks.
pub fn take_login_token(token: &str) -> AppResult<OwnedUserId> {
    let Ok((user_id, expires_at)) = user_login_tokens::table
        .filter(user_login_tokens::token.eq(token))
        .select((user_login_tokens::user_id, user_login_tokens::expires_at))
        .first::<(OwnedUserId, UnixMillis)>(&mut connect()?)
    else {
        return Err(MatrixError::forbidden("Login token is unrecognised").into());
    };

    if expires_at < UnixMillis::now() {
        trace!(?user_id, ?token, "Removing expired login token");
        diesel::delete(user_login_tokens::table.filter(user_login_tokens::token.eq(token))).execute(&mut connect()?)?;
        return Err(MatrixError::forbidden("Login token is expired").into());
    }

    diesel::delete(user_login_tokens::table.filter(user_login_tokens::token.eq(token))).execute(&mut connect()?)?;

    Ok(user_id)
}
