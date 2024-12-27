mod fetch_state;
mod state_at_incoming;
use fetch_state::fetch_state;
use state_at_incoming::{state_at_incoming_degree_one, state_at_incoming_resolved};

use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use diesel::prelude::*;
use futures_util::{stream::FuturesUnordered, StreamExt};
use palpo_core::federation::event::EventResBody;
use tokio::sync::{RwLock, RwLockWriteGuard, Semaphore};

use crate::core::directory::QueryCriteria;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::server_acl::RoomServerAclEventContent;
use crate::core::events::StateEventType;
use crate::core::federation::directory::{
    remote_server_keys_batch_request, remote_server_keys_request, RemoteServerKeysBatchReqBody,
    RemoteServerKeysBatchResBody, RemoteServerKeysReqArgs, ServerKeysResBody,
};
use crate::core::federation::event::{
    get_events_request, room_state_ids_request, RoomStateAtEventReqArgs, RoomStateIdsResBody,
};
use crate::core::federation::key::get_server_key_request;
use crate::core::federation::membership::{SendJoinResBodyV1, SendJoinResBodyV2};
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, RawJsonValue};
use crate::core::state::{self, RoomVersion, StateMap};
use crate::core::{OwnedServerName, ServerName, UnixMillis};
use crate::event::{DbEvent, DbEventData, NewDbEvent, PduEvent};
use crate::room::state::DeltaInfo;
use crate::room::state::{CompressedState, DbRoomStateField, FrameInfo};
use crate::{db, exts::*, schema::*, AppError, AppResult, MatrixError, SigningKeys};

