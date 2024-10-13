use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use diesel::prelude::*;
use tokio::sync::watch::Sender;

use crate::core::client::filter::{FilterDefinition, LazyLoadOptions};
use crate::core::client::sync_events::{
    EphemeralV3, FilterV3, GlobalAccountDataV3, InviteStateV3, InvitedRoomV3, JoinedRoomV3, LeftRoomV3, PresenceV3,
    RoomAccountDataV3, RoomSummaryV3, RoomsV3, StateV3, SyncEventsReqArgsV3, SyncEventsResBodyV3, TimelineV3,
    ToDeviceV3, UnreadNotificationsCount,
};
use crate::core::device::DeviceLists;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::serde::RawJson;
use crate::event::PduEvent;
use crate::room::state::DbRoomStateField;
use crate::schema::*;
use crate::{db, AppError, AppResult};

#[tracing::instrument(skip_all)]
pub async fn sync_events(
    sender_id: OwnedUserId,
    sender_device_id: OwnedDeviceId,
    args: SyncEventsReqArgsV3,
    tx: Sender<Option<AppResult<SyncEventsResBodyV3>>>,
) -> AppResult<()> {
    crate::user::ping_presence(&sender_id, &args.set_presence)?;

    // Setup watchers, so if there's no response, we can wait for them
    let watcher = crate::watch(&sender_id, &sender_device_id);

    let curr_sn = crate::curr_sn()?;
    let since_sn = args.since.as_ref().and_then(|s| s.parse().ok()).unwrap_or_default();
    let next_batch = curr_sn + 1;

    // Load filter
    let filter = match args.filter {
        None => FilterDefinition::default(),
        Some(FilterV3::FilterDefinition(filter)) => filter,
        Some(FilterV3::FilterId(filter_id)) => {
            crate::user::get_filter(&sender_id, filter_id.parse::<i64>().unwrap_or_default())?.unwrap_or_default()
        }
    };

    let (lazy_load_enabled, lazy_load_send_redundant) = match filter.room.state.lazy_load_options {
        LazyLoadOptions::Enabled {
            include_redundant_members: redundant,
        } => (true, redundant),
        _ => (false, false),
    };

    let full_state = args.full_state;

    let mut joined_rooms = BTreeMap::new();
    let mut presence_updates = HashMap::new();
    let mut left_users = HashSet::new(); // Users that have left any encrypted rooms the sender was in
    let mut device_list_updates = HashSet::new();
    let mut device_list_left = HashSet::new();

    // Look for device list updates of this account
    device_list_updates.extend(crate::user::get_keys_changed_users(&sender_id, since_sn, None)?);

    let all_joined_rooms = crate::user::joined_rooms(&sender_id, 0)?;
    for room_id in all_joined_rooms {
        let joined_room = match load_joined_room(
            &sender_id,
            &sender_device_id,
            &room_id,
            since_sn,
            next_batch,
            lazy_load_enabled,
            lazy_load_send_redundant,
            full_state,
            &mut device_list_updates,
            &mut left_users,
        )
        .await
        {
            Ok(joined_room) => joined_room,
            Err(e) => {
                tracing::error!(error = ?e, "load joined room failed");
                continue;
            }
        };
        if !joined_room.is_empty() {
            joined_rooms.insert(room_id.to_owned(), joined_room);
        }

        if crate::allow_local_presence() {
            // Take presence updates from this room
            for (user_id, presence_event) in crate::user::presences_since(&room_id, since_sn)? {
                match presence_updates.entry(user_id) {
                    Entry::Vacant(slot) => {
                        slot.insert(presence_event);
                    }
                    Entry::Occupied(mut slot) => {
                        let curr_event = slot.get_mut();
                        let curr_content = &mut curr_event.content;
                        let new_content = presence_event.content;

                        // Update existing presence event with more info
                        curr_content.presence = new_content.presence;
                        curr_content.status_msg = new_content.status_msg.or(curr_content.status_msg.take());
                        curr_content.last_active_ago = new_content.last_active_ago.or(curr_content.last_active_ago);
                        curr_content.display_name = new_content.display_name.or(curr_content.display_name.take());
                        curr_content.avatar_url = new_content.avatar_url.or(curr_content.avatar_url.take());
                        curr_content.currently_active = new_content.currently_active.or(curr_content.currently_active);
                    }
                }
            }
        }
    }

    let mut left_rooms = BTreeMap::new();
    let all_left_rooms = crate::room::rooms_left(&sender_id)?;

    for room_id in all_left_rooms.keys() {
        let mut left_state_events = Vec::new();

        let left_count = crate::room::get_left_sn(&room_id, &sender_id)?;

        // // Left before last sync
        // if Some(since_sn) >= left_count {
        //     continue;
        // }

        let since_frame_id = crate::room::user::get_last_event_frame_id(&room_id, since_sn)?;

        let since_state_ids = match since_frame_id {
            Some(s) => crate::room::state::get_full_state_ids(s)?,
            None => HashMap::new(),
        };

        let Some(curr_frame_id) = crate::room::state::get_room_frame_id(room_id)? else {
            continue;
        };
        let Some(left_event_id) =
            crate::room::state::get_state_event_id(curr_frame_id, &StateEventType::RoomMember, sender_id.as_str())?
        else {
            error!("Left room but no left state event");
            continue;
        };

        let left_frame_id = match crate::room::state::get_pdu_frame_id(&left_event_id)? {
            Some(s) => s,
            None => {
                error!("Leave event has no state");
                continue;
            }
        };
        if left_frame_id < since_frame_id.unwrap_or_default() || since_frame_id.is_none() {
            continue;
        }

        let mut left_state_ids = crate::room::state::get_full_state_ids(left_frame_id)?;
        let leave_state_key_id = crate::room::state::ensure_field_id(&StateEventType::RoomMember, sender_id.as_str())?;
        left_state_ids.insert(leave_state_key_id, left_event_id);

        for (key, event_id) in left_state_ids {
            if full_state || since_state_ids.get(&key) != Some(&event_id) {
                let DbRoomStateField {
                    event_type, state_key, ..
                } = crate::room::state::get_field(key)?;

                if !lazy_load_enabled
                || event_type != StateEventType::RoomMember
                || full_state
                // TODO: Delete the following line when this is resolved: https://github.com/vector-im/element-web/issues/22565
                || sender_id == state_key
                {
                    let pdu = match crate::room::timeline::get_pdu(&event_id)? {
                        Some(pdu) => pdu,
                        None => {
                            error!("Pdu in state not found: {}", event_id);
                            continue;
                        }
                    };

                    left_state_events.push(pdu.to_sync_state_event());
                }
            }
        }

        left_rooms.insert(
            room_id.to_owned(),
            LeftRoomV3 {
                account_data: RoomAccountDataV3 { events: Vec::new() },
                timeline: TimelineV3 {
                    limited: false,
                    prev_batch: Some(since_sn.to_string()),
                    events: Vec::new(),
                },
                state: StateV3 {
                    events: left_state_events,
                },
            },
        );
    }
   
    let invited_rooms: BTreeMap<_, _> = crate::user::invited_rooms(&sender_id, since_sn)?
        .into_iter()
        .map(|(room_id, invite_state_events)| {
            (
                room_id,
                InvitedRoomV3 {
                    invite_state: InviteStateV3 {
                        events: invite_state_events,
                    },
                },
            )
        })
        .collect();

    for left_room in left_rooms.keys() {
        let left_users = crate::room::get_joined_users(left_room)?;
        for user_id in left_users {
            let dont_share_encrypted_room = crate::room::user::get_shared_rooms(vec![sender_id.clone(), user_id.clone()])?
                .into_iter()
                .filter_map(|other_room_id| {
                    Some(
                        crate::room::state::get_state(&other_room_id, &StateEventType::RoomEncryption, "")
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
    for user_id in left_users {
        let dont_share_encrypted_room = crate::room::user::get_shared_rooms(vec![sender_id.clone(), user_id.clone()])?
            .into_iter()
            .filter_map(|other_room_id| {
                Some(
                    crate::room::state::get_state(&other_room_id, &StateEventType::RoomEncryption, "")
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

    // Remove all to-device events the device received *last time*
    crate::user::remove_to_device_events(&sender_id, &sender_device_id, since_sn - 1)?;

    let response = SyncEventsResBodyV3 {
        next_batch: next_batch.to_string(),
        rooms: RoomsV3 {
            leave: left_rooms,
            join: joined_rooms,
            invite: invited_rooms,
            knock: BTreeMap::new(), // TODO
        },
        presence: PresenceV3 {
            events: presence_updates
                .into_values()
                .map(|v| RawJson::new(&v).expect("PresenceEvent always serializes successfully"))
                .collect(),
        },
        account_data: GlobalAccountDataV3 {
            events: crate::user::get_data_changes(None, &sender_id, since_sn)?
                .into_iter()
                .filter_map(|(_, v)| {
                    serde_json::from_str(v.inner().get())
                        .map_err(|_| AppError::public("Invalid account event in database."))
                        .ok()
                })
                .collect(),
        },
        device_lists: DeviceLists {
            changed: device_list_updates.into_iter().collect(),
            left: device_list_left.into_iter().collect(),
        },
        device_one_time_keys_count: { crate::user::count_one_time_keys(&sender_id, &sender_device_id)? },
        to_device: ToDeviceV3 {
            events: crate::user::get_to_device_events(&sender_id, &sender_device_id)?,
        },
        // Fallback keys are not yet supported
        device_unused_fallback_key_types: None,
    };

    // TODO: Retry the endpoint instead of returning (waiting for #118)
    let r = if !full_state
        && response.rooms.is_empty()
        && response.presence.is_empty()
        && response.account_data.is_empty()
        && response.device_lists.is_empty()
        && response.to_device.is_empty()
    {
        // Hang a few seconds so requests are not spammed
        // Stop hanging if new info arrives
        let mut duration = args.timeout.unwrap_or_default();
        if duration.as_secs() > 30 {
            duration = Duration::from_secs(30);
        }
        let _ = tokio::time::timeout(duration, watcher).await;
        Ok((response, false))
    } else {
        Ok((response, since_sn != next_batch)) // Only cache if we made progress
    };

    if let Ok((_, caching_allowed)) = r {
        if !caching_allowed {
            match crate::SYNC_RECEIVERS
                .write()
                .unwrap()
                .entry((sender_id.clone(), sender_device_id.clone()))
            {
                Entry::Occupied(o) => {
                    // Only remove if the device didn't start a different /sync already
                    if o.get().0 == args.since {
                        o.remove();
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
    }

    let _ = tx.send(Some(r.map(|(r, _)| r)));
    Ok(())
}

#[tracing::instrument(skip_all)]
async fn load_joined_room(
    sender_id: &UserId,
    sender_device_id: &DeviceId,
    room_id: &RoomId,
    since_sn: i64,
    next_batch: i64,
    lazy_load_enabled: bool,
    lazy_load_send_redundant: bool,
    full_state: bool,
    device_list_updates: &mut HashSet<OwnedUserId>,
    left_users: &mut HashSet<OwnedUserId>,
) -> AppResult<JoinedRoomV3> {
    if since_sn > crate::curr_sn()? {
        return Ok(JoinedRoomV3::default());
    }

    let (timeline_pdus, limited) = load_timeline(sender_id, room_id, since_sn, 10)?;

    let send_notification_counts =
        !timeline_pdus.is_empty() || crate::room::user::last_notification_read(sender_id, &room_id)? > since_sn;

    let mut timeline_users = HashSet::new();
    for (_, event) in &timeline_pdus {
        timeline_users.insert(event.sender.as_str().to_owned());
    }

    crate::room::lazy_loading::lazy_load_confirm_delivery(sender_id, &sender_device_id, &room_id, since_sn)?;

    // Database queries:
    let current_frame_id = if let Some(s) = crate::room::state::get_room_frame_id(&room_id)? {
        s
    } else {
        error!("Room {} has no state", room_id);
        return Err(AppError::public("Room has no state"));
    };

    let since_frame_id = crate::room::user::get_last_event_frame_id(&room_id, since_sn)?;
    let (heroes, joined_member_count, invited_member_count, joined_since_last_sync, state_events) = if timeline_pdus
        .is_empty()
        && (since_frame_id == Some(current_frame_id) || since_frame_id.is_none())
    {
        // No state changes
        (Vec::new(), None, None, false, Vec::new())
    } else {
        // Calculates joined_member_count, invited_member_count and heroes
        let calculate_counts = || {
            let joined_member_count = crate::room::joined_member_count(&room_id).unwrap_or(0);
            let invited_member_count = crate::room::invited_member_count(&room_id).unwrap_or(0);

            // Recalculate heroes (first 5 members)
            let mut heroes = Vec::new();

            if joined_member_count + invited_member_count <= 5 {
                // Go through all PDUs and for each member event, check if the user is still joined or
                // invited until we have 5 or we reach the end

                for hero in crate::room::timeline::all_pdus(sender_id, &room_id)?
                    .into_iter() // Ignore all broken pdus
                    .filter(|(_, pdu)| pdu.kind == TimelineEventType::RoomMember)
                    .map(|(_, pdu)| {
                        let content: RoomMemberEventContent = serde_json::from_str(pdu.content.get())
                            .map_err(|_| AppError::public("Invalid member event in database."))?;

                        if let Some(state_key) = &pdu.state_key {
                            let user_id = UserId::parse(state_key.clone())
                                .map_err(|_| AppError::public("Invalid UserId in member PDU."))?;

                            // The membership was and still is invite or join
                            if matches!(content.membership, MembershipState::Join | MembershipState::Invite)
                                && (crate::room::is_joined(&user_id, &room_id)?
                                    || crate::room::is_invited(&user_id, &room_id)?)
                            {
                                Ok::<_, AppError>(Some(state_key.clone()))
                            } else {
                                Ok(None)
                            }
                        } else {
                            Ok(None)
                        }
                    })
                    // Filter out buggy users
                    .filter_map(|u| u.ok())
                    // Filter for possible heroes
                    .flatten()
                {
                    if heroes.contains(&hero) || hero == sender_id.as_str() {
                        continue;
                    }

                    heroes.push(hero);
                }
            }

            Ok::<_, AppError>((Some(joined_member_count), Some(invited_member_count), heroes))
        };

        let joined_since_last_sync = crate::room::user::joined_sn(sender_id, room_id)? >= since_sn;

        if since_sn == 0 || joined_since_last_sync {
            // Probably since = 0, we will do an initial sync
            let (joined_member_count, invited_member_count, heroes) = calculate_counts()?;

            let current_state_ids = crate::room::state::get_full_state_ids(current_frame_id)?;

            let mut state_events = Vec::new();
            let mut lazy_loaded = HashSet::new();

            for (state_key_id, id) in current_state_ids {
                let DbRoomStateField {
                    event_type, state_key, ..
                } = crate::room::state::get_field(state_key_id)?;

                if event_type != StateEventType::RoomMember {
                    let pdu = match crate::room::timeline::get_pdu(&id)? {
                        Some(pdu) => pdu,
                        None => {
                            error!("Pdu in state not found: {}", id);
                            continue;
                        }
                    };
                    state_events.push(pdu);
                } else if !lazy_load_enabled
                    || full_state
                    || timeline_users.contains(&state_key)
                    // TODO: Delete the following line when this is resolved: https://github.com/vector-im/element-web/issues/22565
                    || *sender_id == state_key
                {
                    let pdu = match crate::room::timeline::get_pdu(&id)? {
                        Some(pdu) => pdu,
                        None => {
                            error!("Pdu in state not found: {}", id);
                            continue;
                        }
                    };

                    // This check is in case a bad user ID made it into the database
                    if let Ok(uid) = UserId::parse(&state_key) {
                        lazy_loaded.insert(uid);
                    }
                    state_events.push(pdu);
                }
            }

            // Reset lazy loading because this is an initial sync
            crate::room::lazy_loading::lazy_load_reset(sender_id, sender_device_id, &room_id)?;

            // The state_events above should contain all timeline_users, let's mark them as lazy
            // loaded.
            crate::room::lazy_loading::lazy_load_mark_sent(
                sender_id,
                sender_device_id,
                &room_id,
                lazy_loaded,
                next_batch,
            );

            if joined_since_last_sync {// && encrypted_room || new_encrypted_room {
                // If the user is in a new encrypted room, give them all joined users
                device_list_updates.extend(
                    crate::room::get_joined_users(&room_id)?
                        .into_iter()
                        .filter(|user_id| {
                            // Don't send key updates from the sender to the sender
                            sender_id != user_id
                        })
                        // .filter(|user_id| {
                            // Only send keys if the sender doesn't share an encrypted room with the target already
                            // !share_encrypted_room(sender_id, user_id, &room_id).unwrap_or(false)
                        // }),
                );
            }
            (heroes, joined_member_count, invited_member_count, true, state_events)
        } else if let Some(since_frame_id) = since_frame_id {
            // Incremental /sync
            let mut state_events = Vec::new();
            let mut lazy_loaded = HashSet::new();

            if since_frame_id != current_frame_id {
                let current_state_ids = crate::room::state::get_full_state_ids(current_frame_id)?;
                let since_state_ids = crate::room::state::get_full_state_ids(since_frame_id)?;

                for (key, id) in current_state_ids {
                    if full_state || since_state_ids.get(&key) != Some(&id) {
                        let pdu = match crate::room::timeline::get_pdu(&id)? {
                            Some(pdu) => pdu,
                            None => {
                                error!("Pdu in state not found: {}", id);
                                continue;
                            }
                        };

                        if pdu.kind == TimelineEventType::RoomMember {
                            match UserId::parse(pdu.state_key.as_ref().expect("State event has state key").clone()) {
                                Ok(state_key_user_id) => {
                                    lazy_loaded.insert(state_key_user_id);
                                }
                                Err(e) => error!("Invalid state key for member event: {}", e),
                            }
                        }

                        state_events.push(pdu);
                    }
                }
            }

            for (_, event) in &timeline_pdus {
                if lazy_loaded.contains(&event.sender) {
                    continue;
                }

                if !crate::room::lazy_loading::lazy_load_was_sent_before(
                    sender_id,
                    sender_device_id,
                    &room_id,
                    &event.sender,
                )? || lazy_load_send_redundant
                {
                    if let Some(member_event) =
                        crate::room::state::get_state(&room_id, &StateEventType::RoomMember, event.sender.as_str())?
                    {
                        lazy_loaded.insert(event.sender.clone());
                        state_events.push(member_event);
                    }
                }
            }

            crate::room::lazy_loading::lazy_load_mark_sent(
                sender_id,
                sender_device_id,
                &room_id,
                lazy_loaded,
                next_batch,
            );

            let encrypted_room =
                crate::room::state::get_pdu(current_frame_id, &StateEventType::RoomEncryption, "")?.is_some();

            let since_encryption = crate::room::state::get_pdu(since_frame_id, &StateEventType::RoomEncryption, "")?;

            // Calculations:
            let new_encrypted_room = encrypted_room && since_encryption.is_none();

            let send_member_count = state_events
                .iter()
                .any(|event| event.kind == TimelineEventType::RoomMember);

            // if encrypted_room {
            for state_event in &state_events {
                if state_event.kind != TimelineEventType::RoomMember {
                    continue;
                }

                if let Some(state_key) = &state_event.state_key {
                    let user_id = UserId::parse(state_key.clone())
                        .map_err(|_| AppError::public("Invalid UserId in member PDU."))?;

                    if user_id == sender_id {
                        continue;
                    }

                    let new_membership = serde_json::from_str::<RoomMemberEventContent>(state_event.content.get())
                        .map_err(|_| AppError::public("Invalid PDU in database."))?
                        .membership;

                    match new_membership {
                        MembershipState::Join => {
                            // A new user joined an encrypted room
                            // if !share_encrypted_room(sender_id, &user_id, &room_id)? {
                            if !crate::room::user::get_shared_rooms(vec![sender_id.to_owned(), user_id.to_owned()])?.is_empty(){
                                device_list_updates.insert(user_id);
                            }
                        }
                        MembershipState::Leave => {
                            // Write down users that have left encrypted rooms we are in
                            left_users.insert(user_id);
                        }
                        _ => {}
                    }
                }
            }
            // }

            if joined_since_last_sync {// && encrypted_room || new_encrypted_room {
                // If the user is in a new encrypted room, give them all joined users
                device_list_updates.extend(
                    crate::room::get_joined_users(&room_id)?
                        .into_iter()
                        .filter(|user_id| {
                            // Don't send key updates from the sender to the sender
                            sender_id != user_id
                        })
                        // .filter(|user_id| {
                            // Only send keys if the sender doesn't share an encrypted room with the target already
                            // !share_encrypted_room(sender_id, user_id, &room_id).unwrap_or(false)
                        // }),
                );
            }

            let (joined_member_count, invited_member_count, heroes) = if send_member_count {
                calculate_counts()?
            } else {
                (None, None, Vec::new())
            };

            (
                heroes,
                joined_member_count,
                invited_member_count,
                joined_since_last_sync,
                state_events,
            )
        } else {
            (Vec::new(), None, None, false, Vec::new())
        }
    };

    // Look for device list updates in this room
    device_list_updates.extend(crate::room::keys_changed_users(room_id, since_sn, None)?);

    let notification_count = if send_notification_counts {
        Some(
            crate::room::user::notification_count(sender_id, &room_id)?
                .try_into()
                .expect("notification count can't go that high"),
        )
    } else {
        None
    };

    let highlight_count = if send_notification_counts {
        Some(
            crate::room::user::highlight_count(sender_id, &room_id)?
                .try_into()
                .expect("highlight count can't go that high"),
        )
    } else {
        None
    };

    let prev_batch = timeline_pdus.first().map(|(sn, _)| sn.to_string());

    let room_events: Vec<_> = timeline_pdus.iter().map(|(_, pdu)| pdu.to_sync_room_event()).collect();

    let mut edus: Vec<_> = crate::room::receipt::read_receipts(&room_id, since_sn)?
        .into_iter() // Filter out buggy events
        .map(|(_, _, v)| v)
        .collect();

    if crate::room::typing::last_typing_update(&room_id).await? >= since_sn {
        edus.push(
            serde_json::from_str(&serde_json::to_string(
                &crate::room::typing::all_typings(&room_id).await?,
            )?)
            .expect("event is valid, we just created it"),
        );
    }

    let account_events = crate::user::get_data_changes(Some(&room_id), sender_id, since_sn)?
        .into_iter()
        .filter_map(|(_, v)| match serde_json::from_str(v.inner().get()) {
            Ok(event) => Some(event),
            Err(e) => {
                tracing::error!(error = ?e, "Invalid account event in database.");
                None
            }
        })
        .collect();
    Ok(JoinedRoomV3 {
        account_data: RoomAccountDataV3 { events: account_events },
        summary: RoomSummaryV3 {
            heroes,
            joined_member_count: joined_member_count.map(|n| (n as u32).into()),
            invited_member_count: invited_member_count.map(|n| (n as u32).into()),
        },
        unread_notifications: UnreadNotificationsCount {
            highlight_count,
            notification_count,
        },
        timeline: TimelineV3 {
            limited: limited || joined_since_last_sync,
            prev_batch,
            events: room_events,
        },
        state: StateV3 {
            events: state_events.iter().map(|pdu| pdu.to_sync_state_event()).collect(),
        },
        ephemeral: EphemeralV3 { events: edus },
        unread_thread_notifications: BTreeMap::new(),
        unread_count: None,
    })
}

#[tracing::instrument]
pub(crate) fn load_timeline(
    user_id: &UserId,
    room_id: &RoomId,
    occur_sn: i64,
    limit: usize,
) -> AppResult<(Vec<(i64, PduEvent)>, bool)> {
    let mut timeline_pdus = crate::room::timeline::get_pdus_forward(user_id, &room_id, occur_sn, limit + 1, None)?;

    if timeline_pdus.len() > limit {
        timeline_pdus.pop();
        Ok((timeline_pdus, true))
    } else {
        Ok((timeline_pdus, false))
    }
}

#[tracing::instrument]
pub(crate) fn share_encrypted_room(sender_id: &UserId, user_id: &UserId, ignore_room: &RoomId) -> AppResult<bool> {
    let shared_rooms = crate::room::user::get_shared_rooms(vec![sender_id.to_owned(), user_id.to_owned()])?
        .into_iter()
        .filter(|room_id| room_id != ignore_room)
        .filter_map(|other_room_id| {
            Some(
                crate::room::state::get_state(&other_room_id, &StateEventType::RoomEncryption, "")
                    .ok()?
                    .is_some(),
            )
        })
        .any(|encrypted| encrypted);

    Ok(shared_rooms)
}