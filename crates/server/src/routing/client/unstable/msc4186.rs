use std::cmp::{self, Ordering};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, hash_map};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::discovery::{
    Capabilities, CapabilitiesResBody, RoomVersionStability, RoomVersionsCapability, VersionsResBody,
};
use crate::core::client::search::{ResultCategories, SearchReqArgs, SearchReqBody, SearchResBody};
use crate::core::client::sync_events::{self, v5::*};
use crate::core::device::DeviceLists;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{AnySyncEphemeralRoomEvent, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::{Seqnum, UserId};
use crate::sync::{share_encrypted_room, DEFAULT_BUMP_TYPES};
use crate::{AppError, AppResult, AuthArgs, DepotExt, EmptyResult, JsonResult, empty_ok, hoops, json_ok};

/// `POST /_matrix/client/unstable/org.matrix.simplified_msc3575/sync`
/// ([MSC4186])
///
/// A simplified version of sliding sync ([MSC3575]).
///
/// Get all new events in a sliding window of rooms since the last sync or a
/// given point in time.
///
/// [MSC3575]: https://github.com/matrix-org/matrix-spec-proposals/pull/3575
/// [MSC4186]: https://github.com/matrix-org/matrix-spec-proposals/pull/4186
#[handler]
pub(super) async fn sync_events_v5(
    _aa: AuthArgs,
    args: SyncEventsReqArgs,
    mut body: JsonBody<SyncEventsReqBody>,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBody> {
    let authed = depot.authed_info()?;
    let body = body.into_inner();
    // Setup watchers, so if there's no response, we can wait for them
    let watcher = crate::watch(authed.user_id(), authed.device_id());

    let next_batch = crate::curr_sn()? + 1;

    let conn_id = body.conn_id.clone();

    let global_since_sn: i64 = args
        .pos
        .as_ref()
        .and_then(|string| string.parse().ok())
        .unwrap_or_default();

    if global_since_sn == 0 {
        if let Some(conn_id) = &body.conn_id {
            crate::user::forget_sync_request_connection(
                authed.user_id().clone(),
                authed.device_id().clone(),
                conn_id.clone(),
            )
        }
    }

    // Get sticky parameters from cache
    let known_rooms =
        crate::user::update_sync_request_with_cache(authed.user_id().to_owned(), authed.device_id().clone(), &mut body);

    let all_joined_rooms = crate::user::joined_rooms(&authed.user_id(), 0)?;

    let all_invited_rooms: Vec<_> = crate::room::state::invited_rooms(authed.user_id())
        .map(|r| r.0)
        .collect()
        .await;

    let all_knocked_rooms: Vec<_> = crate::room::state::knocked_rooms(authed.user_id())?;

    let all_rooms: Vec<&RoomId> = all_joined_rooms
        .iter()
        .map(AsRef::as_ref)
        .chain(all_invited_rooms.iter().map(AsRef::as_ref))
        .chain(all_knocked_rooms.iter().map(AsRef::as_ref))
        .collect();

    let all_joined_rooms = all_joined_rooms.iter().map(AsRef::as_ref).collect();
    let all_invited_rooms = all_invited_rooms.iter().map(AsRef::as_ref).collect();

    let pos = next_batch.clone().to_string();

    let mut todo_rooms: TodoRooms = BTreeMap::new();

    let sync_info: SyncInfo<'_> = (authed.user_id(), authed.device_id(), global_since_sn, &body);
    let mut res_body = SyncEventsResBody {
        txn_id: body.txn_id.clone(),
        pos,
        lists: BTreeMap::new(),
        rooms: BTreeMap::new(),
        extensions: Extensions {
            account_data: collect_account_data(sync_info).await,
            e2ee: collect_e2ee(sync_info, &all_joined_rooms).await?,
            to_device: collect_to_device(sync_info, next_batch).await,
            receipts: collect_receipts().await,
            typing: Typing::default(),
        },
    };

    handle_lists(
        sync_info,
        &all_invited_rooms,
        &all_joined_rooms,
        &all_rooms,
        &mut todo_rooms,
        &known_rooms,
        &mut res_body,
    )
    .await;

    fetch_subscriptions(sync_info, &known_rooms, &mut todo_rooms).await;

    res_body.rooms = process_rooms(
        authed.user_id(),
        next_batch,
        &all_invited_rooms,
        &todo_rooms,
        &mut res_body,
        &body,
    )
    .await?;

    if res_body.rooms.iter().all(|(id, r)| {
        r.timeline.is_empty() && r.required_state.is_empty() && !res_body.extensions.receipts.rooms.contains_key(id)
    }) && res_body
        .extensions
        .to_device
        .clone()
        .is_none_or(|to| to.events.is_empty())
    {
        // Hang a few seconds so requests are not spammed
        // Stop hanging if new info arrives
        let default = Duration::from_secs(30);
        let duration = cmp::min(body.timeout.unwrap_or(default), default);
        _ = tokio::time::timeout(duration, watcher).await;
    }

    trace!(
        rooms=?res_body.rooms.len(),
        account_data=?res_body.extensions.account_data.rooms.len(),
        receipts=?res_body.extensions.receipts.rooms.len(),
        "responding to request with"
    );
    json_ok(res_body)
}

#[allow(clippy::too_many_arguments)]
async fn handle_lists<'a>(
    (sender_id, sender_device, global_since_sn, body): SyncInfo<'_>,
    all_invited_rooms: &Vec<&'a RoomId>,
    all_joined_rooms: &Vec<&'a RoomId>,
    all_rooms: &Vec<&'a RoomId>,
    todo_rooms: &'a mut TodoRooms,
    known_rooms: &'a KnownRooms,
    res_body: &'_ mut SyncEventsResBody,
) -> KnownRooms {
    for (list_id, list) in &body.lists {
        let active_rooms = match list.filters.clone().and_then(|f| f.is_invite) {
            Some(true) => all_invited_rooms,
            Some(false) => all_joined_rooms,
            None => all_rooms,
        };

        let active_rooms = match list.filters.clone().map(|f| f.not_room_types) {
            Some(filter) if filter.is_empty() => active_rooms,
            Some(value) => &filter_rooms(active_rooms, &value, true).await,
            None => active_rooms,
        };

        let mut new_known_rooms: BTreeSet<OwnedRoomId> = BTreeSet::new();

        let ranges = list.ranges.clone();

        for mut range in ranges {
            range.0 = 0;
            range.1 = range.1.clamp(range.0, active_rooms.len());

            let room_ids = active_rooms[range.0..range.1].to_vec();

            let new_rooms: BTreeSet<OwnedRoomId> = room_ids.clone().into_iter().map(From::from).collect();
            new_known_rooms.extend(new_rooms);
            //new_known_rooms.extend(room_ids..cloned());
            for room_id in room_ids {
                let todo_room = todo_rooms
                    .entry(room_id.to_owned())
                    .or_insert((BTreeSet::new(), 0_usize, i64::MAX));

                let limit = list.room_details.timeline_limit.min(100);

                todo_room.0.extend(
                    list.room_details
                        .required_state
                        .iter()
                        .map(|(ty, sk)| (ty.clone(), sk.as_str().into())),
                );

                todo_room.1 = todo_room.1.max(limit);
                // 0 means unknown because it got out of date
                todo_room.2 = todo_room.2.min(
                    known_rooms
                        .get(list_id.as_str())
                        .and_then(|k| k.get(room_id))
                        .copied()
                        .unwrap_or(0),
                );
            }
        }
        response.lists.insert(
            list_id.clone(),
            sync_events::v5::SyncList {
                count: active_rooms.len(),
            },
        );

        if let Some(conn_id) = &body.conn_id {
            crate::user::update_sync_known_rooms(
                sender_id,
                sender_device.to_owned(),
                conn_id.clone(),
                list_id.clone(),
                new_known_rooms,
                global_since_sn,
            );
        }
    }
    BTreeMap::default()
}

async fn fetch_subscriptions(
    (sender_user, sender_device, global_since_sn, body): SyncInfo<'_>,
    known_rooms: &KnownRooms,
    todo_rooms: &mut TodoRooms,
) {
    let mut known_subscription_rooms = BTreeSet::new();
    for (room_id, room) in &body.room_subscriptions {
        if !crate::room::room_exists(room_id)? {
            continue;
        }
        let todo_room = todo_rooms
            .entry(room_id.clone())
            .or_insert((BTreeSet::new(), 0_usize, i64::MAX));

        let limit = room.timeline_limit;

        todo_room.0.extend(
            room.required_state
                .iter()
                .map(|(ty, sk)| (ty.clone(), sk.as_str().into())),
        );
        todo_room.1 = todo_room.1.max(usize_from_ruma(limit));
        // 0 means unknown because it got out of date
        todo_room.2 = todo_room.2.min(
            known_rooms
                .get("subscriptions")
                .and_then(|k| k.get(room_id))
                .copied()
                .unwrap_or(0),
        );
        known_subscription_rooms.insert(room_id.clone());
    }
    // where this went (protomsc says it was removed)
    //for r in body.unsubscribe_rooms {
    //	known_subscription_rooms.remove(&r);
    //	body.room_subscriptions.remove(&r);
    //}

    if let Some(conn_id) = &body.conn_id {
        crate::user::update_sync_known_rooms(
            sender_user,
            sender_device,
            conn_id.clone(),
            "subscriptions".to_owned(),
            known_subscription_rooms,
            global_since_sn,
        );
    }
}

async fn process_rooms(
    sender_id: &UserId,
    next_batch: Seqnum,
    all_invited_rooms: &[&RoomId],
    todo_rooms: &TodoRooms,
    response: &mut SyncEventsResBody,
    body: &SyncEventsReqBody,
) -> AppResult<BTreeMap<OwnedRoomId, sync_events::v5::Room>> {
    let mut rooms = BTreeMap::new();
    for (room_id, (required_state_request, timeline_limit, room_since_sn)) in todo_rooms {
        let mut timestamp: Option<_> = None;
        let mut invite_state = None;
        let (timeline_pdus, limited);
        let new_room_id: &RoomId = (*room_id).as_ref();
        if all_invited_rooms.contains(&new_room_id) {
            // TODO: figure out a timestamp we can use for remote invites
            invite_state = crate::room::state::invite_state(sender_id, room_id).await.ok();

            (timeline_pdus, limited) = (Vec::new(), true);
        } else {
            (timeline_pdus, limited) =
                match crate::sync::load_timeline(sender_id, room_id, roomsincecount, Some(next_batch), *timeline_limit as i64)
                    .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("Encountered missing timeline in {}, error {}", room_id, err);
                        continue;
                    }
                };
        }

        if body.extensions.account_data.enabled == Some(true) {
            response.extensions.account_data.rooms.insert(
                room_id.to_owned(),
                crate::account_data::changes_since(Some(room_id), sender_id, *roomsince, Some(next_batch))
                    .ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
                    .collect()
                    .await,
            );
        }

        let last_privateread_update =
            crate::room::read_receipt::last_privateread_update(sender_id, room_id).await > *roomsince;

        let private_read_event = if last_privateread_update {
            crate::room::read_receipt::private_read_get(room_id, sender_id)
                .await
                .ok()
        } else {
            None
        };

        let mut receipts: Vec<Raw<AnySyncEphemeralRoomEvent>> =
            crate::room::read_receipt::readreceipts_since(room_id, *roomsince)
                .filter_map(|(read_user, _ts, v)| async move {
                    crate::user::user_is_ignored(read_user, sender_id).await.or_some(v)
                })
                .collect()
                .await;

        if let Some(private_read_event) = private_read_event {
            receipts.push(private_read_event);
        }

        let receipt_size = receipts.len();

        if receipt_size > 0 {
            response
                .extensions
                .receipts
                .rooms
                .insert(room_id.clone(), pack_receipts(Box::new(receipts.into_iter())));
        }

        if roomsince != &0
            && timeline_pdus.is_empty()
            && response
                .extensions
                .account_data
                .rooms
                .get(room_id)
                .is_none_or(Vec::is_empty)
            && receipt_size == 0
        {
            continue;
        }

        let prev_batch = timeline_pdus
            .first()
            .map(|(sn, _)| if *sn == 0 { None } else { Some(sn.to_string()) })
            .flatten();

        let room_events: Vec<_> = timeline_pdus
            .iter()
            .stream()
            .filter_map(|item| ignored_filter(item.clone(), sender_id))
            .map(|(_, pdu)| pdu.to_sync_room_event())
            .collect()
            .await;

        for (_, pdu) in timeline_pdus {
            let ts = pdu.origin_server_ts;
            if DEFAULT_BUMP_TYPES.binary_search(&pdu.kind).is_ok() && timestamp.is_none_or(|time| time <= ts) {
                timestamp = Some(ts);
            }
        }

        let required_state = required_state_request
            .iter()
            .filter_map(|state| async move {
                crate::room::state::room_state_get(room_id, &state.0, &state.1)
                    .await
                    .map(|s| s.to_sync_state_event())
                    .ok()
            })
            .collect()
            .await;

        // Heroes
        let heroes: Vec<_> = crate::room::state::get_members(room_id)?.into_iter()
            .filter(|member| *member != sender_id)
            .filter_map(|user_id| {
                crate::room::state::get_member(room_id, user_id)
                    .map_ok(|memberevent| sync_events::v5::Hero {
                        user_id: user_id.into(),
                        name: memberevent.displayname,
                        avatar: memberevent.avatar_url,
                    })
                    .ok()
            })
            .take(5)
            .collect()
            .await;

        let name = match heroes.len().cmp(&(1_usize)) {
            Ordering::Greater => {
                let firsts = heroes[1..]
                    .iter()
                    .map(|h| h.name.clone().unwrap_or_else(|| h.user_id.to_string()))
                    .collect::<Vec<_>>()
                    .join(", ");

                let last = heroes[0].name.clone().unwrap_or_else(|| heroes[0].user_id.to_string());

                Some(format!("{firsts} and {last}"))
            }
            Ordering::Equal => Some(heroes[0].name.clone().unwrap_or_else(|| heroes[0].user_id.to_string())),
            Ordering::Less => None,
        };

        let heroes_avatar = if heroes.len() == 1 {
            heroes[0].avatar.clone()
        } else {
            None
        };

        rooms.insert(
            room_id.clone(),
            SlidingSyncRoom {
                name: crate::room::state::get_name(room_id).ok().flatten().or(name),
                avatar: match heroes_avatar {
                    Some(heroes_avatar) => Some(heroes_avatar),
                    _ => crate::room::state::get_avatar_url(room_id).ok().flatten(),
                },
                initial: Some(roomsince == &0),
                is_dm: None,
                invite_state,
                unread_notifications: sync_events::UnreadNotificationsCount {
                    highlight_count: Some(
                        crate::room::user::highlight_count(sender_id, room_id)?
                            .try_into()
                            .expect("notification count can't go that high"),
                    ),
                    notification_count: Some(
                        crate::room::user::notification_count(sender_id, room_id)?
                            .try_into()
                            .expect("notification count can't go that high"),
                    ),
                },
                timeline: room_events,
                required_state,
                prev_batch,
                limited,
                joined_count: Some(
                    crate::room::state::room_joined_count(room_id)
                        .await
                        .unwrap_or(0)
                        .try_into()
                        .unwrap_or_else(|_| 0),
                ),
                invited_count: Some(
                    crate::room::state::room_invited_count(room_id)
                        .await
                        .unwrap_or(0)
                        .try_into()
                        .unwrap_or_else(|_| 0),
                ),
                num_live: None, // Count events in timeline greater than global sync counter
                bump_stamp: timestamp,
                heroes: Some(heroes),
            },
        );
    }
    Ok(rooms)
}
async fn collect_account_data(
    (sender_id, _, global_since_sn, body): (&UserId, &DeviceId, Seqnum, &SyncEventsReqBody),
) -> sync_events::v5::AccountData {
    let mut account_data = sync_events::v5::AccountData {
        global: Vec::new(),
        rooms: BTreeMap::new(),
    };

    if !body.extensions.account_data.enabled.unwrap_or(false) {
        return sync_events::v5::AccountData::default();
    }

    account_data.global = crate::account_data::changes_since(None, sender_id, global_since_sn, None)
        .ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Global))
        .collect()
        .await;

    if let Some(rooms) = &body.extensions.account_data.rooms {
        for room in rooms {
            account_data.rooms.insert(
                room.clone(),
                crate::account_data::changes_since(Some(room), sender_id, global_since_sn, None)
                    .ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
                    .collect()
                    .await,
            );
        }
    }

    account_data
}

