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
mod presence;
pub use presence::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
    sync::{Arc, LazyLock, Mutex},
};

use diesel::dsl::count_distinct;
use diesel::prelude::*;
use palpo_core::JsonValue;

use crate::core::client::sync_events::{
    ExtensionsConfigV4, RoomSubscriptionV4, SyncEventsReqBodyV4, SyncRequestListV4,
};
use crate::core::events::AnyStrippedStateEvent;
use crate::core::identifiers::*;
use crate::core::serde::RawJson;
use crate::core::{OwnedMxcUri, OwnedRoomId, UnixMillis};
use crate::schema::*;
use crate::{db, diesel_exists, AppError, AppResult};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = users)]
pub struct DbUser {
    pub id: OwnedUserId,
    pub user_type: Option<String>,
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

#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = users)]
pub struct NewDbUser {
    pub id: OwnedUserId,
    pub user_type: Option<String>,
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
    lists: BTreeMap<String, SyncRequestListV4>,
    subscriptions: BTreeMap<OwnedRoomId, RoomSubscriptionV4>,
    known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, i64>>, // For every room, the room_since_sn number
    extensions: ExtensionsConfigV4,
}

/// Returns an iterator over all rooms this user joined.
pub fn joined_rooms(user_id: &UserId, since_sn: i64) -> AppResult<Vec<OwnedRoomId>> {
    room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("join"))
        .filter(room_users::event_sn.ge(since_sn))
        .select(room_users::room_id)
        .load(&mut db::connect()?)
        .map_err(Into::into)
}

pub fn forget_before_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<Option<i64>> {
    room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::forgotten.is_not_null())
        .select(room_users::event_sn)
        .first::<i64>(&mut db::connect()?)
        .optional()
        .map_err(Into::into)
}

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
        .load::<(OwnedRoomId, Option<JsonValue>)>(&mut *db::connect()?)?
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
    diesel_exists!(query, &mut *db::connect()?).map_err(Into::into)
}

pub fn get_user(user_id: &UserId) -> AppResult<Option<DbUser>> {
    users::table
        .find(user_id)
        .first::<DbUser>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}

pub fn create_user(user_id: impl Into<OwnedUserId>, password: Option<&str>) -> AppResult<DbUser> {
    let user_id = user_id.into();
    let user = diesel::insert_into(users::table)
        .values(NewDbUser {
            id: user_id.clone(),
            user_type: None,
            is_admin: false,
            is_guest: password.is_none(),
            appservice_id: None,
            created_at: UnixMillis::now(),
        })
        .get_result::<DbUser>(&mut *db::connect()?)?;
    if let Some(password) = password {
        let hash = crate::utils::hash_password(password)?;
        diesel::insert_into(user_passwords::table)
            .values(NewDbPassword {
                user_id,
                hash,
                created_at: UnixMillis::now(),
            })
            .execute(&mut db::connect()?)?;
    }
    Ok(user)
}

pub fn forget_sync_request_connection(user_id: OwnedUserId, device_id: OwnedDeviceId, conn_id: String) {
    CONNECTIONS.lock().unwrap().remove(&(user_id, device_id, conn_id));
}

pub fn update_sync_request_with_cache(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    req_body: &mut SyncEventsReqBodyV4,
) -> BTreeMap<String, BTreeMap<OwnedRoomId, i64>> {
    let Some(conn_id) = req_body.conn_id.clone() else {
        return BTreeMap::new();
    };
    let connections = CONNECTIONS;

    let mut cache = connections.lock().unwrap();
    let cached = Arc::clone(cache.entry((user_id, device_id, conn_id)).or_insert_with(|| {
        Arc::new(Mutex::new(SlidingSyncCache {
            lists: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            known_rooms: BTreeMap::new(),
            extensions: ExtensionsConfigV4::default(),
        }))
    }));
    let cached = &mut cached.lock().unwrap();
    drop(cache);

    for (list_id, list) in &mut req_body.lists {
        if let Some(cached_list) = cached.lists.get(list_id) {
            if list.sort.is_empty() {
                list.sort = cached_list.sort.clone();
            };
            if list.room_details.required_state.is_empty() {
                list.room_details.required_state = cached_list.room_details.required_state.clone();
            };
            list.room_details.timeline_limit = list
                .room_details
                .timeline_limit
                .or(cached_list.room_details.timeline_limit);
            list.include_old_rooms = list.include_old_rooms.clone().or(cached_list.include_old_rooms.clone());
            match (&mut list.filters, cached_list.filters.clone()) {
                (Some(list_filters), Some(cached_filters)) => {
                    list_filters.is_dm = list_filters.is_dm.or(cached_filters.is_dm);
                    if list_filters.spaces.is_empty() {
                        list_filters.spaces = cached_filters.spaces;
                    }
                    list_filters.is_encrypted = list_filters.is_encrypted.or(cached_filters.is_encrypted);
                    list_filters.is_invite = list_filters.is_invite.or(cached_filters.is_invite);
                    if list_filters.room_types.is_empty() {
                        list_filters.room_types = cached_filters.room_types;
                    }
                    if list_filters.not_room_types.is_empty() {
                        list_filters.not_room_types = cached_filters.not_room_types;
                    }
                    list_filters.room_name_like = list_filters.room_name_like.clone().or(cached_filters.room_name_like);
                    if list_filters.tags.is_empty() {
                        list_filters.tags = cached_filters.tags;
                    }
                    if list_filters.not_tags.is_empty() {
                        list_filters.not_tags = cached_filters.not_tags;
                    }
                }
                (_, Some(cached_filters)) => list.filters = Some(cached_filters),
                (Some(list_filters), _) => list.filters = Some(list_filters.clone()),
                (_, _) => {}
            }
            if list.bump_event_types.is_empty() {
                list.bump_event_types = cached_list.bump_event_types.clone();
            };
        }
        cached.lists.insert(list_id.clone(), list.clone());
    }

    cached
        .subscriptions
        .extend(req_body.room_subscriptions.clone().into_iter());
    req_body
        .room_subscriptions
        .extend(cached.subscriptions.clone().into_iter());

    req_body.extensions.e2ee.enabled = req_body.extensions.e2ee.enabled.or(cached.extensions.e2ee.enabled);

    req_body.extensions.to_device.enabled = req_body
        .extensions
        .to_device
        .enabled
        .or(cached.extensions.to_device.enabled);

    req_body.extensions.account_data.enabled = req_body
        .extensions
        .account_data
        .enabled
        .or(cached.extensions.account_data.enabled);
    req_body.extensions.account_data.lists = req_body.extensions.account_data.lists.clone().or(cached
        .extensions
        .account_data
        .lists
        .clone());
    req_body.extensions.account_data.rooms = req_body.extensions.account_data.rooms.clone().or(cached
        .extensions
        .account_data
        .rooms
        .clone());

    cached.extensions = req_body.extensions.clone();
    cached.known_rooms.clone()
}

