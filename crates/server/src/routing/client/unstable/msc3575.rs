use std::collections::{BTreeMap, BTreeSet, HashSet, hash_map};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UserId;
use crate::core::client::discovery::{
    Capabilities, CapabilitiesResBody, RoomVersionStability, RoomVersionsCapability, VersionsResBody,
};use crate::core::events::RoomAccountDataEventType;
use crate::core::client::search::{ResultCategories, SearchReqArgs, SearchReqBody, SearchResBody};
use crate::core::client::sync_events::{self, v4::*};
use crate::core::device::DeviceLists;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, empty_ok, hoops, json_ok};

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
    // Setup watchers, so if there's no response, we can wait for them
    let watcher = crate::watch(&authed.user_id(), authed.device_id());

    let next_batch = crate::curr_sn()? + 1;

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
        crate::user::update_sync_request_with_cache(authed.user_id().clone(), authed.device_id().clone(), &mut body);

    let all_joined_rooms = crate::user::joined_rooms(&authed.user_id(), 0)?;

    if body.extensions.to_device.enabled.unwrap_or(false) {
        crate::user::remove_to_device_events(authed.user_id(), authed.device_id(), global_since_sn - 1)?;
    }

    let mut left_encrypted_users = HashSet::new(); // Users that have left any encrypted rooms the sender was in
    let mut device_list_changes = HashSet::new();
    let mut device_list_left = HashSet::new();

    if body.extensions.e2ee.enabled.unwrap_or(false) {
        // Look for device list updates of this account
        device_list_changes.extend(crate::user::get_keys_changed_users(
            authed.user_id(),
            global_since_sn,
            None,
        )?);

        for room_id in &all_joined_rooms {
            let current_frame_id = if let Some(s) = crate::room::state::get_room_frame_id(&room_id, None)? {
                s
            } else {
                error!("Room {} has no state", room_id);
                continue;
            };

            let since_frame_id = crate::room::user::get_last_event_frame_id(&room_id, global_since_sn)?;

            let since_sender_member: Option<RoomMemberEventContent> = since_frame_id
                .and_then(|state_hash| {
                    crate::room::state::get_state(state_hash, &StateEventType::RoomMember, authed.user_id().as_str())
                        .transpose()
                })
                .transpose()?
                .and_then(|pdu| {
                    serde_json::from_str(pdu.content.get())
                        .map_err(|_| AppError::public("Invalid PDU in database."))
                        .ok()
                });

            let encrypted_room =
                crate::room::state::get_state(current_frame_id, &StateEventType::RoomEncryption, "")?.is_some();

            if let Some(since_frame_id) = since_frame_id {
                // Skip if there are only timeline changes
                if since_frame_id == current_frame_id {
                    continue;
                }

                let since_encryption =
                    crate::room::state::get_state(since_frame_id, &StateEventType::RoomEncryption, "")?;
                let joined_since_last_sync =
                    crate::room::user::joined_sn(authed.user_id(), room_id)? >= global_since_sn;

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
                                    let user_id = UserId::parse(state_key.clone())
                                        .map_err(|_| AppError::public("Invalid UserId in member PDU."))?;

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
                                                Some(room_id),
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
                                    &authed.user_id() != &user_id
                                })
                                .filter(|user_id| {
                                    // Only send keys if the sender doesn't share an encrypted room with the target already
                                    !crate::bl::sync::share_encrypted_room(&authed.user_id(), user_id, Some(room_id))
                                        .unwrap_or(false)
                                }),
                        );
                    }
                }
            }
            // Look for device list updates in this room
            device_list_changes.extend(crate::room::keys_changed_users(room_id, global_since_sn, None)?.into_iter());
        }
        for user_id in left_encrypted_users {
            let dont_share_encrypted_room =
                crate::room::user::get_shared_rooms(vec![authed.user_id().clone(), user_id.clone()])?
                    .into_iter()
                    .filter_map(|other_room_id| {
                        Some(
                            crate::room::state::get_room_state(&other_room_id, &StateEventType::RoomEncryption, "")
                                .ok()?
                                .is_some(),
                        )
                    })
                    .all(|encrypted| !encrypted);
            // If the user doesn't share an encrypted room with the target anymore, we need to tell
            // them
            if dont_share_encrypted_room {
                device_list_left.insert(user_id);
            }
        }
    }

    let mut lists = BTreeMap::new();
    let mut todo_rooms = BTreeMap::new(); // and required state

    for (list_id, list) in &body.lists {
        if list.filters.as_ref().and_then(|f| f.is_invite).unwrap_or(false) {
            continue;
        }

        let mut new_known_rooms = BTreeSet::new();

        lists.insert(
            list_id.clone(),
            SyncList {
                ops: list
                    .ranges
                    .clone()
                    .into_iter()
                    .map(|mut r| {
                        r.0 = r.0.clamp(0, all_joined_rooms.len() as u64 - 1);
                        r.1 = r.1.clamp(r.0, all_joined_rooms.len() as u64 - 1);
                        let room_ids = all_joined_rooms[(u64::from(r.0) as usize)..=(u64::from(r.1) as usize)].to_vec();
                        new_known_rooms.extend(room_ids.iter().cloned());
                        for room_id in &room_ids {
                            let todo_room = todo_rooms
                                .entry(room_id.clone())
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
            crate::user::update_sync_known_rooms(
                authed.user_id().clone(),
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
        crate::user::update_sync_known_rooms(
            authed.user_id().clone(),
            authed.device_id().clone(),
            conn_id.clone(),
            "subscriptions".to_owned(),
            known_subscription_rooms,
            global_since_sn,
        );
    }

    if let Some(conn_id) = &body.conn_id {
        crate::user::update_sync_subscriptions(
            authed.user_id().clone(),
            authed.device_id().clone(),
            conn_id.clone(),
            body.room_subscriptions.clone(),
        );
    }

    let mut rooms = BTreeMap::new();
    for (room_id, (required_state_request, timeline_limit, room_since_sn)) in &todo_rooms {
        let mut invite_state = None;

        let (timeline_pdus, limited) = if all_invited_rooms.contains(&new_room_id) {
            invite_state = crate::room::state::invite_state(sender_id, room_id).ok();
            (Vec::new(), true)
        } else {
            crate::sync::load_timeline(&authed.user_id(), &room_id, *room_since_sn, *timeline_limit, None)?
        };

        if room_since_sn != &0 && timeline_pdus.is_empty() {
            continue;
        }

        let prev_batch = timeline_pdus
            .first()
            .map(|(sn, _)| if *sn == 0 { None } else { Some(sn.to_string()) })
            .flatten();

        let room_events: Vec<_> = timeline_pdus.iter().map(|(_, pdu)| pdu.to_sync_room_event()).collect();

        let required_state = required_state_request
            .iter()
            .map(|state| crate::room::state::get_room_state(&room_id, &state.0, &state.1))
            .into_iter()
            .flatten()
            .filter_map(|o| o)
            .map(|state| state.to_sync_state_event())
            .collect();

        // Heroes
        let heroes = crate::room::get_joined_users(&room_id, None)?
            .into_iter()
            .filter(|member| &member != &authed.user_id())
            .flat_map(|member| {
                Ok::<_, AppError>(crate::room::state::get_member(&room_id, &member)?.map(|memberevent| {
                    (
                        memberevent.display_name.unwrap_or_else(|| member.to_string()),
                        memberevent.avatar_url,
                    )
                }))
            })
            .flatten()
            .take(5)
            .collect::<Vec<_>>();
        let name = if heroes.len() > 1 {
            let last = heroes[0].0.clone();
            Some(heroes[1..].iter().map(|h| h.0.clone()).collect::<Vec<_>>().join(", ") + " and " + &last)
        } else if heroes.len() == 1 {
            Some(heroes[0].0.clone())
        } else {
            None
        };

        let avatar = if heroes.len() == 1 { heroes[0].1.clone() } else { None };

        rooms.insert(
            room_id.clone(),
            SyncRoom {
                name: crate::room::state::get_name(&room_id, None)?.or_else(|| name),
                avatar: crate::room::state::get_avatar_url(&room_id)?,
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
                        authed.user_id(),
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
                device_one_time_keys_count: crate::user::count_one_time_keys(authed.user_id(), authed.device_id())?,
                // Fallback keys are not yet supported
                device_unused_fallback_key_types: None,
            },
            account_data: AccountData {
                global: if body.extensions.account_data.enabled.unwrap_or(false) {
                    crate::user::get_data_changes(None, &authed.user_id(), global_since_sn)?
                        .into_iter()
                        .filter_map(|(ty, v)| {
                            if ty == RoomAccountDataEventType::Global {
                                serde_json::from_str(v.inner().get())
                                    .map_err(|_| AppError::public("Invalid account event in database."))
                                    .ok()
                            } else {
                                None
                            }
                        })
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
