use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet, hash_map};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::MatrixError;
use crate::core::RawJson;
use crate::core::client::discovery::{
    Capabilities, CapabilitiesResBody, RoomVersionStability, RoomVersionsCapability, VersionsResBody,
};
use crate::core::client::search::{ResultCategories, SearchReqArgs, SearchReqBody, SearchResBody};
use crate::core::client::sync_events::{self, v4::*};
use crate::core::device::DeviceLists;
use crate::core::events::RoomAccountDataEventType;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{AnyRawAccountDataEvent, AnySyncEphemeralRoomEvent, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::room::filter_rooms;
use crate::room::receipt::pack_receipts;
use crate::sync::{DEFAULT_BUMP_TYPES, share_encrypted_room};
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, empty_ok, extract_variant, hoops, json_ok};

pub(crate) const SINGLE_CONNECTION_SYNC: &str = "single_connection_sync";

/// POST `/_matrix/client/unstable/org.matrix.msc3575/sync`
///
/// Sliding Sync endpoint (future endpoint: `/_matrix/client/v4/sync`)
#[handler]
pub(super) async fn sync_events_v4(
    _aa: AuthArgs,
    args: SyncEventsReqArgs,
    mut body: JsonBody<SyncEventsReqBody>,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBody> {
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();

    // Setup watchers, so if there's no response, we can wait for them
    let watcher = crate::watch(sender_id, authed.device_id());

    let next_batch = crate::curr_sn()? + 1;
    let body = body.into_inner();

    let conn_id = body
        .conn_id
        .clone()
        .unwrap_or_else(|| SINGLE_CONNECTION_SYNC.to_owned());

    let global_since_sn: i64 = args
        .pos
        .as_ref()
        .and_then(|string| string.parse().ok())
        .unwrap_or_default();

    if global_since_sn != 0 && !crate::sync_v4::remembered(sender_id.to_owned(), authed.device_id().to_owned(), conn_id)
    {
        debug!("Restarting sync stream because it was gone from the database");
        return Err(MatrixError::unknown_pos("Connection data lost since last time").into());
    }

    if global_since_sn == 0 {
        crate::sync_v4::forget_sync_request_connection(sender_id.to_owned(), authed.device_id().to_owned(), conn_id)
    }

    // Get sticky parameters from cache
    let known_rooms =
        crate::sync_v4::update_sync_request_with_cache(sender_id.to_owned(), authed.device_id().to_owned(), &mut body);

    let all_joined_rooms: Vec<&RoomId> = crate::user::joined_rooms(sender_id, 0)?
        .iter()
        .map(|r| r.as_ref())
        .collect();
    let all_invited_rooms: Vec<&RoomId> = crate::user::invited_rooms(sender_id, 0)?
        .into_iter()
        .map(|r| r.0.as_ref())
        .collect();
    let all_knocked_rooms: Vec<&RoomId> = crate::user::knocked_rooms(sender_id, 0)?
        .into_iter()
        .map(|r| r.0.as_ref())
        .collect();

    let mut all_rooms: Vec<&RoomId> = all_joined_rooms
        .iter()
        .map(AsRef::as_ref)
        .chain(all_invited_rooms.iter().map(AsRef::as_ref))
        .chain(all_knocked_rooms.iter().map(AsRef::as_ref))
        .collect();
    all_rooms.dedup();

    if body.extensions.to_device.enabled.unwrap_or(false) {
        crate::user::remove_to_device_events(sender_id, authed.device_id(), global_since_sn - 1)?;
    }

    let mut left_encrypted_users = HashSet::new(); // Users that have left any encrypted rooms the sender was in
    let mut device_list_changes = HashSet::new();
    let mut device_list_left = HashSet::new();

    let mut receipts = sync_events::v4::Receipts { rooms: BTreeMap::new() };
    let mut account_data = sync_events::v4::AccountData {
        global: Vec::new(),
        rooms: BTreeMap::new(),
    };

    if body.extensions.account_data.enabled.unwrap_or(false) {
        account_data.global = crate::user::data_changes(None, sender_id, global_since_sn, Some(next_batch))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Global))
            .collect()
            .await;

        if let Some(rooms) = body.extensions.account_data.rooms {
            for room in rooms {
                account_data.rooms.insert(
                    room.clone(),
                    crate::user::data_changes(Some(&room), sender_id, global_since_sn, Some(next_batch))
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
                        .collect()
                        .await,
                );
            }
        }
    }

    if body.extensions.e2ee.enabled.unwrap_or(false) {
        // Look for device list updates of this account
        device_list_changes.extend(crate::user::keys_changed_users(sender_id, global_since_sn, None)?);

        for room_id in &all_joined_rooms {
            let Some(current_frame_id) = crate::room::state::get_room_frame_id(&room_id, None)? else {
                error!("Room {} has no state", room_id);
                continue;
            };

            let since_frame_id = crate::room::user::get_last_event_frame_id(&room_id, global_since_sn)?;

            let encrypted_room =
                crate::room::state::get_state(current_frame_id, &StateEventType::RoomEncryption, "")?.is_some();

            if let Some(since_frame_id) = since_frame_id {
                // Skip if there are only timeline changes
                if since_frame_id == current_frame_id {
                    continue;
                }

                let since_encryption =
                    crate::room::state::get_state(since_frame_id, &StateEventType::RoomEncryption, "")?;

                let since_sender_member: Option<RoomMemberEventContent> =
                    crate::room::state::get_state(since_frame_id, &StateEventType::RoomMember, sender_id.as_str())?
                        .map(|pdu| {
                            serde_json::from_str(pdu.content.get())
                                .map_err(|_| AppError::public("Invalid PDU in database."))
                        })
                        .transpose()?;

                let joined_since_last_sync = crate::room::user::joined_sn(sender_id, &room_id)? >= global_since_sn;

                let new_encrypted_room = encrypted_room && since_encryption.is_none();

                if encrypted_room {
                    let current_state_ids = crate::room::state::get_full_state_ids(current_frame_id)?;
                    let since_state_ids = crate::room::state::get_full_state_ids(since_frame_id)?;

                    for (key, id) in current_state_ids {
                        if since_state_ids.get(&key) != Some(&id) {
                            let pdu = match crate::room::timeline::get_pdu(&id)? {
                                Some(pdu) => pdu,
                                None => {
                                    error!("Pdu in state not found: {}", id);
                                    continue;
                                }
                            };
                            if pdu.event_ty == TimelineEventType::RoomMember {
                                if let Some(state_key) = &pdu.state_key {
                                    let Ok(user_id) = UserId::parse(state_key) else {
                                        tracing::error!("Invalid UserId in member PDU.");
                                        continue;
                                    };

                                    if &user_id == authed.user_id() {
                                        continue;
                                    }

                                    let new_membership =
                                        serde_json::from_str::<RoomMemberEventContent>(pdu.content.get())
                                            .map_err(|_| AppError::public("Invalid PDU in database."))?
                                            .membership;

                                    match new_membership {
                                        MembershipState::Join => {
                                            // A new user joined an encrypted room
                                            if !crate::sync::share_encrypted_room(
                                                authed.user_id(),
                                                &user_id,
                                                Some(&room_id),
                                            )? {
                                                device_list_changes.insert(user_id);
                                            }
                                        }
                                        MembershipState::Leave => {
                                            // Write down users that have left encrypted rooms we are in
                                            left_encrypted_users.insert(user_id);
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
                            crate::room::get_joined_users(&room_id, None)?
                                .into_iter()
                                .filter(|user_id| {
                                    // Don't send key updates from the sender to the sender
                                    sender_id != user_id
                                })
                                .filter(|user_id| {
                                    // Only send keys if the sender doesn't share an encrypted room with the target already
                                    !crate::bl::sync::share_encrypted_room(sender_id, user_id, Some(&room_id))
                                        .unwrap_or(false)
                                }),
                        );
                    }
                }
            }
            // Look for device list updates in this room
            device_list_changes.extend(crate::room::keys_changed_users(&room_id, global_since_sn, None)?.into_iter());
        }
        for user_id in left_encrypted_users {
            let dont_share_encrypted_room = !share_encrypted_room(sender_id, &user_id, None)?;
            // If the user doesn't share an encrypted room with the target anymore, we need to tell
            // them
            if dont_share_encrypted_room {
                device_list_left.insert(user_id);
            }
        }
    }

    let mut lists = BTreeMap::new();
    let mut todo_rooms: BTreeMap<OwnedRoomId, (BTreeSet<_>, _, _)> = BTreeMap::new(); // and required state

    for (list_id, list) in &body.lists {
        let active_rooms = match list.filters.clone().and_then(|f| f.is_invite) {
            Some(true) => &all_invited_rooms,
            Some(false) => &all_joined_rooms,
            None => &all_rooms,
        };

        let active_rooms = match list.filters.clone().map(|f| f.not_room_types) {
            Some(filter) if filter.is_empty() => active_rooms.clone(),
            Some(value) => filter_rooms(active_rooms, &value, true),
            None => active_rooms.clone(),
        };

        let active_rooms = match list.filters.clone().map(|f| f.room_types) {
            Some(filter) if filter.is_empty() => active_rooms.clone(),
            Some(value) => filter_rooms(&active_rooms, &value, false),
            None => active_rooms,
        };

        let mut new_known_rooms = BTreeSet::new();
        let ranges = list.ranges.clone();
        lists.insert(
            list_id.clone(),
            SyncList {
                ops: ranges
                    .into_iter()
                    .map(|mut r| {
                        r.0 = r.0.clamp(0, active_rooms.len() as u64 - 1);
                        r.1 = r.1.clamp(r.0, active_rooms.len() as u64 - 1);

                        let room_ids = if !active_rooms.is_empty() {
                            active_rooms[(u64::from(r.0) as usize)..=(u64::from(r.1) as usize)]
                                .iter()
                                .map(|r| (*r).to_owned())
                                .collect::<Vec<OwnedRoomId>>()
                        } else {
                            Vec::new()
                        };
                        new_known_rooms.extend(room_ids.iter().map(|r| r.to_owned()));

                        for room_id in &room_ids {
                            let todo_room =
                                todo_rooms
                                    .entry(room_id.to_owned())
                                    .or_insert((BTreeSet::new(), 0, i64::MAX));
                            let limit = list.room_details.timeline_limit.map_or(10, usize::from).min(100);
                            todo_room.0.extend(list.room_details.required_state.iter().cloned());
                            todo_room.1 = todo_room.1.max(limit);
                            // 0 means unknown because it got out of date
                            todo_room.2 = todo_room.2.min(
                                known_rooms
                                    .get(list_id)
                                    .and_then(|k| k.get(room_id))
                                    .copied()
                                    .unwrap_or_default(),
                            );
                        }
                        SyncOp {
                            op: SlidingOp::Sync,
                            range: Some(r.clone()),
                            index: None,
                            room_ids,
                            room_id: None,
                        }
                    })
                    .collect(),
                count: all_joined_rooms.len() as u64,
            },
        );

        if let Some(conn_id) = &body.conn_id {
            crate::sync_v4::update_sync_known_rooms(
                sender_id.clone(),
                authed.device_id().clone(),
                conn_id.clone(),
                list_id.to_string(),
                new_known_rooms,
                global_since_sn,
            );
        }
    }

    let mut known_subscription_rooms = BTreeSet::new();
    for (room_id, room) in &body.room_subscriptions {
        if !crate::room::room_exists(room_id)? {
            continue;
        }

        let todo_room = todo_rooms
            .entry(room_id.clone())
            .or_insert((BTreeSet::new(), 0, i64::MAX));
        let limit = room.timeline_limit.map_or(10, usize::from).min(100);
        todo_room.0.extend(room.required_state.iter().cloned());
        todo_room.1 = todo_room.1.max(limit);
        // 0 means unknown because it got out of date
        todo_room.2 = todo_room.2.min(
            known_rooms
                .get("subscriptions")
                .and_then(|k| k.get(room_id))
                .copied()
                .unwrap_or_default(),
        );
        known_subscription_rooms.insert(room_id.clone());
    }

    for r in &body.unsubscribe_rooms.clone() {
        known_subscription_rooms.remove(&*r);
        body.room_subscriptions.remove(&*r);
    }

    if let Some(conn_id) = &body.conn_id {
        crate::sync_v4::update_sync_known_rooms(
            sender_id.clone(),
            authed.device_id().clone(),
            conn_id.clone(),
            "subscriptions".to_owned(),
            known_subscription_rooms,
            global_since_sn,
        );
    }

    if let Some(conn_id) = &body.conn_id {
        crate::sync_v4::update_sync_subscriptions(
            sender_id.clone(),
            authed.device_id().clone(),
            conn_id.clone(),
            body.room_subscriptions.clone(),
        );
    }

    let mut rooms = BTreeMap::new();
    for (room_id, (required_state_request, timeline_limit, room_since_sn)) in &todo_rooms {
        let mut invite_state = None;

        let mut timestamp: Option<_> = None;
        let mut invite_state = None;
        let (timeline_pdus, limited) = if all_invited_rooms.contains(&&**room_id) {
            invite_state = crate::room::user::invite_state(sender_id, room_id).ok();
            (Vec::new(), true)
        } else {
            crate::sync::load_timeline(sender_id, &room_id, *room_since_sn, *timeline_limit, None)?
        };

        if room_since_sn != &0 && timeline_pdus.is_empty() {
            continue;
        }

        account_data.rooms.insert(
            room_id.to_owned(),
            crate::user::data_changes(Some(room_id), sender_id, *room_since_sn, Some(next_batch))?
                .into_iter()
                .filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
                .collect(),
        );

        let last_private_read_update =
            crate::room::receipt::last_private_read_update_sn(sender_id, room_id)? > *room_since_sn;

        let private_read_event = if last_private_read_update {
            crate::room::receipt::last_private_read(sender_id, room_id).ok()
        } else {
            None
        };

        let mut vector: Vec<RawJson<AnySyncEphemeralRoomEvent>> =
            crate::room::receipt::read_receipts(room_id, *room_since_sn)?
                .into_iter()
                .filter_map(|(read_user, event_sn, value)| {
                    if crate::user::user_is_ignored(&read_user, sender_id) {
                        Some(value)
                    } else {
                        None
                    }
                })
                .collect();

        if let Some(private_read_event) = private_read_event {
            vector.push(private_read_event);
        }

        let receipt_size = vector.len();
        receipts
            .rooms
            .insert(room_id.clone(), pack_receipts(Box::new(vector.into_iter())));

        if account_data.rooms.get(room_id).is_some_and(Vec::is_empty) && receipt_size == 0 {
            continue;
        }

        let prev_batch = timeline_pdus
            .first()
            .map(|(sn, _)| if *sn == 0 { None } else { Some(sn.to_string()) })
            .flatten();

        let room_events: Vec<_> = timeline_pdus.iter().map(|(_, pdu)| pdu.to_sync_room_event()).collect();

        for (_, pdu) in timeline_pdus {
            let ts = pdu.origin_server_ts;
            if DEFAULT_BUMP_TYPES.binary_search(&pdu.event_ty).is_ok() && timestamp.is_none_or(|time| time <= ts) {
                timestamp = Some(ts);
            }
        }

        let required_state = required_state_request
            .iter()
            .map(|state| crate::room::state::get_room_state(&room_id, &state.0, &state.1))
            .into_iter()
            .flatten()
            .filter_map(|o| o)
            .map(|state| state.to_sync_state_event())
            .collect();

        // Heroes
        let heroes = crate::room::state::get_members(&room_id)?
            .into_iter()
            .filter(|user_id| user_id != sender_id)
            .flat_map(|user_id| {
                Ok::<_, AppError>(
                    crate::room::state::get_member(&room_id, &user_id)?.map(|member| SyncRoomHero {
                        user_id: user_id.into(),
                        name: member.display_name,
                        avatar: member.avatar_url,
                    }),
                )
            })
            .flatten()
            .take(5)
            .collect::<Vec<_>>();

        let hero_name = match heroes.len().cmp(&(1_usize)) {
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

        let hero_avatar = if heroes.len() == 1 { heroes[0].1.clone() } else { None };

        rooms.insert(
            room_id.clone(),
            SyncRoom {
                name: crate::room::state::get_name(&room_id, None)?.or_else(|| hero_name),
                avatar: match hero_avatar {
                    Some(hero_avatar) => Some(hero_avatar),
                    _ => crate::room::state::get_avatar_url(room_id).ok().flatten(),
                },
                initial: Some(room_since_sn == &0),
                is_dm: None,
                invite_state,
                unread_notifications: sync_events::UnreadNotificationsCount {
                    highlight_count: Some(
                        crate::room::user::highlight_count(&authed.user_id(), &room_id)?
                            .try_into()
                            .expect("notification count can't go that high"),
                    ),
                    notification_count: Some(
                        crate::room::user::notification_count(&authed.user_id(), &room_id)?
                            .try_into()
                            .expect("notification count can't go that high"),
                    ),
                },
                timeline: room_events,
                required_state,
                prev_batch,
                limited,
                joined_count: Some((crate::room::joined_member_count(&room_id).unwrap_or(0) as u32).into()),
                invited_count: Some((crate::room::invited_member_count(&room_id).unwrap_or(0) as u32).into()),
                num_live: None, // Count events in timeline greater than global sync counter
                timestamp: None,
                heroes: Some(heroes),
            },
        );
    }

    if rooms
        .iter()
        .all(|(_, r)| r.timeline.is_empty() && r.required_state.is_empty())
    {
        // Hang a few seconds so requests are not spammed
        // Stop hanging if new info arrives
        let mut duration = args.timeout.unwrap_or(Duration::from_secs(30));
        if duration.as_secs() > 30 {
            duration = Duration::from_secs(30);
        }
        let _ = tokio::time::timeout(duration, watcher).await;
    }

    json_ok(SyncEventsResBody {
        initial: global_since_sn == 0,
        txn_id: body.txn_id.clone(),
        pos: next_batch.to_string(),
        lists,
        rooms,
        extensions: Extensions {
            to_device: if body.extensions.to_device.enabled.unwrap_or(false) {
                Some(ToDevice {
                    events: crate::user::get_to_device_events(
                        sender_id,
                        authed.device_id(),
                        Some(global_since_sn),
                        Some(next_batch),
                    )?,
                    next_batch: next_batch.to_string(),
                })
            } else {
                None
            },
            e2ee: E2ee {
                device_lists: DeviceLists {
                    changed: device_list_changes.into_iter().collect(),
                    left: device_list_left.into_iter().collect(),
                },
                device_one_time_keys_count: crate::user::count_one_time_keys(sender_id, authed.device_id())?,
                // Fallback keys are not yet supported
                device_unused_fallback_key_types: None,
            },
            account_data: AccountData {
                global: if body.extensions.account_data.enabled.unwrap_or(false) {
                    crate::user::data_changes(None, sender_id, global_since_sn, None)?
                        .into_iter()
                        .filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Global))
                        .collect()
                } else {
                    Vec::new()
                },
                rooms: BTreeMap::new(),
            },
            receipts: Receipts { rooms: BTreeMap::new() },
            typing: Typing { rooms: BTreeMap::new() },
        },
        delta_token: None,
    })
}