/// When receiving an event one needs to:
/// 0. Check the server is in the room
/// 1. Skip the PDU if we already know about it
/// 1.1. Remove unsigned field
/// 2. Check signatures, otherwise drop
/// 3. Check content hash, redact if doesn't match
/// 4. Fetch any missing auth events doing all checks listed here starting at 1. These are not
///    timeline events
/// 5. Reject "due to auth events" if can't get all the auth events or some of the auth events are
///    also rejected "due to auth events"
/// 6. Reject "due to auth events" if the event doesn't pass auth based on the auth events
/// 7. Persist this event as an outlier
/// 8. If not timeline event: stop
/// 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline
///    events
/// 10. Fetch missing state and auth chain events by calling /state_ids at backwards extremities
///     doing all the checks in this list starting at 1. These are not timeline events
/// 11. Check the auth of the event passes based on the state of the event
/// 12. Ensure that the state is derived from the previous current state (i.e. we calculated by
///     doing state res where one of the inputs was a previously trusted set of state, don't just
///     trust a set of state we got from a remote)
/// 13. Use state resolution to find new room state
/// 14. Check if the event passes auth based on the "current state" of the room, if not soft fail it
#[tracing::instrument(skip(value, is_timeline_event))]
pub(crate) async fn handle_incoming_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    value: BTreeMap<String, CanonicalJsonValue>,
    is_timeline_event: bool,
    // pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    // 0. Check the server is in the room
    if !crate::room::room_exists(room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server").into());
    }

    if crate::room::is_disabled(room_id)? {
        return Err(MatrixError::forbidden("Federation of this room is currently disabled on this server.").into());
    }

    crate::event::handler::acl_check(origin, &room_id)?;

    // 1. Skip the PDU if we already have it as a timeline event
    if let Some(pdu_id) = crate::room::state::get_pdu_frame_id(event_id)? {
        return Ok(());
    }

    // let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
    //     .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;

    let room_version_id = &crate::room::room_version(room_id)?;

    let (incoming_pdu, val) = handle_outlier_pdu(origin, event_id, room_id, room_version_id, value, false).await?;

    check_room_id(room_id, &incoming_pdu)?;

    // 8. if not timeline event: stop
    if !is_timeline_event {
        return Ok(());
    }

    // // Skip old events
    // if incoming_pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
    //     return Ok(());
    // }
    // 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
    let (sorted_prev_events, mut eventid_info) =
        fetch_missing_prev_events(origin, room_id, room_version_id, incoming_pdu.prev_events.clone()).await?;

    println!("HHHHHHHHHHHHHHHHH 3");
    let mut errors = 0;
    debug!(events = ?sorted_prev_events, "Got previous events");
    for prev_id in sorted_prev_events {
        // Check for disabled again because it might have changed
        if crate::room::is_disabled(room_id)? {
            return Err(MatrixError::forbidden("Federation of this room is currently disabled on this server.").into());
        }

        if let Some((time, tries)) = crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&*prev_id) {
            // Exponential backoff
            let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
            if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
            }

            if time.elapsed() < min_elapsed_duration {
                info!("Backing off from {}", prev_id);
                continue;
            }
        }

        if errors >= 5 {
            break;
        }

        if let Some((pdu, json)) = eventid_info.remove(&*prev_id) {
            // // Skip old events
            // if pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
            //     continue;
            // }

            let start_time = Instant::now();
            crate::ROOM_ID_FEDERATION_HANDLE_TIME
                .write()
                .unwrap()
                .insert(room_id.to_owned(), ((*prev_id).to_owned(), start_time));

            if let Err(e) = upgrade_outlier_to_timeline_pdu(&pdu, json, origin, room_id).await {
                errors += 1;
                warn!("Prev event {} failed: {}", prev_id, e);
                match crate::BAD_EVENT_RATE_LIMITER
                    .write()
                    .unwrap()
                    .entry((*prev_id).to_owned())
                {
                    hash_map::Entry::Vacant(e) => {
                        e.insert((Instant::now(), 1));
                    }
                    hash_map::Entry::Occupied(mut e) => *e.get_mut() = (Instant::now(), e.get().1 + 1),
                }
            }
            let elapsed = start_time.elapsed();
            crate::ROOM_ID_FEDERATION_HANDLE_TIME
                .write()
                .unwrap()
                .remove(&room_id.to_owned());
            debug!(
                "Handling prev event {} took {}m{}s",
                prev_id,
                elapsed.as_secs() / 60,
                elapsed.as_secs() % 60
            );
        }
    }

    // Done with prev events, now handling the incoming event

    let start_time = Instant::now();
    crate::ROOM_ID_FEDERATION_HANDLE_TIME
        .write()
        .unwrap()
        .insert(room_id.to_owned(), (event_id.to_owned(), start_time));
    crate::event::handler::upgrade_outlier_to_timeline_pdu(&incoming_pdu, val, origin, room_id).await?;
    crate::ROOM_ID_FEDERATION_HANDLE_TIME
        .write()
        .unwrap()
        .remove(&room_id.to_owned());
    Ok(())
}

