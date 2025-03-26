use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
    sync::{Arc, LazyLock, Mutex},
};

use diesel::dsl::count_distinct;
use diesel::prelude::*;
use palpo_core::JsonValue;

use crate::core::client::sync_events;
use crate::core::events::AnyStrippedStateEvent;
use crate::core::events::GlobalAccountDataEventType;
use crate::core::identifiers::*;
use crate::core::serde::RawJson;
use crate::core::{OwnedMxcUri, OwnedRoomId, UnixMillis};
use crate::schema::*;
use crate::{AppError, AppResult, db, diesel_exists};
use palpo_core::events::ignored_user_list::IgnoredUserListEvent;

pub struct SlidingSyncCache {
    lists: BTreeMap<String, sync_events::v4::ReqList>,
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
    known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, i64>>, // For every room, the room_since_sn number
    extensions: sync_events::v4::ExtensionsConfig,
}

pub const CONNECTIONS: LazyLock<Mutex<BTreeMap<(OwnedUserId, OwnedDeviceId, String), Arc<Mutex<SlidingSyncCache>>>>> =
    LazyLock::new(|| Default::default());

pub fn forget_sync_request_connection(user_id: OwnedUserId, device_id: OwnedDeviceId, conn_id: String) {
    CONNECTIONS
        .lock()
        .unwrap()
        .remove(&(user_id, device_id, conn_id));
}

pub fn remembered(user_id: OwnedUserId, device_id: OwnedDeviceId, conn_id: String) -> bool {
    CONNECTIONS.lock().unwrap().contains_key(&(user_id, device_id, conn_id))
}

pub fn update_sync_request_with_cache(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    req_body: &mut sync_events::v4::SyncEventsReqBody,
) -> BTreeMap<String, BTreeMap<OwnedRoomId, i64>> {
    let Some(conn_id) = req_body.conn_id.clone() else {
        return BTreeMap::new();
    };
    let connections = CONNECTIONS;

    let mut cache = connections.lock().unwrap();
    let cached = Arc::clone(
        cache
            .entry((user_id, device_id, conn_id))
            .or_insert_with(|| {
                Arc::new(Mutex::new(SlidingSyncCache {
                    lists: BTreeMap::new(),
                    subscriptions: BTreeMap::new(),
                    known_rooms: BTreeMap::new(),
                    extensions: sync_events::v4::ExtensionsConfig::default(),
                }))
            }),
    );
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
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
) {
    let connections = CONNECTIONS;

    let mut cache = connections.lock().unwrap();
    let cached = Arc::clone(cache.entry((user_id, device_id, conn_id)).or_insert_with(|| {
        Arc::new(Mutex::new(SlidingSyncCache {
            lists: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            known_rooms: BTreeMap::new(),
            extensions: sync_events::v4::ExtensionsConfig::default(),
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
            extensions: sync_events::v4::ExtensionsConfig::default(),
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
