mod account;
mod admin;
mod appservice;
mod auth;
mod device;
mod directory;
mod key;
mod presence;
mod profile;
mod push_rule;
mod pusher;
mod register;
mod room;
mod room_key;
mod session;
mod third_party;
mod to_device;
mod user;
mod user_directory;
mod voip;

pub(crate) mod media;

use std::collections::{hash_map, BTreeMap, BTreeSet, HashSet};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::discovery::{
    Capabilities, CapabilitiesResBody, RoomVersionStability, RoomVersionsCapability, VersionsResBody,
};
use crate::core::client::search::{
    EventContextResult, ResultCategories, ResultRoomEvents, SearchReqArgs, SearchReqBody, SearchResBody, SearchResult,
};
use crate::core::client::sync_events::{
    AccountDataV4, E2eeV4, ExtensionsV4, ReceiptsV4, SlidingOpV4, SyncEventsReqArgsV3, SyncEventsReqArgsV4,
    SyncEventsReqBodyV4, SyncEventsResBodyV3, SyncEventsResBodyV4, SyncListV4, SyncOpV4, ToDeviceV4, TypingV4,
    UnreadNotificationsCount,
};
use crate::core::device::DeviceLists;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::UserId;
use crate::user::NewDbPresence;
use crate::{empty_ok, hoops, json_ok, AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

pub fn router() -> Router {
    let mut client = Router::with_path("client").oapi_tag("client");
    for v in ["v3", "v1", "r0", "unstable"] {
        client = client
            .push(
                Router::with_path(v)
                    .push(account::public_router())
                    .push(profile::public_router())
                    .push(register::public_router())
                    .push(session::public_router())
                    .push(room::public_router())
                    .push(directory::public_router())
                    .push(media::self_auth_router())
                    .push(
                        Router::with_path("publicRooms")
                            .get(room::get_public_rooms)
                            .post(room::get_filtered_public_rooms),
                    ),
            )
            .push(
                Router::with_path(v)
                    .hoop(hoops::auth_by_access_token)
                    .push(account::authed_router())
                    .push(register::authed_router())
                    .push(session::authed_router())
                    .push(device::authed_router())
                    .push(room_key::authed_router())
                    .push(room::authed_router())
                    .push(user::authed_router())
                    .push(directory::authed_router())
                    .push(user_directory::authed_router())
                    .push(key::authed_router())
                    .push(profile::authed_router())
                    .push(voip::authed_router())
                    .push(appservice::authed_router())
                    .push(admin::authed_router())
                    .push(third_party::authed_router())
                    .push(to_device::authed_router())
                    .push(auth::authed_router())
                    .push(pusher::authed_router())
                    .push(push_rule::authed_router())
                    .push(presence::authed_router())
                    .push(Router::with_path("joined_rooms").get(room::membership::joined_rooms))
                    .push(Router::with_path("join/<room_id_or_alias>").post(room::membership::join_room_by_id_or_alias))
                    .push(Router::with_path("createRoom").post(room::create_room))
                    .push(Router::with_path("notifications").get(get_notifications))
                    .push(Router::with_path("sync").get(sync_events_v3).post(sync_events_v4))
                    .push(
                        Router::with_path("dehydrated_device")
                            .get(device::dehydrated)
                            .put(device::upsert_dehydrated)
                            .delete(device::delete_dehydrated)
                            .push(Router::with_path("<device_id>/events").post(to_device::for_dehydrated)),
                    ),
            )
            .push(
                Router::with_path(v)
                    .hoop(hoops::limit_rate)
                    .hoop(hoops::auth_by_access_token)
                    .push(Router::with_path("search").post(search))
                    .push(Router::with_path("capabilities").get(get_capabilities))
                    .push(Router::with_path("knock/<room_id_or_alias>").post(room::membership::knock_room)),
            )
    }
    client.push(Router::with_path("versions").get(supported_versions))
}

// #POST /_matrix/client/r0/search
/// Searches rooms for messages.
///
/// - Only works if the user is currently joined to the room (TODO: Respect history visibility)
#[endpoint]
fn search(
    _aa: AuthArgs,
    args: SearchReqArgs,
    body: JsonBody<SearchReqBody>,
    depot: &mut Depot,
) -> JsonResult<SearchResBody> {
    let authed = depot.authed_info()?;

    let search_criteria = body.search_categories.room_events.as_ref().unwrap();
    let filter = &search_criteria.filter;

    let room_ids = filter
        .rooms
        .clone()
        .unwrap_or_else(|| crate::user::joined_rooms(authed.user_id(), 0).unwrap_or_default());

    // Use limit or else 10, with maximum 100
    let limit = filter.limit.unwrap_or(10).min(100) as usize;

    let mut searches = Vec::new();

    for room_id in room_ids {
        if !crate::room::is_joined(authed.user_id(), &room_id)? {
            return Err(MatrixError::forbidden("You don't have permission to view this room.").into());
        }

        if let Some(s) = crate::room::search_pdus(&room_id, &search_criteria.search_term)? {
            searches.push(s.0.into_iter().peekable());
        }
    }

    let skip = match args.next_batch.as_ref().map(|s| s.parse()) {
        Some(Ok(s)) => s,
        Some(Err(_)) => return Err(MatrixError::invalid_param("Invalid next_batch token.").into()),
        None => 0, // Default to the start
    };

    let mut results = Vec::new();
    for _ in 0..skip + limit {
        if let Some(s) = searches
            .iter_mut()
            .map(|s| (s.peek().cloned(), s))
            .max_by_key(|(peek, _)| peek.clone())
            .and_then(|(_, i)| i.next())
        {
            results.push(s);
        }
    }

    let results: Vec<_> = results
        .iter()
        .filter_map(|result| {
            crate::room::timeline::get_pdu(result)
                .ok()?
                .filter(|pdu| {
                    crate::room::state::user_can_see_event(authed.user_id(), &pdu.room_id, &pdu.event_id)
                        .unwrap_or(false)
                })
                .map(|pdu| pdu.to_room_event())
        })
        .map(|result| SearchResult {
            context: EventContextResult {
                end: None,
                events_after: Vec::new(),
                events_before: Vec::new(),
                profile_info: BTreeMap::new(),
                start: None,
            },
            rank: None,
            result: Some(result),
        })
        .skip(skip)
        .take(limit)
        .collect();

    let next_batch = if results.len() < limit {
        None
    } else {
        Some((skip + limit).to_string())
    };

    json_ok(SearchResBody::new(ResultCategories {
        room_events: ResultRoomEvents {
            count: Some((results.len() as u32).into()), // TODO: set this to none. Element shouldn't depend on it
            groups: BTreeMap::new(),                    // TODO
            next_batch,
            results,
            state: BTreeMap::new(), // TODO
            highlights: search_criteria
                .search_term
                .split_terminator(|c: char| !c.is_alphanumeric())
                .map(str::to_lowercase)
                .collect(),
        },
    }))
}

// #GET /_matrix/client/r0/capabilities
/// Get information on the supported feature set and other relevent capabilities of this server.
#[endpoint]
async fn get_capabilities(_aa: AuthArgs) -> JsonResult<CapabilitiesResBody> {
    let mut available = BTreeMap::new();
    for room_version in &*crate::UNSTABLE_ROOM_VERSIONS {
        available.insert(room_version.clone(), RoomVersionStability::Unstable);
    }
    for room_version in &*crate::STABLE_ROOM_VERSIONS {
        available.insert(room_version.clone(), RoomVersionStability::Stable);
    }
    json_ok(CapabilitiesResBody {
        capabilities: Capabilities {
            room_versions: RoomVersionsCapability {
                default: crate::default_room_version(),
                available,
            },
            ..Default::default()
        },
    })
}

// #GET /_matrix/client/versions
/// Get the versions of the specification and unstable features supported by this server.
///
/// - Versions take the form MAJOR.MINOR.PATCH
/// - Only the latest PATCH release will be reported for each MAJOR.MINOR value
/// - Unstable features are namespaced and may include version information in their name
///
/// Note: Unstable features are used while developing new features. Clients should avoid using
/// unstable features in their stable releases
#[endpoint]
async fn supported_versions() -> JsonResult<VersionsResBody> {
    json_ok(VersionsResBody {
        versions: vec![
            "r0.5.0".to_owned(),
            "r0.6.0".to_owned(),
            "v1.1".to_owned(),
            "v1.2".to_owned(),
            "v1.3".to_owned(),
            "v1.4".to_owned(),
            "v1.5".to_owned(),
        ],
        unstable_features: BTreeMap::from_iter([("org.matrix.e2e_cross_signing".to_owned(), true)]),
    })
}

#[endpoint]
async fn get_notifications(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: get_notifications
    let _authed = depot.authed_info()?;
    empty_ok()
}

// #GET /_matrix/client/r0/sync
/// Synchronize the client's state with the latest state on the server.
///
/// - This endpoint takes a `since` parameter which should be the `next_batch` value from a
/// previous request for incremental syncs.
///
/// Calling this endpoint without a `since` parameter returns:
/// - Some of the most recent events of each timeline
/// - Notification counts for each room
/// - Joined and invited member counts, heroes
/// - All state events
///
/// Calling this endpoint with a `since` parameter from a previous `next_batch` returns:
/// For joined rooms:
/// - Some of the most recent events of each timeline that happened after since
/// - If user joined the room after since: All state events (unless lazy loading is activated) and
/// all device list updates in that room
/// - If the user was already in the room: A list of all events that are in the state now, but were
/// not in the state at `since`
/// - If the state we send contains a member event: Joined and invited member counts, heroes
/// - Device list updates that happened after `since`
/// - If there are events in the timeline we send or the user send updated his read mark: Notification counts
/// - EDUs that are active now (read receipts, typing updates, presence)
/// - TODO: Allow multiple sync streams to support Pantalaimon
///
/// For invited rooms:
/// - If the user was invited after `since`: A subset of the state of the room at the point of the invite
///
/// For left rooms:
/// - If the user left after `since`: prev_batch token, empty state (TODO: subset of the state at the point of the leave)
///
/// - Sync is handled in an async task, multiple requests from the same device with the same
/// `since` will be cached
#[endpoint]
async fn sync_events_v3(
    _aa: AuthArgs,
    args: SyncEventsReqArgsV3,
    req: &mut Request,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBodyV3> {
    let authed = depot.authed_info()?.clone();
    let mut rx = match crate::SYNC_RECEIVERS
        .write()
        .unwrap()
        .entry((authed.user_id().clone(), authed.device_id().clone()))
    {
        hash_map::Entry::Vacant(v) => {
            let (tx, rx) = tokio::sync::watch::channel(None);
            v.insert((args.since.clone(), rx.clone()));
            tokio::spawn({
                let user_id = authed.user_id().to_owned();
                let device_id = authed.device_id().to_owned();
                crate::user::ping_presence(&user_id, &args.set_presence)?;
                async move {
                    if let Err(e) = crate::sync::sync_events(user_id, device_id, args, tx).await {
                        tracing::error!(error = ?e, "sync_events error 1");
                    }
                }
            });
            rx
        }
        hash_map::Entry::Occupied(mut o) => {
            if o.get().0 != args.since || args.since.is_none() {
                let (tx, rx) = tokio::sync::watch::channel(None);
                if args.since.is_some() {
                    o.insert((args.since.clone(), rx.clone()));
                }
                tokio::spawn({
                    let user_id = authed.user_id().to_owned();
                    let device_id = authed.device_id().to_owned();
                    crate::user::ping_presence(&user_id, &args.set_presence)?;
                    async move {
                        if let Err(e) = crate::sync::sync_events(user_id, device_id, args, tx).await {
                            tracing::error!(error = ?e, "sync_events error 2");
                        }
                    }
                });
                rx
            } else {
                o.get().1.clone()
            }
        }
    };

    let we_have_to_wait = rx.borrow().is_none();
    if we_have_to_wait {
        if let Err(e) = rx.changed().await {
            error!("Error waiting for sync: {}", e);
        }
    }

    let result = match rx
        .borrow()
        .as_ref()
        .expect("When sync channel changes it's always set to some")
    {
        Ok(response) => json_ok(response.clone()),
        Err(error) => Err(AppError::public(error.to_string())),
    };
    result
}

#[endpoint]
pub async fn sync_events_v4(
    _aa: AuthArgs,
    args: SyncEventsReqArgsV4,
    mut body: JsonBody<SyncEventsReqBodyV4>,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBodyV4> {
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
                    crate::room::state::get_pdu(state_hash, &StateEventType::RoomMember, authed.user_id().as_str())
                        .transpose()
                })
                .transpose()?
                .and_then(|pdu| {
                    serde_json::from_str(pdu.content.get())
                        .map_err(|_| AppError::public("Invalid PDU in database."))
                        .ok()
                });

            let encrypted_room =
                crate::room::state::get_pdu(current_frame_id, &StateEventType::RoomEncryption, "")?.is_some();

            if let Some(since_frame_id) = since_frame_id {
                // Skip if there are only timeline changes
                if since_frame_id == current_frame_id {
                    continue;
                }

                let since_encryption =
                    crate::room::state::get_pdu(since_frame_id, &StateEventType::RoomEncryption, "")?;
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
                                                &authed.user_id(),
                                                &user_id,
                                                &room_id,
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
                                    !crate::bl::sync::share_encrypted_room(&authed.user_id(), user_id, &room_id)
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
                            crate::room::state::get_state(&other_room_id, &StateEventType::RoomEncryption, "", None)
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
            SyncListV4 {
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
                        SyncOpV4 {
                            op: SlidingOpV4::Sync,
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
        let (timeline_pdus, limited) =
            crate::sync::load_timeline(&authed.user_id(), &room_id, *room_since_sn, *timeline_limit, None)?;

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
            .map(|state| crate::room::state::get_state(&room_id, &state.0, &state.1, None))
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
            palpo_core::client::sync_events::SlidingSyncRoomV4 {
                name: crate::room::state::get_name(&room_id, None)?.or_else(|| name),
                avatar: crate::room::state::get_avatar(&room_id)?.map_or(avatar, |a| a.url),
                initial: Some(room_since_sn == &0),
                is_dm: None,
                invite_state: None,
                unread_notifications: UnreadNotificationsCount {
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

    json_ok(SyncEventsResBodyV4 {
        initial: global_since_sn == 0,
        txn_id: body.txn_id.clone(),
        pos: next_batch.to_string(),
        lists,
        rooms,
        extensions: ExtensionsV4 {
            to_device: if body.extensions.to_device.enabled.unwrap_or(false) {
                Some(ToDeviceV4 {
                    events: crate::user::get_to_device_events(authed.user_id(), authed.device_id())?,
                    next_batch: next_batch.to_string(),
                })
            } else {
                None
            },
            e2ee: E2eeV4 {
                device_lists: DeviceLists {
                    changed: device_list_changes.into_iter().collect(),
                    left: device_list_left.into_iter().collect(),
                },
                device_one_time_keys_count: crate::user::count_one_time_keys(authed.user_id(), authed.device_id())?,
                // Fallback keys are not yet supported
                device_unused_fallback_key_types: None,
            },
            account_data: AccountDataV4 {
                global: if body.extensions.account_data.enabled.unwrap_or(false) {
                    crate::user::get_data_changes(None, &authed.user_id(), global_since_sn)?
                        .into_iter()
                        .filter_map(|(_, v)| {
                            serde_json::from_str(v.inner().get())
                                .map_err(|_| AppError::public("Invalid account event in database."))
                                .ok()
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                rooms: BTreeMap::new(),
            },
            receipts: ReceiptsV4 { rooms: BTreeMap::new() },
            typing: TypingV4 { rooms: BTreeMap::new() },
        },
        delta_token: None,
    })
}