#[tracing::instrument(skip_all)]
fn handle_outlier_pdu<'a>(
    origin: &'a ServerName,
    event_id: &'a EventId,
    room_id: &'a RoomId,
    room_version_id: &'a RoomVersionId,
    mut value: BTreeMap<String, CanonicalJsonValue>,
    auth_events_known: bool,
) -> Pin<Box<impl Future<Output = AppResult<(PduEvent, BTreeMap<String, CanonicalJsonValue>)>> + 'a + Send>> {
    Box::pin(async move {
        // 1.1. Remove unsigned field
        value.remove("unsigned");

        let room_version = RoomVersion::new(room_version_id).expect("room version is supported");

        let origin_server_ts = value.get("origin_server_ts").ok_or_else(|| {
            error!("Invalid PDU, no origin_server_ts field");
            MatrixError::missing_param("Invalid PDU, no origin_server_ts field")
        })?;

        let origin_server_ts: UnixMillis = {
            let ts = origin_server_ts
                .as_integer()
                .ok_or_else(|| MatrixError::invalid_param("origin_server_ts must be an integer"))?;

            UnixMillis(
                ts.try_into()
                    .map_err(|_| MatrixError::invalid_param("Time must be after the unix epoch"))?,
            )
        };

        let mut val = match crate::server_key::verify_event(&value, Some(room_version_id)).await {
            Ok(crate::core::signatures::Verified::Signatures) => {
                // Redact
                warn!("Calculated hash does not match: {}", event_id);
                let obj = match crate::core::canonical_json::redact(value, room_version_id, None) {
                    Ok(obj) => obj,
                    Err(_) => return Err(MatrixError::invalid_param("Redaction failed").into()),
                };

                // Skip the PDU if it is redacted and we already have it as an outlier event
                if crate::room::timeline::get_pdu_json(event_id)?.is_some() {
                    return Err(MatrixError::invalid_param("Event was redacted and we already knew about it").into());
                }

                obj
            }
            Ok(crate::core::signatures::Verified::All) => value,
            Err(e) => {
                // Drop
                warn!("Dropping bad event {}: {}", event_id, e,);
                return Err(MatrixError::invalid_param("Signature verification failed").into());
            }
        };
        println!("MMMMMMMMMMMMMM===1");

        // Now that we have checked the signature and hashes we can add the eventID and convert
        // to our PduEvent type
        val.insert(
            "event_id".to_owned(),
            CanonicalJsonValue::String(event_id.as_str().to_owned()),
        );
        let incoming_pdu = serde_json::from_value::<PduEvent>(
            serde_json::to_value(&val).expect("CanonicalJsonObj is a valid JsonValue"),
        )
        .map_err(|_| AppError::internal("Event is not a valid PDU."))?;

        println!("MMMMMMMMMMMMMM===2");
        check_room_id(room_id, &incoming_pdu)?;

        if !auth_events_known {
            // 4. fetch any missing auth events doing all checks listed here starting at 1. These are not timeline events
            // 5. Reject "due to auth events" if can't get all the auth events or some of the auth events are also rejected "due to auth events"
            // NOTE: Step 5 is not applied anymore because it failed too often
            debug!(event_id = ?incoming_pdu.event_id, "Fetching auth events");
            println!("MMMMMMMMMMMMMM===3");
            fetch_and_handle_outliers(
                origin,
                &incoming_pdu
                    .auth_events
                    .iter()
                    .map(|x| Arc::from(&**x))
                    .collect::<Vec<_>>(),
                room_id,
                room_version_id,
            )
            .await?;
        }

        // 6. Reject "due to auth events" if the event doesn't pass auth based on the auth events
        debug!("Auth check for {} based on auth events", incoming_pdu.event_id);

        // Build map of auth events
        let mut auth_events = HashMap::new();
        println!("MMMMMMMMMMMMMM=??==3");
        for id in &incoming_pdu.auth_events {
            let auth_event = match crate::room::timeline::get_pdu(id)? {
                Some(e) => e,
                None => {
                    warn!("Could not find auth event {}", id);
                    continue;
                }
            };

            check_room_id(room_id, &auth_event)?;

            match auth_events.entry((
                auth_event.event_ty.to_string().into(),
                auth_event.state_key.clone().expect("all auth events have state keys"),
            )) {
                hash_map::Entry::Vacant(v) => {
                    v.insert(auth_event);
                }
                hash_map::Entry::Occupied(_) => {
                    return Err(MatrixError::invalid_param(
                        "Auth event's type and state_key combination exists multiple times.",
                    )
                    .into());
                }
            }
        }

        // The original create event must be in the auth events
        if !matches!(
            auth_events.get(&(StateEventType::RoomCreate, "".to_owned())),
            Some(_) | None
        ) {
            return Err(MatrixError::invalid_param("Incoming event refers to wrong create event.").into());
        }

        if !state::event_auth::auth_check(
            &room_version,
            &incoming_pdu,
            None::<PduEvent>, // TODO: third party invite
            |k, s| auth_events.get(&(k.to_string().into(), s.to_owned())),
        )
        .map_err(|_e| MatrixError::invalid_param("Auth check failed outllier pdu"))?
        {
            return Err(MatrixError::invalid_param("Auth check failed outllier pdu").into());
        }

        debug!("Validation successful.");

        // 7. Persist the event as an outlier.
        diesel::insert_into(events::table)
            .values(NewDbEvent::from_canonical_json(&incoming_pdu.event_id, &val)?)
            .on_conflict_do_nothing()
            .execute(&mut *db::connect()?)?;

        debug!("Added pdu as outlier.");

        Ok((incoming_pdu, val))
    })
}

