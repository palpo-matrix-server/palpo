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

use diesel::dsl::count_distinct;
use diesel::prelude::*;

use crate::core::Seqnum;
use crate::core::client::sync_events;
use crate::core::events::ignored_user_list::IgnoredUserListEvent;
use crate::core::events::{AnyStrippedStateEvent, GlobalAccountDataEventType};
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::data::schema::*;
use crate::data::{self, connect, diesel_exists};
use crate::{AppError, AppResult};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = users)]
pub struct DbUser {
    pub id: OwnedUserId,
    pub ty: Option<String>,
    pub is_admin: bool,
    pub is_guest: bool,
    pub appservice_id: Option<String>,
    pub shadow_banned: bool,
    pub consent_at: Option<UnixMillis>,
    pub consent_version: Option<String>,
    pub consent_server_notice_sent: Option<String>,
    pub approved_at: Option<UnixMillis>,
    pub approved_by: Option<OwnedUserId>,
    pub deactivated_at: Option<UnixMillis>,
    pub deactivated_by: Option<OwnedUserId>,
    pub locked_at: Option<UnixMillis>,
    pub locked_by: Option<OwnedUserId>,
    pub created_at: UnixMillis,
}

#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = users)]
pub struct NewDbUser {
    pub id: OwnedUserId,
    pub ty: Option<String>,
    pub is_admin: bool,
    pub is_guest: bool,
    pub appservice_id: Option<String>,
    pub created_at: UnixMillis,
}

impl DbUser {
    pub fn is_deactivated(&self) -> bool {
        self.deactivated_at.is_some()
    }
}

pub struct SlidingSyncCache {
    lists: BTreeMap<String, sync_events::v4::ReqList>,
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
    known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, i64>>, // For every room, the room_since_sn number
    extensions: sync_events::v4::ExtensionsConfig,
}

/// Returns an iterator over all rooms this user joined.
pub fn joined_rooms(user_id: &UserId, since_sn: Seqnum) -> AppResult<Vec<OwnedRoomId>> {
    room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("join"))
        .filter(room_users::event_sn.ge(since_sn))
        .select(room_users::room_id)
        .load(&mut connect()?)
        .map_err(Into::into)
}

// pub fn forget_before_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<Option<i64>> {
//     room_users::table
//         .filter(room_users::user_id.eq(user_id))
//         .filter(room_users::forgotten.eq(true))
//         .select(room_users::event_sn)
//         .first::<i64>(&mut connect()?)
//         .optional()
//         .map_err(Into::into)
// }

/// Returns an iterator over all rooms a user was invited to.
pub fn invited_rooms(
    user_id: &UserId,
    since_sn: i64,
) -> AppResult<Vec<(OwnedRoomId, Vec<RawJson<AnyStrippedStateEvent>>)>> {
    let list = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("invite"))
        .filter(room_users::event_sn.ge(since_sn))
        .select((room_users::room_id, room_users::state_data))
        .load::<(OwnedRoomId, Option<JsonValue>)>(&mut connect()?)?
        .into_iter()
        .filter_map(|(room_id, state_data)| {
            if let Some(state_data) = state_data
                .map(|state_data| serde_json::from_value(state_data).ok())
                .flatten()
            {
                Some((room_id, state_data))
            } else {
                None
            }
        })
        .collect();
    Ok(list)
}

pub fn knocked_rooms(
    user_id: &UserId,
    since_sn: i64,
) -> AppResult<Vec<(OwnedRoomId, Vec<RawJson<AnyStrippedStateEvent>>)>> {
    let list = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("knock"))
        .filter(room_users::event_sn.ge(since_sn))
        .select((room_users::room_id, room_users::state_data))
        .load::<(OwnedRoomId, Option<JsonValue>)>(&mut connect()?)?
        .into_iter()
        .filter_map(|(room_id, state_data)| {
            if let Some(state_data) = state_data
                .map(|state_data| serde_json::from_value(state_data).ok())
                .flatten()
            {
                Some((room_id, state_data))
            } else {
                None
            }
        })
        .collect();
    Ok(list)
}

pub const CONNECTIONS: LazyLock<Mutex<BTreeMap<(OwnedUserId, OwnedDeviceId, String), Arc<Mutex<SlidingSyncCache>>>>> =
    LazyLock::new(|| Default::default());

/// Check if a user has an account on this homeserver.
pub fn user_exists(user_id: &UserId) -> AppResult<bool> {
    let query = users::table.find(user_id);
    diesel_exists!(query, &mut connect()?).map_err(Into::into)
}