async fn collect_e2ee<'a>(
    (sender_id, sender_device, global_since_sn, body): (&UserId, &DeviceId, Seqnum, &SyncEventsReqBody),
    all_joined_rooms: &'a Vec<&'a RoomId>,
) -> AppResult<sync_events::v5::E2ee> {
    if !body.extensions.e2ee.enabled.unwrap_or(false) {
        return Ok(sync_events::v5::E2ee::default());
    }
    let mut left_encrypted_users = HashSet::new(); // Users that have left any encrypted rooms the sender was in
    let mut device_list_changes = HashSet::new();
    let mut device_list_left = HashSet::new();
    // Look for device list updates of this account
    device_list_changes.extend(crate::user::get_keys_changed_users(sender_id, global_since_sn, None)?);

    for room_id in all_joined_rooms {
        let Ok(current_frame_id) = crate::room::state::get_room_frame_id(room_id, None) else {
            error!("Room {room_id} has no state");
            continue;
        };

        let since_frame_id = crate::room::user::get_token_frame_id(room_id, global_since_sn)
            .await
            .ok();

        let encrypted_room = crate::room::state::get_state(current_frame_id, &StateEventType::RoomEncryption, "")
            .await
            .is_ok();

        if let Some(since_frame_id) = since_frame_id {
            // Skip if there are only timeline changes
            if since_frame_id == current_frame_id {
                continue;
            }

            let since_encryption =
                crate::room::state::get_state(since_frame_id, &StateEventType::RoomEncryption, "").await;

            let since_sender_member: Option<RoomMemberEventContent> =
                crate::room::state::state_get_content(since_frame_id, &StateEventType::RoomMember, sender_id.as_str())
                    .ok()
                    .await;

            let joined_since_last_sync = since_sender_member
                .as_ref()
                .is_none_or(|member| member.membership != MembershipState::Join);

            let new_encrypted_room = encrypted_room && since_encryption.is_err();

            if encrypted_room {
                let current_state_ids: HashMap<_, OwnedEventId> =
                    crate::room::state::get_full_state_ids(current_frame_id);

                let since_state_ids: HashMap<_, _> = crate::room::state::get_full_state_ids(since_frame_id)?;

                for (key, id) in current_state_ids {
                    if since_state_ids.get(&key) != Some(&id) {
                        let Ok(Some(pdu)) = crate::room::timeline::get_pdu(&id) else {
                            error!("Pdu in state not found: {id}");
                            continue;
                        };
                        if pdu.event_ty == TimelineEventType::RoomMember {
                            if let Some(Ok(user_id)) = pdu.state_key.as_deref().map(UserId::parse) {
                                if user_id == sender_id {
                                    continue;
                                }

                                let content: RoomMemberEventContent = pdu.get_content()?;
                                match content.membership {
                                    MembershipState::Join => {
                                        // A new user joined an encrypted room
                                        if !share_encrypted_room(sender_id, user_id, Some(room_id))? {
                                            device_list_changes.insert(user_id.to_owned());
                                        }
                                    }
                                    MembershipState::Leave => {
                                        // Write down users that have left encrypted rooms we
                                        // are in
                                        left_encrypted_users.insert(user_id.to_owned());
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if joined_since_last_sync || new_encrypted_room {
                    // If the user is in a new encrypted room, give them all joined users
                    device_list_changes.extend(
                        crate::room::state::get_members(room_id)?.into_iter()
                            // Don't send key updates from the sender to the sender
                            .filter(|user_id| sender_id != *user_id)
                            // Only send keys if the sender doesn't share an encrypted room with the target
                            // already
                            .filter_map(|user_id| {
                                if Ok(true) = share_encrypted_room(sender_id, user_id, Some(room_id)) {
									Some(user_id.to_owned())
								}else {
									None
								}
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
        }
        // Look for device list updates in this room
        device_list_changes.extend(crate::room::keys_changed_users(room_id, global_since_sn, None)?);
    }

    for user_id in left_encrypted_users {
        let Ok(share_encrypted_room) = share_encrypted_room(sender_id, &user_id, None) else {
            continue;
        };

        // If the user doesn't share an encrypted room with the target anymore, we need
        // to tell them
        if !share_encrypted_room {
            device_list_left.insert(user_id);
        }
    }

    Ok(E2ee {
        device_lists: DeviceLists {
            changed: device_list_changes.into_iter().collect(),
            left: device_list_left.into_iter().collect(),
        },
        device_one_time_keys_count: crate::user::count_one_time_keys(sender_id, sender_device)?,
        device_unused_fallback_key_types: None,
    })
}

async fn collect_to_device(
    (sender_id, sender_device, global_since_sn, body): SyncInfo<'_>,
    next_batch: Seqnum,
) -> Option<sync_events::v5::ToDevice> {
    if !body.extensions.to_device.enabled.unwrap_or(false) {
        return None;
    }

    crate::user::remove_to_device_events(sender_id, sender_device, global_since_sn).ok()?;

    Some(sync_events::v5::ToDevice {
        next_batch: next_batch.to_string(),
        events: crate::user::get_to_device_events(sender_id, sender_device, None, Some(next_batch)).ok()?,
    })
}

async fn collect_receipts() -> sync_events::v5::Receipts {
    sync_events::v5::Receipts { rooms: BTreeMap::new() }
    // TODO: get explicitly requested read receipts
}