#[tracing::instrument(skip(incoming_pdu, val))]
pub async fn upgrade_outlier_to_timeline_pdu(
    incoming_pdu: &PduEvent,
    val: BTreeMap<String, CanonicalJsonValue>,
    origin: &ServerName,
    room_id: &RoomId,
) -> AppResult<()> {
    println!("xxUUUUUUUUUUUUUU 0");
    let conf = crate::config();
    if crate::room::pdu_metadata::is_event_soft_failed(&incoming_pdu.event_id)? {
        return Err(MatrixError::invalid_param("Event has been soft failed").into());
    }
    println!("xxUUUUUUUUUUUUUU 1");

    info!("Upgrading {} to timeline pdu", incoming_pdu.event_id);
    let room_version_id = &crate::room::room_version(room_id)?;
    println!("xxUUUUUUUUUUUUUU 2");
    let room_version = RoomVersion::new(&room_version_id).expect("room version is supported");

    // 10. Fetch missing state and auth chain events by calling /state_ids at backwards extremities
    //     doing all the checks in this list starting at 1. These are not timeline events.

    // TODO: if we know the prev_events of the incoming event we can avoid the request and build
    // the state from a known point and resolve if > 1 prev_event

    debug!("Requesting state at event");
    println!("UUUUUUUUUUUUUU 0");
    let mut state_at_incoming_event = if incoming_pdu.prev_events.len() == 1 {
        state_at_incoming_degree_one(incoming_pdu).await?
    } else {
        state_at_incoming_resolved(incoming_pdu, room_id, room_version_id).await?
    };

    println!("UUUUUUUUUUUUUU 1");
    let event_data = DbEventData {
        event_id: (&*incoming_pdu.event_id).to_owned(),
        event_sn: incoming_pdu.event_sn,
        room_id: incoming_pdu.room_id.to_owned(),
        internal_metadata: None,
        json_data: serde_json::to_value(&val)?,
        format_version: None,
    };
    diesel::insert_into(event_datas::table)
        .values(&event_data)
        .on_conflict((event_datas::event_id, event_datas::event_sn))
        .do_update()
        .set(&event_data)
        .execute(&mut db::connect()?)?;

    let event = DbEvent {
        id: (&*incoming_pdu.event_id).to_owned(),
        sn: incoming_pdu.event_sn,
        ty: incoming_pdu.event_ty.to_string(),
        room_id: incoming_pdu.room_id.to_owned(),
        unrecognized_keys: None,
        depth: incoming_pdu.depth as i64,
        origin_server_ts: Some(UnixMillis(incoming_pdu.origin_server_ts.0)),
        received_at: None,
        sender_id: None,
        contains_url: event_data.json_data.get("url").is_some(),
        worker_id: None,
        state_key: incoming_pdu.state_key.clone(),
        processed: false,
        outlier: false,
        soft_failed: false,
        rejection_reason: None,
    };
    diesel::insert_into(events::table)
        .values(&event)
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)?;
    println!("UUUUUUUUUUUUUU 2");

    let state_at_incoming_event = match state_at_incoming_event {
        None => fetch_state(origin, room_id, &room_version_id, &incoming_pdu.event_id)
            .await?
            .unwrap_or_default(),
        Some(state) => state,
    };
    println!("UUUUUUUUUUUUUU 3");

    debug!("Starting auth check");
    // 11. Check the auth of the event passes based on the state of the event
    let check_result = state::event_auth::auth_check(
        &room_version,
        &incoming_pdu,
        None::<PduEvent>, // TODO: third party invite
        |k, s| {
            crate::room::state::ensure_field_id(&k.to_string().into(), s)
                .ok()
                .and_then(|state_key_id| state_at_incoming_event.get(&state_key_id))
                .and_then(|event_id| crate::room::timeline::get_pdu(event_id).ok().flatten())
        },
    )
    .map_err(|_e| MatrixError::invalid_param("Auth check failed for event passes based on the state"))?;

    println!("UUUUUUUUUUUUUU 8");
    if !check_result {
        return Err(AppError::internal(
            "Event has failed auth check with state at the event.",
        ));
    }
    println!("UUUUUUUUUUUUUU 9");
    debug!("Auth check succeeded");

    // Soft fail check before doing state res
    let auth_events = crate::room::state::get_auth_events(
        room_id,
        &incoming_pdu.event_ty,
        &incoming_pdu.sender,
        incoming_pdu.state_key.as_deref(),
        &incoming_pdu.content,
    )?;

    println!("UUUUUUUUUUUUUU 10");
    let soft_fail = !state::event_auth::auth_check(&room_version, &incoming_pdu, None::<PduEvent>, |k, s| {
        auth_events.get(&(k.clone(), s.to_owned()))
    })
    .map_err(|_e| MatrixError::invalid_param("Auth check failed before doing state"))?;

    println!("UUUUUUUUUUUUUU 11");
    // 13. Use state resolution to find new room state

    // We start looking at current room state now, so lets lock the room
    // Now we calculate the set of extremities this room has after the incoming event has been
    // applied. We start with the previous extremities (aka leaves)
    debug!("Calculating extremities");
    let mut extremities = crate::room::state::get_forward_extremities(room_id)?;

    println!("UUUUUUUUUUUUUU 102");
    // Remove any forward extremities that are referenced by this incoming event's prev_events
    for prev_event in &incoming_pdu.prev_events {
        if extremities.contains(prev_event) {
            extremities.remove(prev_event);
        }
    }

    println!("UUUUUUUUUUUUUU 13");
    // // Only keep those extremities were not referenced yet
    // extremities.retain(|id| {
    //     !matches!(
    //         crate::room::pdu_metadata::is_event_referenced(room_id, id),
    //         Ok(true)
    //     )
    // });

    debug!("Compressing state at event");
    let compressed_state_ids = Arc::new(
        state_at_incoming_event
            .iter()
            .map(|(field_id, event_id)| {
                crate::room::state::compress_event(room_id, *field_id, event_id, crate::event::get_event_sn(event_id)?)
            })
            .collect::<AppResult<_>>()?,
    );
    println!("UUUUUUUUUUUUUU 14");

    if incoming_pdu.state_key.is_some() {
        debug!("Preparing for stateres to derive new room state");

        // We also add state after incoming event to the fork states
        println!("UUUUUUUUUUUUUU 14---0");
        let mut state_after = state_at_incoming_event.clone();
        if let Some(state_key) = &incoming_pdu.state_key {
            let state_key_id =
                crate::room::state::ensure_field_id(&incoming_pdu.event_ty.to_string().into(), state_key)?;

            state_after.insert(state_key_id, Arc::from(&*incoming_pdu.event_id));
        }

        println!("UUUUUUUUUUUUUU 14---1");
        let new_room_state = resolve_state(room_id, room_version_id, state_after)?;

        println!("UUUUUUUUUUUUUU 14---2");
        // Set the new room state to the resolved state
        debug!("Forcing new room state");

        let DeltaInfo {
            frame_id,
            appended,
            disposed,
        } = crate::room::state::save_state(room_id, new_room_state)?;

        println!("UUUUUUUUUUUUUU 14---3");
        crate::room::state::force_state(room_id, frame_id, appended, disposed)?;
        println!("UUUUUUUUUUUUUU 14---4");
    }

    println!("UUUUUUUUUUUUUU 15");
    // 14. Check if the event passes auth based on the "current state" of the room, if not soft fail it
    debug!("Starting soft fail auth check");

    if soft_fail {
        crate::room::timeline::append_incoming_pdu(
            &incoming_pdu,
            val,
            extremities.iter().map(|e| (**e).to_owned()).collect(),
            compressed_state_ids,
            soft_fail,
        )?;

        // Soft fail, we keep the event as an outlier but don't add it to the timeline
        warn!("Event was soft failed: {:?}", incoming_pdu);
        crate::room::pdu_metadata::mark_event_soft_failed(&incoming_pdu.event_id)?;
        return Err(MatrixError::invalid_param("Event has been soft failed").into());
    }

    println!("UUUUUUUUUUUUUU 16");
    debug!("Appending pdu to timeline");
    extremities.insert(incoming_pdu.event_id.clone());

    // Now that the event has passed all auth it is added into the timeline.
    // We use the `state_at_event` instead of `state_after` so we accurately
    // represent the state for this event.

    let pdu_id = crate::room::timeline::append_incoming_pdu(
        &incoming_pdu,
        val,
        extremities.iter().map(|e| (**e).to_owned()).collect(),
        compressed_state_ids,
        soft_fail,
    )?;

    println!("UUUUUUUUUUUUUU 17");
    debug!("Appended incoming pdu");

    // Event has passed all auth/stateres checks
    // drop(state_lock);
    Ok(())
}