pub fn get_user(user_id: &UserId) -> AppResult<Option<DbUser>> {
    users::table
        .find(user_id)
        .first::<DbUser>(&mut connect()?)
        .optional()
        .map_err(Into::into)
}

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

/// Returns the number of users registered on this server.
pub fn count() -> AppResult<u64> {
    let count = user_passwords::table
        .select(count_distinct(user_passwords::user_id))
        .first::<i64>(&mut connect()?)?;
    Ok(count as u64)
}

/// Returns a list of local users as list of usernames.
///
/// A user account is considered `local` if the length of it's password is greater then zero.
pub fn list_local_users() -> AppResult<Vec<OwnedUserId>> {
    user_passwords::table
        .select(user_passwords::user_id)
        .load::<OwnedUserId>(&mut connect()?)
        .map_err(Into::into)
}

/// Returns the display_name of a user on this homeserver.
pub fn display_name(user_id: &UserId) -> AppResult<Option<String>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::display_name)
        .first::<Option<String>>(&mut connect()?)
        .map_err(Into::into)
}

pub fn set_display_name(user_id: &UserId, display_name: Option<&str>) -> AppResult<()> {
    diesel::update(
        user_profiles::table
            .filter(user_profiles::user_id.eq(user_id.as_str()))
            .filter(user_profiles::room_id.is_null()),
    )
    .set(user_profiles::display_name.eq(display_name))
    .execute(&mut connect()?)
    .map(|_| ())
    .map_err(Into::into)
}

/// Get the avatar_url of a user.
pub fn avatar_url(user_id: &UserId) -> AppResult<Option<OwnedMxcUri>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::avatar_url)
        .first::<Option<OwnedMxcUri>>(&mut connect()?)
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
}

/// Get the blurhash of a user.
pub fn blurhash(user_id: &UserId) -> AppResult<Option<String>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::blurhash)
        .first::<Option<String>>(&mut connect()?)
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
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

pub fn deactivate(user_id: &UserId, doer_id: &UserId) -> AppResult<()> {
    diesel::update(users::table.find(user_id))
        .set((
            users::deactivated_at.eq(UnixMillis::now()),
            users::deactivated_by.eq(doer_id.to_owned()),
        ))
        .execute(&mut connect()?)?;

    diesel::delete(user_threepids::table.filter(user_threepids::user_id.eq(user_id))).execute(&mut connect()?)?;
    diesel::delete(user_access_tokens::table.filter(user_access_tokens::user_id.eq(user_id)))
        .execute(&mut connect()?)?;

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
pub async fn full_user_deactivate(
	services: &Services,
	user_id: &UserId,
	all_joined_rooms: &[OwnedRoomId],
) -> Result<()> {
	services.users.deactivate_account(user_id).await.ok();
	super::update_displayname(services, user_id, None, all_joined_rooms).await;
	super::update_avatar_url(services, user_id, None, None, all_joined_rooms).await;

	services
		.users
		.all_profile_keys(user_id)
		.ready_for_each(|(profile_key, _)| {
			services.users.set_profile_key(user_id, &profile_key, None);
		})
		.await;

	for room_id in all_joined_rooms {
		let state_lock = services.rooms.state.mutex.lock(room_id).await;

		let room_power_levels = services
			.rooms
			.state_accessor
			.room_state_get_content::<RoomPowerLevelsEventContent>(
				room_id,
				&StateEventType::RoomPowerLevels,
				"",
			)
			.await
			.ok();

		let user_can_demote_self =
			room_power_levels
				.as_ref()
				.is_some_and(|power_levels_content| {
					RoomPowerLevels::from(power_levels_content.clone())
						.user_can_change_user_power_level(user_id, user_id)
				}) || services
				.rooms
				.state_accessor
				.room_state_get(room_id, &StateEventType::RoomCreate, "")
				.await
				.is_ok_and(|event| event.sender == user_id);

		if user_can_demote_self {
			let mut power_levels_content = room_power_levels.unwrap_or_default();
			power_levels_content.users.remove(user_id);

			// ignore errors so deactivation doesn't fail
			match services
				.rooms
				.timeline
				.build_and_append_pdu(
					PduBuilder::state(String::new(), &power_levels_content),
					user_id,
					room_id,
					&state_lock,
				)
				.await
			{
				| Err(e) => {
					warn!(%room_id, %user_id, "Failed to demote user's own power level: {e}");
				},
				| _ => {
					info!("Demoted {user_id} in {room_id} as part of account deactivation");
				},
			}
		}
	}

	super::leave_all_rooms(services, user_id).await;

	Ok(())
}
