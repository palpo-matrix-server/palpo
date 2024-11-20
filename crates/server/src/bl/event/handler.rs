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
use crate::event::{NewDbEvent, PduEvent};
use crate::room::state::{CompressedStateEvent, DbRoomStateField};
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
#[tracing::instrument(skip(value, is_timeline_event, pub_key_map))]
pub(crate) async fn handle_incoming_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    value: BTreeMap<String, CanonicalJsonValue>,
    is_timeline_event: bool,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
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
    let create_event = crate::room::state::get_state(room_id, &StateEventType::RoomCreate, "", None)?
        .ok_or_else(|| AppError::internal("Failed to find create event in database"))?;

    let create_event_content: RoomCreateEventContent =
        serde_json::from_str(create_event.content.get()).map_err(|e| {
            error!("Invalid create event: {}", e);
            AppError::public("Invalid create event in db")
        })?;
    let room_version_id = &create_event_content.room_version;

    let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
        .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;

    let (incoming_pdu, val) =
        handle_outlier_pdu(origin, &create_event, event_id, room_id, value, false, pub_key_map).await?;
    check_room_id(room_id, &incoming_pdu)?;

    // 8. if not timeline event: stop
    if !is_timeline_event {
        return Ok(());
    }

    // Skip old events
    if incoming_pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
        return Ok(());
    }

    // 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
    let (sorted_prev_events, mut eventid_info) = fetch_unknown_prev_events(
        origin,
        &create_event,
        room_id,
        room_version_id,
        pub_key_map,
        incoming_pdu.prev_events.clone(),
    )
    .await?;

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
            // Skip old events
            if pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
                continue;
            }

            let start_time = Instant::now();
            crate::ROOM_ID_FEDERATION_HANDLE_TIME
                .write()
                .unwrap()
                .insert(room_id.to_owned(), ((*prev_id).to_owned(), start_time));

            if let Err(e) =
                upgrade_outlier_to_timeline_pdu(&pdu, json, &create_event, origin, room_id, pub_key_map).await
            {
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
    crate::event::handler::upgrade_outlier_to_timeline_pdu(
        &incoming_pdu,
        val,
        &create_event,
        origin,
        room_id,
        pub_key_map,
    )
    .await?;
    crate::ROOM_ID_FEDERATION_HANDLE_TIME
        .write()
        .unwrap()
        .remove(&room_id.to_owned());
    Ok(())
}