fn resolve_state(
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    incoming_state: HashMap<i64, Arc<EventId>>,
) -> AppResult<Arc<HashSet<CompressedState>>> {
    debug!("Loading current room state ids");
    println!("VVresolve_state  0");
    let current_frame_id = crate::room::state::get_room_frame_id(room_id, None)?;

    println!("VVresolve_state  1");
    let current_state_ids = if let Some(current_frame_id) = current_frame_id {
        crate::room::state::get_full_state_ids(current_frame_id)?
    } else {
        HashMap::new()
    };

    println!("VVresolve_state  2");
    let fork_states = [current_state_ids, incoming_state];

    let mut auth_chain_sets = Vec::new();
    debug!("Loading fork states");
    for state in &fork_states {
        for event_id in state.values() {
            auth_chain_sets.push(crate::room::auth_chain::get_auth_chain(room_id, event_id)?);
        }
    }

    println!("VVresolve_state  3");
    let fork_states: Vec<_> = fork_states
        .into_iter()
        .map(|map| {
            map.into_iter()
                .filter_map(|(k, event_id)| {
                    crate::room::state::get_field(k)
                        .map(
                            |DbRoomStateField {
                                 event_ty, state_key, ..
                             }| ((event_ty.to_string().into(), state_key), event_id),
                        )
                        .ok()
                })
                .collect::<StateMap<_>>()
        })
        .collect();
    debug!("Resolving state");

    println!("VVresolve_state  4");
    // let lock = crate::STATERES_MUTEX.lock;
    let state =
        match state::resolve(
            room_version_id,
            &fork_states,
            auth_chain_sets,
            |id| match crate::room::timeline::get_pdu(id) {
                Err(e) => {
                    error!("LOOK AT ME Failed to fetch event: {}", e);
                    None
                }
                Ok(pdu) => pdu,
            },
        ) {
            Ok(new_state) => new_state,
            Err(_) => {
                return Err(AppError::internal(
                    "State resolution failed, either an event could not be found or deserialization",
                ));
            }
        };
    // drop(lock);

    debug!("State resolution done. Compressing state");

    println!("VVresolve_state  5");
    let new_room_state = state
        .into_iter()
        .map(|((event_type, state_key), event_id)| {
            let state_key_id = crate::room::state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
            let event_sn = crate::event::get_event_sn(&event_id)?;
            crate::room::state::compress_event(room_id, state_key_id, &event_id, event_sn)
        })
        .collect::<AppResult<_>>()?;

    Ok(Arc::new(new_room_state))
}

