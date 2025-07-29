use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, LazyLock, Mutex},
};

use crate::core::client::sync_events;
use crate::core::identifiers::*;

pub struct SlidingSyncCache {
    lists: BTreeMap<String, sync_events::v5::ReqList>,
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v5::RoomSubscription>,
    known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, i64>>, // For every room, the room_since_sn number
    extensions: sync_events::v5::ExtensionsConfig,
}

pub static CONNECTIONS: LazyLock<
    Mutex<BTreeMap<(OwnedUserId, OwnedDeviceId, String), Arc<Mutex<SlidingSyncCache>>>>,
> = LazyLock::new(|| Default::default());

pub fn forget_sync_request_connection(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    conn_id: String,
) {
    CONNECTIONS
        .lock()
        .unwrap()
        .remove(&(user_id, device_id, conn_id));
}
/// load params from cache if body doesn't contain it, as long as it's allowed
/// in some cases we may need to allow an empty list as an actual value
fn list_or_sticky<T: Clone>(target: &mut Vec<T>, cached: &Vec<T>) {
    if target.is_empty() {
        target.clone_from(cached);
    }
}
fn some_or_sticky<T>(target: &mut Option<T>, cached: Option<T>) {
    if target.is_none() {
        *target = cached;
    }
}
pub fn update_sync_request_with_cache(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    req_body: &mut sync_events::v5::SyncEventsReqBody,
) -> BTreeMap<String, BTreeMap<OwnedRoomId, i64>> {
    let Some(conn_id) = req_body.conn_id.clone() else {
        return BTreeMap::new();
    };

    let mut cache = CONNECTIONS.lock().unwrap();
    let cached = Arc::clone(
        cache
            .entry((user_id, device_id, conn_id))
            .or_insert_with(|| {
                Arc::new(Mutex::new(SlidingSyncCache {
                    lists: BTreeMap::new(),
                    subscriptions: BTreeMap::new(),
                    known_rooms: BTreeMap::new(),
                    extensions: sync_events::v5::ExtensionsConfig::default(),
                }))
            }),
    );
    let cached = &mut cached.lock().unwrap();
    drop(cache);

    for (list_id, list) in &mut req_body.lists {
        if let Some(cached_list) = cached.lists.get(list_id) {
            list_or_sticky(
                &mut list.room_details.required_state,
                &cached_list.room_details.required_state,
            );
            some_or_sticky(&mut list.include_heroes, cached_list.include_heroes);

            match (&mut list.filters, cached_list.filters.clone()) {
                (Some(filters), Some(cached_filters)) => {
                    some_or_sticky(&mut filters.is_invite, cached_filters.is_invite);
                    // TODO (morguldir): Find out how a client can unset this, probably need
                    // to change into an option inside palpo
                    list_or_sticky(&mut filters.not_room_types, &cached_filters.not_room_types);
                }
                (_, Some(cached_filters)) => list.filters = Some(cached_filters),
                (Some(list_filters), _) => list.filters = Some(list_filters.clone()),
                (..) => {}
            }
        }
        cached.lists.insert(list_id.clone(), list.clone());
    }

    cached
        .subscriptions
        .extend(req_body.room_subscriptions.clone().into_iter());
    req_body
        .room_subscriptions
        .extend(cached.subscriptions.clone().into_iter());

    req_body.extensions.e2ee.enabled = req_body
        .extensions
        .e2ee
        .enabled
        .or(cached.extensions.e2ee.enabled);

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
    req_body.extensions.account_data.lists = req_body
        .extensions
        .account_data
        .lists
        .clone()
        .or(cached.extensions.account_data.lists.clone());
    req_body.extensions.account_data.rooms = req_body
        .extensions
        .account_data
        .rooms
        .clone()
        .or(cached.extensions.account_data.rooms.clone());

    some_or_sticky(
        &mut req_body.extensions.typing.enabled,
        cached.extensions.typing.enabled,
    );
    some_or_sticky(
        &mut req_body.extensions.typing.rooms,
        cached.extensions.typing.rooms.clone(),
    );
    some_or_sticky(
        &mut req_body.extensions.typing.lists,
        cached.extensions.typing.lists.clone(),
    );
    some_or_sticky(
        &mut req_body.extensions.receipts.enabled,
        cached.extensions.receipts.enabled,
    );
    some_or_sticky(
        &mut req_body.extensions.receipts.rooms,
        cached.extensions.receipts.rooms.clone(),
    );
    some_or_sticky(
        &mut req_body.extensions.receipts.lists,
        cached.extensions.receipts.lists.clone(),
    );

    cached.extensions = req_body.extensions.clone();
    cached.known_rooms.clone()
}

pub fn update_sync_subscriptions(
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    conn_id: String,
    subscriptions: BTreeMap<OwnedRoomId, sync_events::v5::RoomSubscription>,
) {
    let mut cache = CONNECTIONS.lock().unwrap();
    let cached = Arc::clone(
        cache
            .entry((user_id, device_id, conn_id))
            .or_insert_with(|| {
                Arc::new(Mutex::new(SlidingSyncCache {
                    lists: BTreeMap::new(),
                    subscriptions: BTreeMap::new(),
                    known_rooms: BTreeMap::new(),
                    extensions: sync_events::v5::ExtensionsConfig::default(),
                }))
            }),
    );
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
    let mut cache = CONNECTIONS.lock().unwrap();
    let cached = Arc::clone(
        cache
            .entry((user_id, device_id, conn_id))
            .or_insert_with(|| {
                Arc::new(Mutex::new(SlidingSyncCache {
                    lists: BTreeMap::new(),
                    subscriptions: BTreeMap::new(),
                    known_rooms: BTreeMap::new(),
                    extensions: sync_events::v5::ExtensionsConfig::default(),
                }))
            }),
    );
    let cached = &mut cached.lock().unwrap();
    drop(cache);

    for (roomid, lastsince) in cached
        .known_rooms
        .entry(list_id.clone())
        .or_default()
        .iter_mut()
    {
        if !new_cached_rooms.contains(roomid) {
            *lastsince = 0;
        }
    }
    let list = cached.known_rooms.entry(list_id).or_default();
    for roomid in new_cached_rooms {
        list.insert(roomid, global_since_sn);
    }
}
