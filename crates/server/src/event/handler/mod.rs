mod fetch_state;
mod state_at_incoming;
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque, hash_map};
use std::future::Future;
use std::iter::once;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use diesel::prelude::*;
use fetch_state::fetch_state;
use palpo_core::federation::event::EventResBody;
use state_at_incoming::{state_at_incoming_degree_one, state_at_incoming_resolved};

use crate::core::UnixMillis;
use crate::core::events::StateEventType;
use crate::core::events::room::server_acl::RoomServerAclEventContent;
use crate::core::federation::event::{EventReqArgs, event_request};
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonValue, canonical_json};
use crate::core::state::{RoomVersion, StateMap, event_auth};
use crate::data::connect;
use crate::data::room::{DbEventData, NewDbEvent};
use crate::data::schema::*;
use crate::event::PduEvent;
use crate::room::state::{CompressedState, DbRoomStateField, DeltaInfo};
use crate::{AppError, AppResult, MatrixError, config, exts::*};

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
    if !crate::room::room_exists(room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server").into());
    }

    // 1.2 Check if the room is disabled
    if crate::room::is_disabled(room_id)? {
        return Err(
            MatrixError::forbidden("Federation of this room is currently disabled on this server.", None).into(),
        );
    }

    // 1.3.1 Check room ACL on origin field/server
    crate::event::handler::acl_check(origin, &room_id)?;

    // 1.3.2 Check room ACL on sender's server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::invalid_param("PDU does not have a valid sender key: {e}"))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("User ID in sender is invalid."))?;

    if sender.server_name().ne(origin) {
        crate::event::handler::acl_check(sender.server_name(), room_id)?;
    }

    // 1. Skip the PDU if we already have it as a timeline event
    if crate::room::state::get_pdu_frame_id(event_id).is_ok() {
        println!("skipped");
        return Ok(());
    }

    let room_version_id = &crate::room::room_version(room_id)?;

    let (incoming_pdu, val) = handle_outlier_pdu(origin, event_id, room_id, room_version_id, value, false).await?;

    check_room_id(room_id, &incoming_pdu)?;

    // 8. if not timeline event: stop
    if !is_timeline_event {
        return Ok(());
    }

    // Skip old events
    let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
        .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;
    if incoming_pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
        println!(" incoming_pdu.origin_server_ts < first_pdu_in_room.origin_server_ts");
        return Ok(());
    }

    // 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
    let (sorted_prev_events, mut event_info) =
        fetch_missing_prev_events(origin, room_id, room_version_id, incoming_pdu.prev_events.clone()).await?;

    debug!(events = ?sorted_prev_events, "Got previous events");
    for prev_id in sorted_prev_events {
        if let Err(e) = handle_prev_pdu(
            origin,
            event_id,
            room_id,
            &mut event_info,
            &incoming_pdu,
            first_pdu_in_room.origin_server_ts,
            &prev_id,
        )
        .await
        {
            warn!("Prev event {prev_id} failed: {e}");
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

async fn handle_prev_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    event_info: &mut HashMap<Arc<EventId>, (Arc<PduEvent>, BTreeMap<String, CanonicalJsonValue>)>,
    create_event: &PduEvent,
    first_ts_in_room: UnixMillis,
    prev_id: &EventId,
) -> AppResult<()> {
    if let Some((time, tries)) = crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&*prev_id) {
        // Exponential backoff
        let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
        if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
            min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
        }

        if time.elapsed() < min_elapsed_duration {
            info!("Backing off from {}", prev_id);
            return Ok(());
        }
    }

    if let Some((pdu, json)) = event_info.remove(&*prev_id) {
        // Skip old events
        if pdu.origin_server_ts < first_ts_in_room {
            println!("skip old event {}", pdu.event_id);
            return Ok(());
        }

        let start_time = Instant::now();
        crate::ROOM_ID_FEDERATION_HANDLE_TIME
            .write()
            .unwrap()
            .insert(room_id.to_owned(), ((*prev_id).to_owned(), start_time));

        upgrade_outlier_to_timeline_pdu(&pdu, json, origin, room_id).await?;

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
                let obj = match canonical_json::redact(value, room_version_id, None) {
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

        // Now that we have checked the signature and hashes we can add the eventID and convert
        // to our PduEvent type
        val.insert(
            "event_id".to_owned(),
            CanonicalJsonValue::String(event_id.as_str().to_owned()),
        );
        let incoming_pdu = PduEvent::from_json_value(
            event_id,
            crate::event::ensure_event_sn(room_id, event_id)?,
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
                room_id,
                room_version_id,
            )
            .await?;
        }

        // 6. Reject "due to auth events" if the event doesn't pass auth based on the auth events
        debug!("Auth check for {} based on auth events", incoming_pdu.event_id);

        // Build map of auth events
        let mut auth_events = HashMap::new();
        for id in &incoming_pdu.auth_events {
            let auth_event = match crate::room::timeline::get_pdu(id) {
                Ok(e) => e,
                Err(_) => {
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

        // if !state::event_auth::auth_check(
        //     &room_version,
        //     &incoming_pdu,
        //     None::<PduEvent>, // TODO: third party invite
        //     |k, s| auth_events.get(&(k.to_string().into(), s.to_owned())),
        // )
        // .map_err(|_e| MatrixError::invalid_param("Auth check failed outlier pdu"))?
        // {
        //     return Err(MatrixError::invalid_param("Auth check failed outlier pdu").into());
        // }

        debug!("Validation successful.");

        // 7. Persist the event as an outlier.
        let mut db_event = NewDbEvent::from_canonical_json(&incoming_pdu.event_id, incoming_pdu.event_sn, &val)?;
        db_event.is_outlier = true;

        diesel::insert_into(events::table)
            .values(db_event)
            .on_conflict_do_nothing()
            .execute(&mut connect()?)?;
        let event_data = DbEventData {
            event_id: (&*incoming_pdu.event_id).to_owned(),
            event_sn: incoming_pdu.event_sn,
            room_id: incoming_pdu.room_id.clone(),
            internal_metadata: None,
            json_data: serde_json::to_value(&val)?,
            format_version: None,
        };
        diesel::insert_into(event_datas::table)
            .values(&event_data)
            .on_conflict((event_datas::event_id, event_datas::event_sn))
            .do_update()
            .set(&event_data)
            .execute(&mut connect()?)
            .unwrap();

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
    // Skip the PDU if we already have it as a timeline event
    if crate::room::timeline::has_non_outlier_pdu(&incoming_pdu.event_id)? {
        return Ok(());
    }

    if crate::room::pdu_metadata::is_event_soft_failed(&incoming_pdu.event_id)? {
        return Err(MatrixError::invalid_param("Event has been soft failed").into());
    }

    info!("Upgrading {} to timeline pdu", incoming_pdu.event_id);
    let timer = Instant::now();
    let room_version_id = &crate::room::room_version(room_id)?;
    let room_version = RoomVersion::new(&room_version_id).expect("room version is supported");

    // 10. Fetch missing state and auth chain events by calling /state_ids at backwards extremities
    //     doing all the checks in this list starting at 1. These are not timeline events.
    debug!("Resolving state at event");

    let state_at_incoming_event = if incoming_pdu.prev_events.len() == 1 {
        state_at_incoming_degree_one(incoming_pdu).await?
    } else {
        state_at_incoming_resolved(incoming_pdu, room_id, room_version_id).await?
    };

    let state_at_incoming_event = match state_at_incoming_event {
        None => fetch_state(origin, room_id, &room_version_id, &incoming_pdu.event_id)
            .await?
            .unwrap_or_default(),
        Some(state) => state,
    };

    debug!("Performing auth check");
    // 11. Check the auth of the event passes based on the state of the event
    event_auth::auth_check(
        &room_version,
        &incoming_pdu,
        None::<PduEvent>, // TODO: third party invite
        |k, s| {
            crate::room::state::ensure_field_id(&k.to_string().into(), s)
                .ok()
                .and_then(|state_key_id| state_at_incoming_event.get(&state_key_id))
                .and_then(|event_id| crate::room::timeline::get_pdu(event_id).ok())
        },
    )?;

    debug!("Auth check succeeded");

    debug!("Gathering auth events");
    let auth_events = crate::room::state::get_auth_events(
        room_id,
        &incoming_pdu.event_ty,
        &incoming_pdu.sender,
        incoming_pdu.state_key.as_deref(),
        &incoming_pdu.content,
    )?;

    event_auth::auth_check(&room_version, &incoming_pdu, None::<PduEvent>, |k, s| {
        auth_events.get(&(k.clone(), s.to_owned()))
    })?;

    // Soft fail check before doing state res
    debug!("Performing soft-fail check");
    let soft_fail = match incoming_pdu.redacts_id(&room_version_id) {
        None => false,
        Some(redact_id) => {
            !crate::room::state::user_can_redact(&redact_id, &incoming_pdu.sender, &incoming_pdu.room_id, true).await?
        }
    };

    // 13. Use state resolution to find new room state

    // We start looking at current room state now, so lets lock the room
    // Now we calculate the set of extremities this room has after the incoming event has been
    // applied. We start with the previous extremities (aka leaves)
    debug!("Calculating extremities");
    let mut extremities: BTreeSet<_> = crate::room::state::get_forward_extremities(room_id)?
        .into_iter()
        .collect();

    // Remove any forward extremities that are referenced by this incoming event's prev_events
    for prev_event in &incoming_pdu.prev_events {
        if extremities.contains(prev_event) {
            extremities.remove(prev_event);
        }
    }

    // // Only keep those extremities were not referenced yet
    // extremities.retain(|id| !matches!(crate::room::pdu_metadata::is_event_referenced(room_id, id), Ok(true)));

    debug!("Compressing state at event");
    let compressed_state_ids = Arc::new(
        state_at_incoming_event
            .iter()
            .map(|(field_id, event_id)| {
                crate::room::state::compress_event(
                    room_id,
                    *field_id,
                    crate::event::ensure_event_sn(room_id, event_id)?,
                )
            })
            .collect::<AppResult<_>>()?,
    );

    if incoming_pdu.state_key.is_some() {
        debug!("Preparing for stateres to derive new room state");

        // We also add state after incoming event to the fork states
        let mut state_after = state_at_incoming_event.clone();

        if let Some(state_key) = &incoming_pdu.state_key {
            let state_key_id =
                crate::room::state::ensure_field_id(&incoming_pdu.event_ty.to_string().into(), state_key)?;

            state_after.insert(state_key_id, Arc::from(&*incoming_pdu.event_id));
        }

        let new_room_state = resolve_state(room_id, room_version_id, state_after)?;

        // Set the new room state to the resolved state
        debug!("Forcing new room state");

        let DeltaInfo {
            frame_id,
            appended,
            disposed,
        } = crate::room::state::save_state(room_id, new_room_state)?;

        crate::room::state::force_state(room_id, frame_id, appended, disposed)?;
    }

    // 14. Check if the event passes auth based on the "current state" of the room, if not soft fail it
    debug!("Starting soft fail auth check");

    if soft_fail {
        let extremities = extremities.iter().map(Borrow::borrow);
        crate::room::timeline::append_incoming_pdu(&incoming_pdu, val, extremities, compressed_state_ids, soft_fail)?;

        // Soft fail, we keep the event as an outlier but don't add it to the timeline
        warn!("Event was soft failed: {:?}", incoming_pdu);
        crate::room::pdu_metadata::mark_event_soft_failed(&incoming_pdu.event_id)?;
        return Err(MatrixError::invalid_param("Event has been soft failed").into());
    }

    // Now that the event has passed all auth it is added into the timeline.
    // We use the `state_at_event` instead of `state_after` so we accurately
    // represent the state for this event.
    let extremities = extremities
        .iter()
        .map(Borrow::borrow)
        .chain(once(incoming_pdu.event_id.borrow()));
    debug!("Appended incoming pdu");
    let pdu_id =
        crate::room::timeline::append_incoming_pdu(&incoming_pdu, val, extremities, compressed_state_ids, soft_fail)?;

    // Event has passed all auth/stateres checks
    // drop(state_lock);
    Ok(())
}

fn resolve_state(
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    incoming_state: HashMap<i64, Arc<EventId>>,
) -> AppResult<Arc<CompressedState>> {
    debug!("Loading current room state ids");

    let current_state_ids = if let Ok(current_frame_id) = crate::room::state::get_room_frame_id(room_id, None) {
        crate::room::state::get_full_state_ids(current_frame_id)?
    } else {
        HashMap::new()
    };

    debug!("Loading fork states");
    let fork_states = [current_state_ids, incoming_state];

    let mut auth_chain_sets = Vec::new();
    for state in &fork_states {
        auth_chain_sets.push(crate::room::auth_chain::get_auth_chain_ids(
            room_id,
            state.values().map(|e| &**e),
        )?);
    }

    let fork_states: Vec<_> = fork_states
        .into_iter()
        .map(|map| {
            map.into_iter()
                .filter_map(|(k, event_id)| {
                    crate::room::state::get_field(k)
                        .map(
                            |DbRoomStateField {
                                 event_ty, state_key, ..
                             }| { ((event_ty.to_string().into(), state_key), event_id) },
                        )
                        .ok()
                })
                .collect::<StateMap<_>>()
        })
        .collect();
    debug!("Resolving state");

    // let lock = crate::STATERES_MUTEX.lock;
    let state = match crate::core::state::resolve(
        room_version_id,
        &fork_states,
        auth_chain_sets
            .iter()
            .map(|set| set.iter().map(|id| Arc::from(&**id)).collect::<HashSet<_>>())
            .collect::<Vec<_>>(),
        |id| match crate::room::timeline::get_pdu(id) {
            Err(e) => {
                error!("LOOK AT ME Failed to fetch event: {}", e);
                None
            }
            Ok(pdu) => Some(pdu),
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
            let event_sn = crate::event::ensure_event_sn(room_id, &event_id)?;
            crate::room::state::compress_event(room_id, state_key_id, event_sn)
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

    let mut events_with_auth_events = Vec::with_capacity(events.len());
    for id in events {
        // a. Look in the main timeline (pduid_pdu tree)
        // b. Look at outlier pdu tree
        // (get_pdu_json checks both)
        if let Ok(local_pdu) = crate::room::timeline::get_pdu(id) {
            trace!("Found {} in db", id);
            events_with_auth_events.push((id, Some(local_pdu), vec![]));
            continue;
        }

        // c. Ask origin server over federation
        // We also handle its auth chain here so we don't get a stack overflow in
        // handle_outlier_pdu.
        let mut todo_auth_events: VecDeque<_> = [Arc::clone(id)].into();
        let mut events_in_reverse_order = Vec::new();
        let mut events_all = HashSet::new();
        while let Some(next_id) = todo_auth_events.pop_front() {
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

            if crate::room::timeline::has_pdu(&next_id).unwrap_or(false) {
                trace!("Found {} in db", next_id);
                continue;
            }

            info!("Fetching {} over federation.", next_id);
            let request = event_request(&origin.origin().await, EventReqArgs::new(next_id.clone()))?.into_inner();

            match crate::sending::send_federation_request(&origin, request)
                .await?
                .json::<EventResBody>()
                .await
            {
                Ok(res) => {
                    info!("Got {} over federation", next_id);

                    let Ok((calculated_event_id, value)) =
                        crate::event::gen_event_id_canonical_json(&res.pdu, room_version_id)
                    else {
                        back_off((*next_id).to_owned());
                        continue;
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
                                todo_auth_events.push_back(a);
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
        events_with_auth_events.push((id, None, events_in_reverse_order));
    }

    let mut pdus = Vec::with_capacity(events_with_auth_events.len());
    for (id, local_pdu, events_in_reverse_order) in events_with_auth_events {
        // a. Look in the main timeline (pduid_pdu tree)
        // b. Look at outlier pdu tree
        // (get_pdu_json checks both)
        if let Some(local_pdu) = local_pdu {
            trace!("Found {id} in db");
            pdus.push((local_pdu.clone(), None));
        }
        for (next_id, value) in events_in_reverse_order.into_iter().rev() {
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

            if let Ok(pdu) = crate::room::timeline::get_pdu(&next_id) {
                pdus.push((pdu, Some(value)));
                continue;
            }
            match handle_outlier_pdu(origin, &next_id, room_id, room_version_id, value.clone(), true).await {
                Ok((pdu, json)) => {
                    if next_id == *id {
                        pdus.push((pdu, Some(json)));
                    }
                }
                Err(e) => {
                    warn!("Authentication of event {} failed: {:?}", next_id, e);
                    back_off((*next_id).to_owned());
                }
            }
        }
    }
    Ok(pdus)
}

pub async fn fetch_missing_prev_events(
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
    let mut event_info = HashMap::new();
    let mut todo_outlier_stack: VecDeque<Arc<EventId>> = initial_set.into();
    let mut amount = 0;
    let room_version_id = &crate::room::room_version(room_id)?;

    let first_pdu_in_room = crate::room::timeline::first_pdu_in_room(room_id)?
        .ok_or_else(|| AppError::internal("Failed to find first pdu in database."))?;

    while let Some(prev_event_id) = todo_outlier_stack.pop_front() {
        if let Some((pdu, mut json_opt)) =
            fetch_and_handle_outliers(origin, &[prev_event_id.clone()], room_id, room_version_id)
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

            if json_opt.is_none() {
                json_opt = crate::room::timeline::get_pdu_json(&prev_event_id).ok().flatten();
            }

            if let Some(json) = json_opt {
                if pdu.origin_server_ts > first_pdu_in_room.origin_server_ts {
                    amount = amount.saturating_add(1);
                    for prev_prev in &pdu.prev_events {
                        if !graph.contains_key(prev_prev) {
                            todo_outlier_stack.push_back(prev_prev.clone());
                        }
                    }

                    graph.insert(prev_event_id.clone(), pdu.prev_events.iter().cloned().collect());
                } else {
                    // Time based check failed
                    graph.insert(prev_event_id.clone(), HashSet::new());
                }

                event_info.insert(prev_event_id.clone(), (Arc::new(pdu), json));
            } else {
                // Get json failed, so this was not fetched over federation
                graph.insert(prev_event_id.clone(), HashSet::new());
            }
        } else {
            // Fetch and handle failed
            graph.insert(prev_event_id.clone(), HashSet::new());
        }
    }

    let sorted = crate::room::state::lexicographical_topological_sort(&graph, &|event_id| {
        // This return value is the key used for sorting events,
        // events are then sorted by power level, time,
        // and lexically by event_id.
        futures_util::future::ok((
            0,
            event_info
                .get(&event_id)
                .map_or_else(|| UnixMillis(0), |info| info.0.origin_server_ts),
        ))
    })
    .await
    .map_err(|_| AppError::internal("Error sorting prev events"))?;

    Ok((sorted, event_info))
}

/// Returns Ok if the acl allows the server
pub fn acl_check(server_name: &ServerName, room_id: &RoomId) -> AppResult<()> {
    let acl_event = match crate::room::state::get_room_state(room_id, &StateEventType::RoomServerAcl, "", None) {
        Ok(acl) => acl,
        Err(_) => return Ok(()),
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
        Err(MatrixError::forbidden("Server was denied by room ACL", None).into())
    }
}

fn check_room_id(room_id: &RoomId, pdu: &PduEvent) -> AppResult<()> {
    if pdu.room_id != room_id {
        warn!("Found event from room {} in room {}", pdu.room_id, room_id);
        return Err(MatrixError::invalid_param("Event has wrong room id").into());
    }
    Ok(())
}