/// Find the event and auth it. Once the event is validated (steps 1 - 8)
/// it is appended to the outliers Tree.
///
/// Returns pdu and if we fetched it over federation the raw json.
///
/// a. Look in the main timeline (pduid_pdu tree)
/// b. Look at outlier pdu tree
/// c. Ask origin server over federation
/// d. TODO: Ask other servers over federation?
#[tracing::instrument(skip_all)]
pub(crate) async fn fetch_and_handle_outliers(
    origin: &ServerName,
    events: &[Arc<EventId>],
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
) -> AppResult<Vec<(PduEvent, Option<BTreeMap<String, CanonicalJsonValue>>)>> {
    let back_off = |id| match crate::BAD_EVENT_RATE_LIMITER.write().unwrap().entry(id) {
        hash_map::Entry::Vacant(e) => {
            e.insert((Instant::now(), 1));
        }
        hash_map::Entry::Occupied(mut e) => *e.get_mut() = (Instant::now(), e.get().1 + 1),
    };

    let mut pdus = vec![];
    for id in events {
        // a. Look in the main timeline (pduid_pdu tree)
        // b. Look at outlier pdu tree
        // (get_pdu_json checks both)
        if let Ok(Some(local_pdu)) = crate::room::timeline::get_pdu(id) {
            trace!("Found {} in db", id);
            pdus.push((local_pdu, None));
            continue;
        }

        // c. Ask origin server over federation
        // We also handle its auth chain here so we don't get a stack overflow in
        // handle_outlier_pdu.
        let mut todo_auth_events = vec![Arc::clone(id)];
        let mut events_in_reverse_order = Vec::new();
        let mut events_all = HashSet::new();
        while let Some(next_id) = todo_auth_events.pop() {
            if let Some((time, tries)) = crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&*next_id) {
                // Exponential backoff
                let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
                if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                    min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
                }

                if time.elapsed() < min_elapsed_duration {
                    info!("Backing off from {}", next_id);
                    continue;
                }
            }

            if events_all.contains(&next_id) {
                continue;
            }

            if let Ok(Some(_)) = crate::room::timeline::get_pdu(&next_id) {
                trace!("Found {} in db", next_id);
                continue;
            }

            info!("Fetching {} over federation.", next_id);
            let request = get_events_request(&origin.origin().await, &next_id)?.into_inner();
            match crate::sending::send_federation_request(&origin, request)
                .await?
                .json::<EventResBody>()
                .await
            {
                Ok(res) => {
                    info!("Got {} over federation", next_id);
                    let (calculated_event_id, value) =
                        match crate::event::gen_event_id_canonical_json(&res.pdu, room_version_id) {
                            Ok(t) => t,
                            Err(_) => {
                                back_off((*next_id).to_owned());
                                continue;
                            }
                        };

                    if calculated_event_id != *next_id {
                        warn!(
                            "Server didn't return event id we requested: requested: {}, we got {}. Event: {:?}",
                            next_id, calculated_event_id, &res.pdu
                        );
                    }

                    if let Some(auth_events) = value.get("auth_events").and_then(|c| c.as_array()) {
                        for auth_event in auth_events {
                            if let Ok(auth_event) = serde_json::from_value(auth_event.clone().into()) {
                                let a: Arc<EventId> = auth_event;
                                todo_auth_events.push(a);
                            } else {
                                warn!("Auth event id is not valid");
                            }
                        }
                    } else {
                        warn!("Auth event list invalid");
                    }

                    events_in_reverse_order.push((next_id.clone(), value));
                    events_all.insert(next_id);
                }
                Err(_) => {
                    warn!("Failed to fetch event: {}", next_id);
                    back_off((*next_id).to_owned());
                }
            }
        }

        for (next_id, value) in events_in_reverse_order.iter().rev() {
            if let Some((time, tries)) = crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&**next_id) {
                // Exponential backoff
                let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
                if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                    min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
                }

                if time.elapsed() < min_elapsed_duration {
                    info!("Backing off from {}", next_id);
                    continue;
                }
            }

            match handle_outlier_pdu(origin, next_id, room_id, room_version_id, value.clone(), true).await {
                Ok((pdu, json)) => {
                    if next_id == id {
                        pdus.push((pdu, Some(json)));
                    }
                }
                Err(e) => {
                    warn!("Authentication of event {} failed: {:?}", next_id, e);
                    back_off((**next_id).to_owned());
                }
            }
        }
    }
    Ok(pdus)
}