#[tracing::instrument(skip_all)]
fn handle_outlier_pdu<'a>(
    origin: &'a ServerName,
    create_event: &'a PduEvent,
    event_id: &'a EventId,
    room_id: &'a RoomId,
    mut value: BTreeMap<String, CanonicalJsonValue>,
    auth_events_known: bool,
    pub_key_map: &'a RwLock<BTreeMap<String, SigningKeys>>,
) -> Pin<Box<impl Future<Output = AppResult<(PduEvent, BTreeMap<String, CanonicalJsonValue>)>> + 'a + Send>> {
    Box::pin(async move {
        // 1.1. Remove unsigned field
        value.remove("unsigned");

        // 2. Check signatures, otherwise drop
        // 3. check content hash, redact if doesn't match
        let create_event_content: RoomCreateEventContent =
            serde_json::from_str(create_event.content.get()).map_err(|e| {
                error!("Invalid create event: {}", e);
                AppError::public("Invalid create event in db")
            })?;

        let room_version_id = &create_event_content.room_version;
        let room_version = RoomVersion::new(room_version_id).expect("room version is supported");

        // TODO: For RoomVersion6 we must check that RawJson<..> is canonical do we anywhere?: https://matrix.org/docs/spec/rooms/v6#canonical-json

        // We go through all the signatures we see on the value and fetch the corresponding signing
        // keys
        fetch_required_signing_keys(&value, pub_key_map).await?;

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

        let guard = pub_key_map.read().await;
        let pkey_map = (*guard).clone();

        // Removing all the expired keys, unless the room version allows stale keys
        let filtered_keys = crate::filter_keys_server_map(pkey_map, origin_server_ts, room_version_id);

        let mut val = match crate::core::signatures::verify_event(&filtered_keys, &value, room_version_id) {
            Err(e) => {
                // Drop
                warn!("Dropping bad event {}: {}", event_id, e,);
                return Err(MatrixError::invalid_param("Signature verification failed").into());
            }
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
        };

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

        check_room_id(room_id, &incoming_pdu)?;

        if !auth_events_known {
            // 4. fetch any missing auth events doing all checks listed here starting at 1. These are not timeline events
            // 5. Reject "due to auth events" if can't get all the auth events or some of the auth events are also rejected "due to auth events"
            // NOTE: Step 5 is not applied anymore because it failed too often
            debug!(event_id = ?incoming_pdu.event_id, "Fetching auth events");
            fetch_and_handle_outliers(
                origin,
                &incoming_pdu
                    .auth_events
                    .iter()
                    .map(|x| Arc::from(&**x))
                    .collect::<Vec<_>>(),
                create_event,
                room_id,
                room_version_id,
                pub_key_map,
            )
            .await?;
        }

        // 6. Reject "due to auth events" if the event doesn't pass auth based on the auth events
        debug!("Auth check for {} based on auth events", incoming_pdu.event_id);

        // Build map of auth events
        let mut auth_events = HashMap::new();
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
                auth_event.kind.to_string().into(),
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

#[tracing::instrument(skip(incoming_pdu, val, create_event, pub_key_map))]
pub async fn upgrade_outlier_to_timeline_pdu(
    incoming_pdu: &PduEvent,
    val: BTreeMap<String, CanonicalJsonValue>,
    create_event: &PduEvent,
    origin: &ServerName,
    room_id: &RoomId,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    let conf = crate::config();
    if crate::room::pdu_metadata::is_event_soft_failed(&incoming_pdu.event_id)? {
        return Err(MatrixError::invalid_param("Event has been soft failed").into());
    }

    info!("Upgrading {} to timeline pdu", incoming_pdu.event_id);

    let create_event_content: RoomCreateEventContent =
        serde_json::from_str(create_event.content.get()).map_err(|e| {
            warn!("Invalid create event: {}", e);
            AppError::public("Invalid create event in db")
        })?;

    let room_version_id = &create_event_content.room_version;
    let room_version = RoomVersion::new(room_version_id).expect("room version is supported");

    // 10. Fetch missing state and auth chain events by calling /state_ids at backwards extremities
    //     doing all the checks in this list starting at 1. These are not timeline events.

    // TODO: if we know the prev_events of the incoming event we can avoid the request and build
    // the state from a known point and resolve if > 1 prev_event

    debug!("Requesting state at event");
    let mut state_at_incoming_event = None;

    if incoming_pdu.prev_events.len() == 1 {
        let prev_event = &*incoming_pdu.prev_events[0];
        let prev_frame_id = crate::room::state::get_pdu_frame_id(prev_event)?;

        let state = if let Some(frame_id) = prev_frame_id {
            Some(crate::room::state::get_full_state_ids(frame_id))
        } else {
            None
        };

        if let Some(Ok(mut state)) = state {
            debug!("Using cached state");
            let prev_pdu = crate::room::timeline::get_pdu(prev_event)
                .ok()
                .flatten()
                .ok_or_else(|| AppError::internal("Could not find prev event, but we know the state."))?;

            if let Some(state_key) = &prev_pdu.state_key {
                let state_key_id = crate::room::state::ensure_field_id(&prev_pdu.kind.to_string().into(), state_key)?;

                state.insert(state_key_id, Arc::from(prev_event));
                // Now it's the state after the pdu
            }

            state_at_incoming_event = Some(state);
        }
    } else {
        debug!("Calculating state at event using state res");
        let mut extremity_sstate_hashes = HashMap::new();

        let mut okay = true;
        for prev_eventid in &incoming_pdu.prev_events {
            let prev_event = if let Ok(Some(pdu)) = crate::room::timeline::get_pdu(prev_eventid) {
                pdu
            } else {
                okay = false;
                break;
            };

            let sstate_hash = if let Ok(Some(s)) = crate::room::state::get_pdu_frame_id(prev_eventid) {
                s
            } else {
                okay = false;
                break;
            };

            extremity_sstate_hashes.insert(sstate_hash, prev_event);
        }

        if okay {
            let mut fork_states = Vec::with_capacity(extremity_sstate_hashes.len());
            let mut auth_chain_sets = Vec::with_capacity(extremity_sstate_hashes.len());

            for (sstate_hash, prev_event) in extremity_sstate_hashes {
                let mut leaf_state: HashMap<_, _> = crate::room::state::get_full_state_ids(sstate_hash)?;

                if let Some(state_key) = &prev_event.state_key {
                    let state_key_id =
                        crate::room::state::ensure_field_id(&prev_event.kind.to_string().into(), state_key)?;
                    leaf_state.insert(state_key_id, Arc::from(&*prev_event.event_id));
                    // Now it's the state after the pdu
                }

                let mut state = StateMap::with_capacity(leaf_state.len());
                let mut starting_events = Vec::with_capacity(leaf_state.len());

                for (k, id) in leaf_state {
                    if let Ok(DbRoomStateField {
                        event_type, state_key, ..
                    }) = crate::room::state::get_field(k)
                    {
                        // FIXME: Undo .to_string().into() when StateMap
                        //        is updated to use StateEventType
                        state.insert((event_type.to_string().into(), state_key), id.clone());
                    } else {
                        warn!("Failed to get_state_key_id.");
                    }
                    starting_events.push(id);
                }

                for starting_event in starting_events {
                    auth_chain_sets.push(crate::room::auth_chain::get_auth_chain(room_id, &starting_event)?);
                }

                fork_states.push(state);
            }

            let lock = crate::STATERES_MUTEX.lock();

            let result = state::resolve(room_version_id, &fork_states, auth_chain_sets, |id| {
                let res = crate::room::timeline::get_pdu(id);
                if let Err(e) = &res {
                    error!("LOOK AT ME Failed to fetch event: {}", e);
                }
                res.ok().flatten()
            });
            drop(lock);

            state_at_incoming_event = match result {
                Ok(new_state) => Some(
                    new_state
                        .into_iter()
                        .map(|((event_type, state_key), event_id)| {
                            let state_key_id =
                                crate::room::state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
                            Ok((state_key_id, event_id))
                        })
                        .collect::<AppResult<_>>()?,
                ),
                Err(e) => {
                    warn!(
                        "State resolution on prev events failed, either an event could not be found or deserialization: {}",
                        e
                    );
                    None
                }
            }
        }
    }

    if state_at_incoming_event.is_none() {
        debug!("Calling /state_ids");
        // Call /state_ids to find out what the state at this pdu is. We trust the server's
        // response to some extend, but we still do a lot of checks on the events
        let request = room_state_ids_request(
            &origin.origin().await,
            RoomStateAtEventReqArgs {
                room_id: room_id.to_owned(),
                event_id: (&*incoming_pdu.event_id).to_owned(),
            },
        )?
        .into_inner();
        match crate::sending::send_federation_request(origin, request)
            .await?
            .json::<RoomStateIdsResBody>()
            .await
        {
            Ok(res) => {
                debug!("Fetching state events at event.");
                let state_vec = fetch_and_handle_outliers(
                    origin,
                    &res.pdu_ids.iter().map(|x| Arc::from(&**x)).collect::<Vec<_>>(),
                    create_event,
                    room_id,
                    room_version_id,
                    pub_key_map,
                )
                .await?;

                let mut state: HashMap<_, Arc<EventId>> = HashMap::new();
                for (pdu, _) in state_vec {
                    let state_key = pdu
                        .state_key
                        .clone()
                        .ok_or_else(|| AppError::internal("Found non-state pdu in state events."))?;

                    let state_key_id = crate::room::state::ensure_field_id(&pdu.kind.to_string().into(), &state_key)?;

                    match state.entry(state_key_id) {
                        hash_map::Entry::Vacant(v) => {
                            v.insert(Arc::from(&*pdu.event_id));
                        }
                        hash_map::Entry::Occupied(_) => {
                            return Err(AppError::internal(
                                "State event's type and state_key combination exists multiple times.",
                            ))
                        }
                    }
                }

                // The original create event must still be in the state
                let create_state_key_id = crate::room::state::ensure_field_id(&StateEventType::RoomCreate, "")?;

                if state.get(&create_state_key_id).map(|id| id.as_ref()) != Some(&create_event.event_id) {
                    return Err(AppError::internal("Incoming event refers to wrong create event."));
                }

                state_at_incoming_event = Some(state);
            }
            Err(e) => {
                warn!("Fetching state for event failed: {}", e);
                return Err(e.into());
            }
        };
    }

    let state_at_incoming_event = state_at_incoming_event.expect("we always set this to some above");

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

    if !check_result {
        return Err(AppError::internal(
            "Event has failed auth check with state at the event.",
        ));
    }
    debug!("Auth check succeeded");

    // Soft fail check before doing state res
    let auth_events = crate::room::state::get_auth_events(
        room_id,
        &incoming_pdu.kind,
        &incoming_pdu.sender,
        incoming_pdu.state_key.as_deref(),
        &incoming_pdu.content,
    )?;

    let soft_fail = !state::event_auth::auth_check(&room_version, &incoming_pdu, None::<PduEvent>, |k, s| {
        auth_events.get(&(k.clone(), s.to_owned()))
    })
    .map_err(|_e| MatrixError::invalid_param("Auth check failed before doing state"))?;

    // 13. Use state resolution to find new room state

    // We start looking at current room state now, so lets lock the room
    // Now we calculate the set of extremities this room has after the incoming event has been
    // applied. We start with the previous extremities (aka leaves)
    debug!("Calculating extremities");
    let mut extremities = crate::room::state::get_forward_extremities(room_id)?;

    // Remove any forward extremities that are referenced by this incoming event's prev_events
    for prev_event in &incoming_pdu.prev_events {
        if extremities.contains(prev_event) {
            extremities.remove(prev_event);
        }
    }

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

    if incoming_pdu.state_key.is_some() {
        debug!("Preparing for stateres to derive new room state");

        // We also add state after incoming event to the fork states
        let mut state_after = state_at_incoming_event.clone();
        if let Some(state_key) = &incoming_pdu.state_key {
            let state_key_id = crate::room::state::ensure_field_id(&incoming_pdu.kind.to_string().into(), state_key)?;

            state_after.insert(state_key_id, Arc::from(&*incoming_pdu.event_id));
        }

        let new_room_state = resolve_state(room_id, room_version_id, state_after)?;

        // Set the new room state to the resolved state
        debug!("Forcing new room state");

        let (sstate_hash, new, removed) = crate::room::state::save_state(room_id, new_room_state)?;

        crate::room::state::force_state(room_id, sstate_hash, new, removed)?;
    }

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

    debug!("Appended incoming pdu");

    // Event has passed all auth/stateres checks
    // drop(state_lock);
    Ok(())
}

fn resolve_state(
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    incoming_state: HashMap<i64, Arc<EventId>>,
) -> AppResult<Arc<HashSet<CompressedStateEvent>>> {
    debug!("Loading current room state ids");
    let current_frame_id = crate::room::state::get_room_frame_id(room_id, None)?
        .ok_or_else(|| AppError::public("every room has state"))?;

    let current_state_ids = crate::room::state::get_full_state_ids(current_frame_id)?;

    let fork_states = [current_state_ids, incoming_state];

    let mut auth_chain_sets = Vec::new();
    debug!("Loading fork states");
    for state in &fork_states {
        for event_id in state.values() {
            auth_chain_sets.push(crate::room::auth_chain::get_auth_chain(room_id, event_id)?);
        }
    }

    let fork_states: Vec<_> = fork_states
        .into_iter()
        .map(|map| {
            map.into_iter()
                .filter_map(|(k, event_id)| {
                    crate::room::state::get_field(k)
                        .map(
                            |DbRoomStateField {
                                 event_type, state_key, ..
                             }| ((event_type.to_string().into(), state_key), event_id),
                        )
                        .ok()
                })
                .collect::<StateMap<_>>()
        })
        .collect();

    debug!("Resolving state");

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
    create_event: &PduEvent,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
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

            match handle_outlier_pdu(origin, create_event, next_id, room_id, value.clone(), true, pub_key_map).await {
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

async fn fetch_unknown_prev_events(
    origin: &ServerName,
    create_event: &PduEvent,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
    initial_set: Vec<Arc<EventId>>,
) -> AppResult<(
    Vec<Arc<EventId>>,
    HashMap<Arc<EventId>, (Arc<PduEvent>, BTreeMap<String, CanonicalJsonValue>)>,
)> {
    let conf = crate::config();
    let mut graph: HashMap<Arc<EventId>, _> = HashMap::new();
    let mut eventid_info = HashMap::new();
    let mut todo_outlier_stack: Vec<Arc<EventId>> = initial_set;

    let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
        .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;

    let mut amount = 0;

    while let Some(prev_event_id) = todo_outlier_stack.pop() {
        if let Some((pdu, json_opt)) = fetch_and_handle_outliers(
            origin,
            &[prev_event_id.clone()],
            create_event,
            room_id,
            room_version_id,
            pub_key_map,
        )
        .await?
        .pop()
        {
            check_room_id(room_id, &pdu)?;

            if amount > conf.max_fetch_prev_events {
                // Max limit reached
                warn!("Max prev event limit reached!");
                graph.insert(prev_event_id.clone(), HashSet::new());
                continue;
            }

            if let Some(json) = json_opt.or_else(|| crate::room::timeline::get_pdu_json(&prev_event_id).ok().flatten())
            {
                if pdu.origin_server_ts > first_pdu_in_room.origin_server_ts {
                    amount += 1;
                    for prev_prev in &pdu.prev_events {
                        if !graph.contains_key(prev_prev) {
                            todo_outlier_stack.push(prev_prev.clone());
                        }
                    }

                    graph.insert(prev_event_id.clone(), pdu.prev_events.iter().cloned().collect());
                } else {
                    // Time based check failed
                    graph.insert(prev_event_id.clone(), HashSet::new());
                }

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

#[tracing::instrument(skip_all)]
pub(crate) async fn fetch_required_signing_keys(
    event: &BTreeMap<String, CanonicalJsonValue>,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    let signatures = event
        .get("signatures")
        .ok_or(AppError::public("No signatures in server response pdu."))?
        .as_object()
        .ok_or(AppError::public("Invalid signatures object in server response pdu."))?;

    // We go through all the signatures we see on the value and fetch the corresponding signing
    // keys
    for (signature_server, signature) in signatures {
        let signature_object = signature.as_object().ok_or(AppError::public(
            "Invalid signatures content object in server response pdu.",
        ))?;

        let signature_ids = signature_object.keys().cloned().collect::<Vec<_>>();

        let fetch_res = fetch_signing_keys(
            signature_server
                .as_str()
                .try_into()
                .map_err(|_| AppError::public("Invalid servername in signatures of server response pdu."))?,
            signature_ids,
        )
        .await;

        let keys = match fetch_res {
            Ok(keys) => keys,
            Err(_) => {
                warn!("Signature verification failed: Could not fetch signing key.",);
                continue;
            }
        };

        pub_key_map.write().await.insert(signature_server.clone(), keys);
    }

    Ok(())
}

// Gets a list of servers for which we don't have the signing key yet. We go over
// the PDUs and either cache the key or add it to the list that needs to be retrieved.
fn get_server_keys_from_cache(
    pdu: &RawJsonValue,
    servers: &mut BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>>,
    room_version: &RoomVersionId,
    pub_key_map: &mut RwLockWriteGuard<'_, BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    let value: CanonicalJsonObject = serde_json::from_str(pdu.get()).map_err(|e| {
        error!("Invalid PDU in server response: {:?}: {:?}", pdu, e);
        AppError::public("Invalid PDU in server response")
    })?;

    let event_id = format!(
        "${}",
        crate::core::signatures::reference_hash(&value, room_version).expect("palpo can calculate reference hashes")
    );
    let event_id = <&EventId>::try_from(event_id.as_str()).expect("palpo's reference hashes are valid event ids");

    if let Some((time, tries)) = crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(event_id) {
        // Exponential backoff
        let mut min_elapsed_duration = Duration::from_secs(30) * (*tries) * (*tries);
        if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
            min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
        }

        if time.elapsed() < min_elapsed_duration {
            debug!("Backing off from {}", event_id);
            return Err(AppError::public("bad event, still backing off"));
        }
    }

    let signatures = value
        .get("signatures")
        .ok_or(AppError::public("No signatures in server response pdu."))?
        .as_object()
        .ok_or(AppError::public("Invalid signatures object in server response pdu."))?;

    for (signature_server, signature) in signatures {
        let signature_object = signature.as_object().ok_or(AppError::public(
            "Invalid signatures content object in server response pdu.",
        ))?;

        let signature_ids = signature_object.keys().cloned().collect::<Vec<_>>();

        let contains_all_ids = |keys: &SigningKeys| {
            signature_ids.iter().all(|id| {
                keys.verify_keys
                    .keys()
                    .map(ToString::to_string)
                    .any(|key_id| id == &key_id)
                    || keys
                        .old_verify_keys
                        .keys()
                        .map(ToString::to_string)
                        .any(|key_id| id == &key_id)
            })
        };

        let origin = <&ServerName>::try_from(signature_server.as_str())
            .map_err(|_| AppError::public("Invalid servername in signatures of server response pdu."))?;

        if servers.contains_key(origin) || pub_key_map.contains_key(origin.as_str()) {
            continue;
        }

        trace!("Loading signing keys for {}", origin);

        if let Some(result) = crate::signing_keys_for(origin)? {
            if !contains_all_ids(&result) {
                trace!("Signing key not loaded for {}", origin);
                servers.insert(origin.to_owned(), BTreeMap::new());
            }

            pub_key_map.insert(origin.to_string(), result);
        }
    }

    Ok(())
}

pub(crate) async fn fetch_join_signing_keys(
    event: &SendJoinResBodyV2,
    room_version: &RoomVersionId,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    let conf = crate::config();
    let mut servers: BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>> = BTreeMap::new();

    {
        let mut pkm = pub_key_map.write().await;

        // Try to fetch keys, failure is okay
        // Servers we couldn't find in the cache will be added to `servers`
        for pdu in &event.room_state.state {
            let _ = get_server_keys_from_cache(pdu, &mut servers, room_version, &mut pkm);
        }
        for pdu in &event.room_state.auth_chain {
            let _ = get_server_keys_from_cache(pdu, &mut servers, room_version, &mut pkm);
        }

        drop(pkm);
    }

    if servers.is_empty() {
        info!("We had all keys locally");
        return Ok(());
    }
    for server in &conf.trusted_servers {
        info!("Asking batch signing keys from trusted server {}", server);
        let request = remote_server_keys_batch_request(
            &server.origin().await,
            RemoteServerKeysBatchReqBody {
                server_keys: servers.clone(),
            },
        )?
        .into_inner();
        if let Ok(keys) = crate::sending::send_federation_request(&server, request)
            .await?
            .json::<RemoteServerKeysBatchResBody>()
            .await
        {
            trace!("Got signing keys: {:?}", keys);
            let mut pkm = pub_key_map.write().await;
            for k in keys.server_keys {
                let k = match k.deserialize() {
                    Ok(key) => key,
                    Err(e) => {
                        warn!(
                            "Received error {} while fetching keys from trusted server {}",
                            e, server
                        );
                        // warn!("{}", k.into_json());
                        continue;
                    }
                };

                // TODO: Check signature from trusted server?
                servers.remove(&k.server_name);

                let result = crate::add_signing_key_from_trusted_server(&k.server_name, k.clone())?;

                pkm.insert(k.server_name.to_string(), result);
            }
        }

        if servers.is_empty() {
            info!("Trusted server supplied all signing keys");
            return Ok(());
        }
    }

    info!("Asking individual servers for signing keys: {servers:?}");
    let mut futures: FuturesUnordered<_> = servers
        .into_keys()
        .map(|server| async move {
            let request = get_server_key_request(&server.origin().await)?.into_inner();
            let server_keys = crate::sending::send_federation_request(&server, request)
                .await?
                .json::<ServerKeysResBody>()
                .await;
            Ok::<_, AppError>((server_keys, server))
        })
        .collect();

    while let Some(result) = futures.next().await {
        info!("Received new result");
        if let Ok((Ok(get_keys_response), origin)) = result {
            info!("Result is from {origin}");
            let result = crate::add_signing_key_from_origin(&origin, get_keys_response.0.clone())?;
            pub_key_map.write().await.insert(origin.to_string(), result);
        }
        info!("Done handling result");
    }
    info!("Search for signing keys done");
    Ok(())
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

/// Search the DB for the signing keys of the given server, if we don't have them
/// fetch them from the server and save to our DB.
#[tracing::instrument(skip_all)]
pub async fn fetch_signing_keys(origin: &ServerName, signature_ids: Vec<String>) -> AppResult<SigningKeys> {
    let contains_all_ids = |keys: &SigningKeys| {
        signature_ids.iter().all(|id| {
            keys.verify_keys
                .keys()
                .map(ToString::to_string)
                .any(|key_id| id == &key_id)
                || keys
                    .old_verify_keys
                    .keys()
                    .map(ToString::to_string)
                    .any(|key_id| id == &key_id)
        })
    };
    let conf = crate::config();
    let permit = crate::SERVER_NAME_RATE_LIMITER
        .read()
        .unwrap()
        .get(origin)
        .map(|s| Arc::clone(s).acquire_owned());

    let permit = match permit {
        Some(p) => p,
        None => {
            let mut write = crate::SERVER_NAME_RATE_LIMITER.write().unwrap();
            let s = Arc::clone(
                write
                    .entry(origin.to_owned())
                    .or_insert_with(|| Arc::new(Semaphore::new(1))),
            );

            s.acquire_owned()
        }
    }
    .await;

    let back_off = |id| match crate::BAD_SIGNATURE_RATE_LIMITER.write().unwrap().entry(id) {
        hash_map::Entry::Vacant(e) => {
            e.insert((Instant::now(), 1));
        }
        hash_map::Entry::Occupied(mut e) => *e.get_mut() = (Instant::now(), e.get().1 + 1),
    };

    if let Some((time, tries)) = crate::BAD_SIGNATURE_RATE_LIMITER.read().unwrap().get(&signature_ids) {
        // Exponential backoff
        let mut min_elapsed_duration = Duration::from_secs(30) * (*tries) * (*tries);
        if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
            min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
        }

        if time.elapsed() < min_elapsed_duration {
            debug!("Backing off from {:?}", signature_ids);
            return Err(AppError::public("bad signature, still backing off"));
        }
    }

    trace!("Loading signing keys for {}", origin);

    let result = crate::signing_keys_for(origin)?;

    let mut expires_soon_or_has_expired = false;

    if let Some(result) = result.clone() {
        let ts_threshold = UnixMillis::from_system_time(SystemTime::now() + Duration::from_secs(30 * 60))
            .expect("Should be valid until year 500,000,000");

        debug!(
            "The treshhold is {:?}, found time is {:?} for server {}",
            ts_threshold, result.valid_until_ts, origin
        );

        if contains_all_ids(&result) {
            // We want to ensure that the keys remain valid by the time the other functions that handle signatures reach them
            if result.valid_until_ts > ts_threshold {
                debug!(
                    "Keys for {} are deemed as valid, as they expire at {:?}",
                    &origin, &result.valid_until_ts
                );
                return Ok(result);
            }

            expires_soon_or_has_expired = true;
        }
    }

    let mut keys = result.unwrap_or_else(|| SigningKeys {
        verify_keys: BTreeMap::new(),
        old_verify_keys: BTreeMap::new(),
        valid_until_ts: UnixMillis::now(),
    });

    // We want to set this to the max, and then lower it whenever we see older keys
    keys.valid_until_ts = UnixMillis::from_system_time(SystemTime::now() + Duration::from_secs(7 * 86400))
        .expect("Should be valid until year 500,000,000");

    debug!("Fetching signing keys for {} over federation", origin);

    let key_request = get_server_key_request(&origin.origin().await)?.into_inner();
    if let Some(mut server_key) = crate::sending::send_federation_request(origin, key_request)
        .await?
        .json::<ServerKeysResBody>()
        .await
        .ok()
        .map(|resp| resp.0)
    {
        // Keys should only be valid for a maximum of seven days
        server_key.valid_until_ts = server_key.valid_until_ts.min(
            UnixMillis::from_system_time(SystemTime::now() + Duration::from_secs(7 * 86400))
                .expect("Should be valid until year 500,000,000"),
        );

        crate::add_signing_key_from_origin(origin, server_key.clone())?;

        if keys.valid_until_ts > server_key.valid_until_ts {
            keys.valid_until_ts = server_key.valid_until_ts;
        }

        keys.verify_keys.extend(
            server_key
                .verify_keys
                .into_iter()
                .map(|(id, key)| (id.to_string(), key)),
        );
        keys.old_verify_keys.extend(
            server_key
                .old_verify_keys
                .into_iter()
                .map(|(id, key)| (id.to_string(), key)),
        );

        if contains_all_ids(&keys) {
            return Ok(keys);
        }
    }

    for server in &conf.trusted_servers {
        debug!("Asking {} for {}'s signing key", server, origin);
        let keys_request = remote_server_keys_request(
            &server.origin().await,
            RemoteServerKeysReqArgs {
                server_name: origin.to_owned(),
                minimum_valid_until_ts: UnixMillis::from_system_time(
                    SystemTime::now()
                        .checked_add(Duration::from_secs(3600))
                        .expect("SystemTime to large"),
                )
                .unwrap_or(UnixMillis::now()),
            },
        )?
        .into_inner();
        if let Some(server_keys) = crate::sending::send_federation_request(server, keys_request)
            .await?
            .json::<RemoteServerKeysBatchResBody>()
            .await
            .ok()
            .map(|resp| {
                resp.server_keys
                    .into_iter()
                    .filter_map(|e| e.deserialize().ok())
                    .collect::<Vec<_>>()
            })
        {
            trace!("Got signing keys: {:?}", server_keys);
            for mut k in server_keys {
                if k.valid_until_ts
                    // Half an hour should give plenty of time for the server to respond with keys that are still
                    // valid, given we requested keys which are valid at least an hour from now
                    < UnixMillis::from_system_time(
                    SystemTime::now() + Duration::from_secs(30 * 60),
                )
                    .expect("Should be valid until year 500,000,000")
                {
                    // Keys should only be valid for a maximum of seven days
                    k.valid_until_ts = k.valid_until_ts.min(
                        UnixMillis::from_system_time(SystemTime::now() + Duration::from_secs(7 * 86400))
                            .expect("Should be valid until year 500,000,000"),
                    );

                    if keys.valid_until_ts > k.valid_until_ts {
                        keys.valid_until_ts = k.valid_until_ts;
                    }

                    crate::add_signing_key_from_trusted_server(origin, k.clone())?;
                    keys.verify_keys
                        .extend(k.verify_keys.into_iter().map(|(id, key)| (id.to_string(), key)));
                    keys.old_verify_keys
                        .extend(k.old_verify_keys.into_iter().map(|(id, key)| (id.to_string(), key)));
                } else {
                    warn!(
                        "Server {} gave us keys older than we requested, valid until: {:?}",
                        origin, k.valid_until_ts
                    );
                }

                if contains_all_ids(&keys) {
                    return Ok(keys);
                }
            }
        }
    }

    drop(permit);

    back_off(signature_ids);

    warn!("Failed to find public key for server: {}", origin);
    Err(AppError::public("Failed to find public key for server").into())
}

fn check_room_id(room_id: &RoomId, pdu: &PduEvent) -> AppResult<()> {
    if pdu.room_id != room_id {
        warn!("Found event from room {} in room {}", pdu.room_id, room_id);
        return Err(MatrixError::invalid_param("Event has wrong room id").into());
    }
    Ok(())
}
