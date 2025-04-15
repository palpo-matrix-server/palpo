mod device;
pub use device::*;
mod password;
pub use password::*;
mod profile;
pub use profile::*;
mod filter;
pub use filter::*;
mod refresh_token;
pub use refresh_token::*;
mod data;
pub use data::*;
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

use crate::core::client::sync_events;
use crate::core::events::ignored_user_list::IgnoredUserListEvent;
use crate::core::events::{AnyStrippedStateEvent, GlobalAccountDataEventType};
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::Seqnum;
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::schema::*;
use crate::{connect, diesel_exists, DataError, DataResult};

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

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_access_tokens)]
pub struct DbAccessToken {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub puppets_user_id: Option<OwnedUserId>,
    pub last_validated: Option<UnixMillis>,
    pub refresh_token_id: Option<i64>,
    pub is_used: bool,
    pub expired_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_access_tokens)]
pub struct NewDbAccessToken {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub puppets_user_id: Option<OwnedUserId>,
    pub last_validated: Option<UnixMillis>,
    pub refresh_token_id: Option<i64>,
    pub is_used: bool,
    pub expired_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
}

impl NewDbAccessToken {
    pub fn new(user_id: OwnedUserId, device_id: OwnedDeviceId, token: String) -> Self {
        Self {
            user_id,
            device_id,
            token,
            puppets_user_id: None,
            last_validated: None,
            refresh_token_id: None,
            is_used: false,
            expired_at: None,
            created_at: UnixMillis::now(),
        }
    }
}

/// Returns an iterator over all rooms this user joined.
pub fn joined_rooms(user_id: &UserId, since_sn: Seqnum) -> DataResult<Vec<OwnedRoomId>> {
    room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("join"))
        .filter(room_users::event_sn.ge(since_sn))
        .select(room_users::room_id)
        .load(&mut connect()?)
        .map_err(Into::into)
}
/// Returns an iterator over all rooms a user was invited to.
pub fn invited_rooms(
    user_id: &UserId,
    since_sn: i64,
) -> DataResult<Vec<(OwnedRoomId, Vec<RawJson<AnyStrippedStateEvent>>)>> {
    let list = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("invite"))
        .filter(room_users::event_sn.ge(since_sn))
        .select((room_users::room_id, room_users::state_data))
        .load::<(OwnedRoomId, Option<JsonValue>)>(&mut *connect()?)?
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
) -> DataResult<Vec<(OwnedRoomId, Vec<RawJson<AnyStrippedStateEvent>>)>> {
    let list = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("knock"))
        .filter(room_users::event_sn.ge(since_sn))
        .select((room_users::room_id, room_users::state_data))
        .load::<(OwnedRoomId, Option<JsonValue>)>(&mut *connect()?)?
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

/// Check if a user has an account on this homeserver.
pub fn user_exists(user_id: &UserId) -> DataResult<bool> {
    let query = users::table.find(user_id);
    diesel_exists!(query, &mut *connect()?).map_err(Into::into)
}

pub fn get_user(user_id: &UserId) -> DataResult<Option<DbUser>> {
    users::table
        .find(user_id)
        .first::<DbUser>(&mut *connect()?)
        .optional()
        .map_err(Into::into)
}

/// Returns the number of users registered on this server.
pub fn count() -> DataResult<u64> {
    let count = user_passwords::table
        .select(count_distinct(user_passwords::user_id))
        .first::<i64>(&mut *connect()?)?;
    Ok(count as u64)
}

/// Returns a list of local users as list of usernames.
///
/// A user account is considered `local` if the length of it's password is greater then zero.
pub fn list_local_users() -> DataResult<Vec<OwnedUserId>> {
    user_passwords::table
        .select(user_passwords::user_id)
        .load::<OwnedUserId>(&mut *connect()?)
        .map_err(Into::into)
}

/// Returns the display_name of a user on this homeserver.
pub fn display_name(user_id: &UserId) -> DataResult<Option<String>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::display_name)
        .first::<Option<String>>(&mut *connect()?)
        .map_err(Into::into)
}

pub fn set_display_name(user_id: &UserId, display_name: Option<&str>) -> DataResult<()> {
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
pub fn avatar_url(user_id: &UserId) -> DataResult<Option<OwnedMxcUri>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::avatar_url)
        .first::<Option<OwnedMxcUri>>(&mut *connect()?)
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
}

/// Get the blurhash of a user.
pub fn blurhash(user_id: &UserId) -> DataResult<Option<String>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::blurhash)
        .first::<Option<String>>(&mut *connect()?)
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
) -> DataResult<()> {
    if let Some(signatures) = cross_signing_key.get_mut("signatures").and_then(|v| v.as_object_mut()) {
        // Don't allocate for the full size of the current signatures, but require
        // at most one resize if nothing is dropped
        let new_capacity = signatures.len() / 2;
        for (user, signature) in mem::replace(signatures, serde_json::Map::with_capacity(new_capacity)) {
            let sid =
                <&UserId>::try_from(user.as_str()).map_err(|_| DataError::internal("Invalid user ID in database."))?;
            if sender_id == Some(user_id) || sid == user_id || allowed_signatures(sid) {
                signatures.insert(user, signature);
            }
        }
    }

    Ok(())
}

pub fn deactivate(user_id: &UserId, doer_id: &UserId) -> DataResult<()> {
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
    if let Ok(Some(ignored)) = crate::user::data::get_global_data::<IgnoredUserListEvent>(
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