async fn fetch_missing_prev_events(
    origin: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    initial_set: Vec<Arc<EventId>>,
) -> AppResult<(
    Vec<Arc<EventId>>,
    HashMap<Arc<EventId>, (Arc<PduEvent>, BTreeMap<String, CanonicalJsonValue>)>,
)> {
    let conf = crate::config();
    let mut graph: HashMap<Arc<EventId>, _> = HashMap::new();
    let mut eventid_info = HashMap::new();
    let mut todo_outlier_stack: Vec<Arc<EventId>> = initial_set;

    // let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
    //     .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;

    let mut amount = 0;

    println!("xxxxxxxxxx   0");
    let room_version_id = &crate::room::room_version(room_id)?;
    while let Some(prev_event_id) = todo_outlier_stack.pop() {
        println!("xxxxxxxxxx   1");
        if let Some((pdu, json_opt)) =
            fetch_and_handle_outliers(origin, &[prev_event_id.clone()], room_id, room_version_id)
                .await?
                .pop()
        {
            println!("xxxxxxxxxx   2");
            check_room_id(room_id, &pdu)?;

            if amount > conf.max_fetch_prev_events {
                // Max limit reached
                warn!("Max prev event limit reached!");
                graph.insert(prev_event_id.clone(), HashSet::new());
                continue;
            }

            if let Some(json) = json_opt.or_else(|| crate::room::timeline::get_pdu_json(&prev_event_id).ok().flatten())
            {
                // if pdu.origin_server_ts > first_pdu_in_room.origin_server_ts {
                //     amount += 1;
                //     for prev_prev in &pdu.prev_events {
                //         if !graph.contains_key(prev_prev) {
                //             todo_outlier_stack.push(prev_prev.clone());
                //         }
                //     }

                //     graph.insert(prev_event_id.clone(), pdu.prev_events.iter().cloned().collect());
                // } else {
                // Time based check failed
                graph.insert(prev_event_id.clone(), HashSet::new());
                // }

                eventid_info.insert(prev_event_id.clone(), (Arc::new(pdu), json));
            } else {
                // Get json failed, so this was not fetched over federation
                graph.insert(prev_event_id.clone(), HashSet::new());
            }
        } else {
            // Fetch and handle failed
            graph.insert(prev_event_id.clone(), HashSet::new());
        }
    }

    let sorted = state::lexicographical_topological_sort(&graph, |event_id| {
        // This return value is the key used for sorting events,
        // events are then sorted by power level, time,
        // and lexically by event_id.
        Ok((
            0,
            eventid_info
                .get(event_id)
                .map_or_else(|| UnixMillis::default(), |info| info.0.origin_server_ts),
        ))
    })
    .map_err(|_| AppError::internal("Error sorting prev events"))?;

    Ok((sorted, eventid_info))
}

/// Returns Ok if the acl allows the server
pub fn acl_check(server_name: &ServerName, room_id: &RoomId) -> AppResult<()> {
    let acl_event = match crate::room::state::get_state(room_id, &StateEventType::RoomServerAcl, "", None)? {
        Some(acl) => acl,
        None => return Ok(()),
    };

    let acl_event_content: RoomServerAclEventContent = match serde_json::from_str(acl_event.content.get()) {
        Ok(content) => content,
        Err(_) => {
            warn!("Invalid ACL event");
            return Ok(());
        }
    };

    if acl_event_content.allow.is_empty() {
        // Ignore broken acl events
        return Ok(());
    }

    if acl_event_content.is_allowed(server_name) {
        Ok(())
    } else {
        info!("Server {} was denied by room ACL in {}", server_name, room_id);
        Err(MatrixError::forbidden("Server was denied by room ACL").into())
    }
}

fn check_room_id(room_id: &RoomId, pdu: &PduEvent) -> AppResult<()> {
    if pdu.room_id != room_id {
        warn!("Found event from room {} in room {}", pdu.room_id, room_id);
        return Err(MatrixError::invalid_param("Event has wrong room id").into());
    }
    Ok(())
}