pub fn update_sync_subscriptions(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    conn_id: String,
    subscriptions: BTreeMap<OwnedRoomId, RoomSubscriptionV4>,
) {
    let connections = CONNECTIONS;

    let mut cache = connections.lock().unwrap();
    let cached = Arc::clone(cache.entry((user_id, device_id, conn_id)).or_insert_with(|| {
        Arc::new(Mutex::new(SlidingSyncCache {
            lists: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            known_rooms: BTreeMap::new(),
            extensions: ExtensionsConfigV4::default(),
        }))
    }));
    let cached = &mut cached.lock().unwrap();
    drop(cache);

    cached.subscriptions = subscriptions;
}

pub fn update_sync_known_rooms(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    conn_id: String,
    list_id: String,
    new_cached_rooms: BTreeSet<OwnedRoomId>,
    global_since_sn: i64,
) {
    let connections = CONNECTIONS;

    let mut cache = connections.lock().unwrap();
    let cached = Arc::clone(cache.entry((user_id, device_id, conn_id)).or_insert_with(|| {
        Arc::new(Mutex::new(SlidingSyncCache {
            lists: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            known_rooms: BTreeMap::new(),
            extensions: ExtensionsConfigV4::default(),
        }))
    }));
    let cached = &mut cached.lock().unwrap();
    drop(cache);

    for (roomid, lastsince) in cached.known_rooms.entry(list_id.clone()).or_default().iter_mut() {
        if !new_cached_rooms.contains(roomid) {
            *lastsince = 0;
        }
    }
    let list = cached.known_rooms.entry(list_id).or_default();
    for roomid in new_cached_rooms {
        list.insert(roomid, global_since_sn);
    }
}

/// Returns the number of users registered on this server.
pub fn count() -> AppResult<u64> {
    let count = user_passwords::table
        .select(count_distinct(user_passwords::user_id))
        .first::<i64>(&mut *db::connect()?)?;
    Ok(count as u64)
}

/// Returns a list of local users as list of usernames.
///
/// A user account is considered `local` if the length of it's password is greater then zero.
pub fn list_local_users() -> AppResult<Vec<OwnedUserId>> {
    user_passwords::table
        .select(user_passwords::user_id)
        .load::<OwnedUserId>(&mut *db::connect()?)
        .map_err(Into::into)
}

/// Returns the display_name of a user on this homeserver.
pub fn display_name(user_id: &UserId) -> AppResult<Option<String>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::display_name)
        .first::<Option<String>>(&mut *db::connect()?)
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
}

pub fn set_display_name(user_id: &UserId, display_name: Option<&str>) -> AppResult<()> {
    diesel::update(
        user_profiles::table
            .filter(user_profiles::user_id.eq(user_id.as_str()))
            .filter(user_profiles::room_id.is_null()),
    )
    .set(user_profiles::display_name.eq(display_name))
    .execute(&mut db::connect()?)
    .map(|_| ())
    .map_err(Into::into)
}

/// Get the avatar_url of a user.
pub fn avatar_url(user_id: &UserId) -> AppResult<Option<OwnedMxcUri>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.is_null())
        .select(user_profiles::avatar_url)
        .first::<Option<OwnedMxcUri>>(&mut *db::connect()?)
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
        .first::<Option<String>>(&mut *db::connect()?)
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
        .execute(&mut db::connect()?)?;

    diesel::delete(user_threepids::table.filter(user_threepids::user_id.eq(user_id))).execute(&mut db::connect()?)?;
    diesel::delete(user_access_tokens::table.filter(user_access_tokens::user_id.eq(user_id)))
        .execute(&mut db::connect()?)?;

    Ok(())
}
