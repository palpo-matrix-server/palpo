use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque, hash_map};
use std::future::Future;
use std::iter::once;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use diesel::prelude::*;
use indexmap::IndexMap;

use super::fetching::{
    fetch_and_process_auth_chain, fetch_and_process_missing_events,
    fetch_and_process_missing_state, fetch_and_process_missing_state_by_ids, fetch_state_ids,
};
use super::resolver::state_at_incoming_resolved;
use crate::core::events::StateEventType;
use crate::core::events::TimelineEventType;
use crate::core::events::room::server_acl::RoomServerAclEventContent;
use crate::core::federation::authorization::{
    EventAuthorizationResBody, event_authorization_request,
};
use crate::core::federation::event::RoomStateIdsResBody;
use crate::core::federation::event::{
    EventReqArgs, EventResBody, MissingEventsReqBody, MissingEventsResBody, event_request,
    missing_events_request,
};
use crate::core::identifiers::*;
use crate::core::room_version_rules::StateResolutionV2Rules;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, JsonValue, canonical_json};
use crate::core::signatures::Verified;
use crate::core::state::{Event, StateError, StateMap, event_auth};
use crate::core::{self, Seqnum, UnixMillis};
use crate::data::room::{DbEvent, DbEventData, NewDbEvent};
use crate::data::schema::*;
use crate::data::{self, connect, diesel_exists};
use crate::event::{PduEvent, SnPduEvent, ensure_event_sn, handler};
use crate::room::state::{CompressedState, DbRoomStateField, DeltaInfo};
use crate::room::{state, timeline};
use crate::utils::SeqnumQueueGuard;
use crate::{AppError, AppResult, MatrixError, exts::*, room};

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
#[tracing::instrument(skip_all)]
pub(crate) async fn process_incoming_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    value: BTreeMap<String, CanonicalJsonValue>,
    is_timeline_event: bool,
) -> AppResult<()> {
    if !crate::room::room_exists(room_id)? {
        return Err(MatrixError::not_found("room is unknown to this server").into());
    }

    let event = events::table
        .filter(events::id.eq(event_id))
        .first::<DbEvent>(&mut connect()?);
    if let Ok(event) = event {
        if !event.is_outlier {
            return Ok(());
        }
        if event.is_rejected || event.soft_failed {
            diesel::delete(&event).execute(&mut connect()?).ok();
            diesel::delete(event_points::table.filter(event_points::event_id.eq(event_id)))
                .execute(&mut connect()?)
                .ok();
            diesel::delete(event_datas::table.filter(event_datas::event_id.eq(event_id)))
                .execute(&mut connect()?)
                .ok();
        }
    }

    // 1.2 Check if the room is disabled
    if crate::room::is_disabled(room_id)? {
        return Err(MatrixError::forbidden(
            "federation of this room is currently disabled on this server",
            None,
        )
        .into());
    }

    // 1.3.1 Check room ACL on origin field/server
    handler::acl_check(origin, room_id)?;

    // 1.3.2 Check room ACL on sender's server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::invalid_param("pdu does not have a valid sender key: {e}"))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("user id in sender is invalid."))?;

    if sender.server_name().ne(origin) {
        handler::acl_check(sender.server_name(), room_id)?;
    }

    // 1. Skip the PDU if we already have it as a timeline event
    if state::get_pdu_frame_id(event_id).is_ok() {
        return Ok(());
    }

    let Some((mut incoming_pdu, val, event_guard)) =
        process_to_outlier_pdu(origin, event_id, room_id, room_version_id, value).await?
    else {
        return Ok(());
    };

    if let Err(e) = process_pdu_missing_deps(
        &mut incoming_pdu,
        origin,
        room_id,
        room_version_id,
        true,
        true,
    )
    .await
    {
        error!("error processing missing deps for {}: {}", event_id, e);
    }

    if incoming_pdu.is_rejected {
        return Ok(());
    }
    check_room_id(room_id, &incoming_pdu)?;

    // 8. if not timeline event: stop
    if !is_timeline_event {
        return Ok(());
    }

    // Skip old events
    let first_pdu_in_room = timeline::first_pdu_in_room(room_id)?
        .ok_or_else(|| AppError::internal("failed to find first pdu in database."))?;
    if incoming_pdu.origin_server_ts < first_pdu_in_room.origin_server_ts {
        return Ok(());
    }

    // Done with prev events, now handling the incoming event
    let start_time = Instant::now();
    crate::ROOM_ID_FEDERATION_HANDLE_TIME
        .write()
        .unwrap()
        .insert(room_id.to_owned(), (event_id.to_owned(), start_time));
    handler::process_to_timeline_pdu(incoming_pdu, val, origin, room_id, true).await?;
    drop(event_guard);
    crate::ROOM_ID_FEDERATION_HANDLE_TIME
        .write()
        .unwrap()
        .remove(&room_id.to_owned());
    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn process_pulled_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    value: BTreeMap<String, CanonicalJsonValue>,
    known_events: &mut HashSet<OwnedEventId>,
) -> AppResult<()> {
    // 1.3.1 Check room ACL on origin field/server
    handler::acl_check(origin, room_id)?;

    // 1.3.2 Check room ACL on sender's server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::invalid_param("pdu does not have a valid sender key: {e}"))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("user id in sender is invalid"))?;

    if sender.server_name().ne(origin) {
        handler::acl_check(sender.server_name(), room_id)?;
    }

    // 1. Skip the PDU if we already have it as a timeline event
    if state::get_pdu_frame_id(event_id).is_ok() {
        return Ok(());
    }

    let Some((pdu, json_data, guard)) =
        process_to_outlier_pdu(origin, event_id, room_id, room_version_id, value).await?
    else {
        return Ok(());
    };

    let mut event_ids = pdu.prev_events.clone();
    event_ids.extend(pdu.auth_events.clone());
    let missing = diesel_exists!(
        event_points::table
            .filter(event_points::event_id.eq_any(&event_ids))
            .filter(event_points::frame_id.is_null()),
        &mut connect()?
    )?;
    if missing {
        return Ok(());
    }

    process_to_timeline_pdu(pdu, json_data, origin, room_id, false).await
}

#[tracing::instrument(skip_all)]
pub async fn process_pdu_missing_deps(
    incoming_pdu: &mut SnPduEvent,
    origin: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    fetch_missing_prev_events: bool,
    fetch_state_for_missing: bool,
) -> AppResult<()> {
    let version_rules = crate::room::get_version_rules(room_version_id)?;
    let auth_rules = &version_rules.authorization;

    let mut soft_failed = false;
    let mut rejection_reason = None;
    let mut known_events = HashSet::new();
    // 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
    if let Err(e) = fetch_and_process_missing_prev_events(
        origin,
        room_id,
        room_version_id,
        incoming_pdu,
        &mut known_events,
    )
    .await
    {
        if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
            if *kind == core::error::ErrorKind::BadJson {
                rejection_reason = Some(format!("failed to bad prev events: {}", e));
            } else {
                soft_failed = true;
            }
        } else {
            soft_failed = true;
        }
    }

    let (_auth_events, missing_auth_event_ids) =
        match timeline::get_may_missing_pdus(room_id, &incoming_pdu.auth_events) {
            Ok(s) => s,
            Err(e) => {
                info!(
                    "error getting auth events for {}: {}",
                    incoming_pdu.event_id, e
                );
                soft_failed = true;
                (vec![], vec![])
            }
        };

    if !missing_auth_event_ids.is_empty() {
        if fetch_state_for_missing {
            println!("======missing_auth_event_ids: {:?}", missing_auth_event_ids);
            if let Err(_e) = fetch_and_process_missing_state_by_ids(
                origin,
                room_id,
                room_version_id,
                &incoming_pdu.event_id,
            )
            .await
            {
                soft_failed = true;
            }
        } else {
            println!("DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDdetch_and_process_auth_chain");
            if let Err(_e) =
                fetch_and_process_auth_chain(origin, room_id, &incoming_pdu.event_id).await
            {
                soft_failed = true;
            }
        }
    }
    let (auth_events, missing_auth_event_ids) =
        timeline::get_may_missing_pdus(room_id, &incoming_pdu.auth_events)?;
    if !missing_auth_event_ids.is_empty() {
        warn!(
            "missing auth events for {}: {:?}",
            incoming_pdu.event_id, missing_auth_event_ids
        );
        soft_failed = true;
    } else {
        let rejected_auth_events = auth_events
            .iter()
            .filter_map(|pdu| {
                if pdu.is_rejected {
                    Some(pdu.event_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if !rejected_auth_events.is_empty() {
            rejection_reason = Some(format!(
                "event's auth events rejected: {rejected_auth_events:?}"
            ))
        }
    }

    let auth_events = auth_events
        .into_iter()
        .map(|auth_event| {
            (
                (
                    auth_event.event_ty.to_string().into(),
                    auth_event.state_key.clone().unwrap_or_default(),
                ),
                auth_event,
            )
        })
        .collect::<HashMap<_, _>>();

    // The original create event must be in the auth events
    if !matches!(
        auth_events.get(&(StateEventType::RoomCreate, "".to_owned())),
        Some(_) | None
    ) {
        rejection_reason = Some(format!("incoming event refers to wrong create event"));
    }

    if let Err(_e) = event_auth::auth_check(
            &auth_rules,
            incoming_pdu,
            &async |event_id| {
                timeline::get_pdu(&event_id)
                    .map_err(|_| StateError::other("missing pdu 1"))
            },
            &async |k, s| {
                if let Some(pdu) = auth_events
                    .get(&(k.to_string().into(), s.to_owned()))
                {
                    return Ok(pdu.to_owned());
                }
                if auth_rules.room_create_event_id_as_room_id && k == StateEventType::RoomCreate {
                    let pdu = crate::room::get_create(room_id)
                        .map_err(|_| StateError::other("missing create event"))?
                        .into_inner();
                    if pdu.room_id != *room_id {
                        Err(StateError::other("mismatched room id in create event"))
                    } else {
                        Ok(pdu)
                    }
                } else {
                    Err(StateError::other(format!(
                        "failed auth check when process to outlier pdu, missing state event, event_type: {k}, state_key:{s}"
                    )))
                }
            },
        )
        .await
            && rejection_reason.is_none()
        {
            soft_failed = true;
            // rejection_reason = Some(e.to_string())
        };

    debug!("validation successful");

    diesel::update(events::table.filter(events::sn.eq(incoming_pdu.event_sn)))
        .set((
            events::soft_failed.eq(soft_failed),
            events::is_rejected.eq(rejection_reason.is_some()),
            events::rejection_reason.eq(rejection_reason.clone()),
        ))
        .execute(&mut connect()?)?;

    let existed_events = events::table
        .filter(events::id.eq_any(&incoming_pdu.prev_events))
        .select(events::id)
        .load::<OwnedEventId>(&mut connect()?)?;
    let missing_events = incoming_pdu
        .prev_events
        .iter()
        .filter(|id| !known_events.contains(*id) && !existed_events.contains(id))
        .collect::<Vec<_>>();
    if !missing_events.is_empty() {
        data::room::add_timeline_gap(&incoming_pdu.room_id, incoming_pdu.event_sn)?;
    }

    incoming_pdu.soft_failed = soft_failed;
    incoming_pdu.is_rejected = rejection_reason.is_some();
    incoming_pdu.rejection_reason = rejection_reason;
    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn process_to_outlier_pdu(
    origin: &ServerName,
    event_id: &EventId,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    mut value: CanonicalJsonObject,
) -> AppResult<Option<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)>> {
    if let Some((room_id, event_sn, event_data)) = event_datas::table
        .filter(event_datas::event_id.eq(event_id))
        .select((
            event_datas::room_id,
            event_datas::event_sn,
            event_datas::json_data,
        ))
        .first::<(OwnedRoomId, Seqnum, JsonValue)>(&mut connect()?)
        .optional()?
        && let Ok(val) = serde_json::from_value::<CanonicalJsonObject>(event_data.clone())
    {
        return Ok(Some((
            SnPduEvent::from_json_value(&room_id, event_id, event_sn, event_data)?,
            val,
            None,
        )));
    }

    // 1.1. Remove unsigned field
    value.remove("unsigned");

    let version_rules = crate::room::get_version_rules(room_version_id)?;
    let auth_rules = &version_rules.authorization;
    let origin_server_ts = value.get("origin_server_ts").ok_or_else(|| {
        error!("invalid pdu, no origin_server_ts field");
        MatrixError::missing_param("invalid pdu, no origin_server_ts field")
    })?;

    let _origin_server_ts = {
        let ts = origin_server_ts
            .as_integer()
            .ok_or_else(|| MatrixError::invalid_param("origin_server_ts must be an integer"))?;

        UnixMillis(
            ts.try_into()
                .map_err(|_| MatrixError::invalid_param("time must be after the unix epoch"))?,
        )
    };
    let mut val = match crate::server_key::verify_event(&value, Some(room_version_id)).await {
        Ok(Verified::Signatures) => {
            // Redact
            warn!("calculated hash does not match: {}", event_id);
            let obj = match canonical_json::redact(value, &version_rules.redaction, None) {
                Ok(obj) => obj,
                Err(_) => return Err(MatrixError::invalid_param("redaction failed").into()),
            };

            // Skip the PDU if it is redacted and we already have it as an outlier event
            if timeline::get_pdu_json(event_id)?.is_some() {
                return Err(MatrixError::invalid_param(
                    "event was redacted and we already knew about it",
                )
                .into());
            }

            obj
        }
        Ok(Verified::All) => value,
        Err(e) => {
            warn!("dropping bad event {}: {}  {value:#?}", event_id, e,);
            return Err(MatrixError::invalid_param("signature verification failed").into());
        }
    };

    // Now that we have checked the signature and hashes we can add the eventID and convert
    // to our PduEvent type
    val.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(event_id.as_str().to_owned()),
    );
    let mut incoming_pdu = PduEvent::from_json_value(
        room_id,
        event_id,
        serde_json::to_value(&val).expect("`CanonicalJson` is a valid `JsonValue`"),
    )
    .map_err(|_| AppError::internal("event is not a valid PDU."))?;

    check_room_id(room_id, &incoming_pdu)?;

    let server_joined = crate::room::is_server_joined(crate::config::server_name(), room_id)?;
    if !server_joined {
        if let Some(state_key) = incoming_pdu.state_key.as_deref()
            && incoming_pdu.event_ty == TimelineEventType::RoomMember
            && state_key.ends_with(&*format!(":{}", crate::config::server_name()))
        {
            let (event_sn, event_guard) = ensure_event_sn(room_id, event_id)?;
            let mut db_event: NewDbEvent =
                NewDbEvent::from_canonical_json(&incoming_pdu.event_id, event_sn, &val)?;
            db_event.is_outlier = true;
            db_event.rejection_reason = None;
            db_event.save()?;
            DbEventData {
                event_id: incoming_pdu.event_id.clone(),
                event_sn,
                room_id: incoming_pdu.room_id.clone(),
                internal_metadata: None,
                json_data: serde_json::to_value(&val)?,
                format_version: None,
            }
            .save()?;

            debug!("added pdu as outlier");
            return Ok(Some((
                SnPduEvent::new(incoming_pdu, event_sn),
                val,
                event_guard,
            )));
        }
        return Ok(None);
    }

    let mut soft_failed = false;
    let mut rejection_reason = None;
    let (auth_events, missing_auth_event_ids) =
        timeline::get_may_missing_pdus(room_id, &incoming_pdu.auth_events)?;
    if !missing_auth_event_ids.is_empty() {
        warn!(
            "missing auth events for {}: {:?}",
            incoming_pdu.event_id, missing_auth_event_ids
        );
        soft_failed = true;
    } else {
        let rejected_auth_events = auth_events
            .iter()
            .filter_map(|pdu| {
                if pdu.is_rejected {
                    Some(pdu.event_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if !rejected_auth_events.is_empty() {
            rejection_reason = Some(format!(
                "event's auth events rejected: {rejected_auth_events:?}"
            ))
        }
    }

    let auth_events = auth_events
        .into_iter()
        .map(|auth_event| {
            (
                (
                    auth_event.event_ty.to_string().into(),
                    auth_event.state_key.clone().unwrap_or_default(),
                ),
                auth_event,
            )
        })
        .collect::<HashMap<(StateEventType, _), _>>();

    // // The original create event must be in the auth events
    // if !matches!(
    //     auth_events.get(&(StateEventType::RoomCreate, "".to_owned())),
    //     Some(_) | None
    // ) {
    //     rejection_reason = Some(format!("incoming event refers to wrong create event"));
    // }

    if let Err(_e) = event_auth::auth_check(
            &auth_rules,
            &incoming_pdu,
            &async |event_id| {
                timeline::get_pdu(&event_id)
                    .map(|s| s.pdu)
                    .map_err(|_| StateError::other("missing pdu 1"))
            },
            &async |k, s| {
                if let Some(pdu) = auth_events
                    .get(&(k.to_string().into(), s.to_owned()))
                    .map(|s| s.pdu.clone())
                {
                    return Ok(pdu);
                }
                if auth_rules.room_create_event_id_as_room_id && k == StateEventType::RoomCreate {
                    let pdu = crate::room::get_create(room_id)
                        .map_err(|_| StateError::other("missing create event"))?
                        .into_inner();
                    if pdu.room_id != *room_id {
                        Err(StateError::other("mismatched room id in create event"))
                    } else {
                        Ok(pdu.into_inner())
                    }
                } else {
                    Err(StateError::other(format!(
                        "failed auth check when process to outlier pdu, missing state event, event_type: {k}, state_key:{s}"
                    )))
                }
            },
        )
        .await
            && rejection_reason.is_none()
        {
            soft_failed = true;
            // rejection_reason = Some(e.to_string())
        };

    // 7. Persist the event as an outlier.
    let (event_sn, event_guard) = ensure_event_sn(room_id, event_id)?;
    let mut db_event = NewDbEvent::from_canonical_json(&incoming_pdu.event_id, event_sn, &val)?;
    db_event.is_outlier = true;
    db_event.soft_failed = soft_failed;
    db_event.is_rejected = rejection_reason.is_some();
    db_event.rejection_reason = rejection_reason.clone();
    db_event.save()?;
    DbEventData {
        event_id: incoming_pdu.event_id.clone(),
        event_sn,
        room_id: incoming_pdu.room_id.clone(),
        internal_metadata: None,
        json_data: serde_json::to_value(&val)?,
        format_version: None,
    }
    .save()?;

    incoming_pdu.soft_failed = soft_failed;
    incoming_pdu.is_rejected = rejection_reason.is_some();
    incoming_pdu.rejection_reason = rejection_reason;
    Ok(Some((
        SnPduEvent::new(incoming_pdu, event_sn),
        val,
        event_guard,
    )))
}

#[tracing::instrument(skip(incoming_pdu, json_data))]
pub async fn process_to_timeline_pdu(
    incoming_pdu: SnPduEvent,
    json_data: BTreeMap<String, CanonicalJsonValue>,
    origin: &ServerName,
    room_id: &RoomId,
    fetch_missing: bool,
) -> AppResult<()> {
    // Skip the PDU if we already have it as a timeline event
    if !incoming_pdu.is_outlier {
        return Ok(());
    }

    info!("upgrading {} to timeline pdu", incoming_pdu.event_id);
    let room_version_id = &room::get_version(room_id)?;
    let version_rules = crate::room::get_version_rules(room_version_id)?;
    let auth_rules = &version_rules.authorization;

    // 10. Fetch missing state and auth chain events by calling /state_ids at backwards extremities
    //     doing all the checks in this list starting at 1. These are not timeline events.
    debug!("resolving state at event");
    let server_joined = crate::room::is_server_joined(crate::config::server_name(), room_id)?;
    if !server_joined {
        if let Some(state_key) = incoming_pdu.state_key.as_deref()
            && incoming_pdu.event_ty == TimelineEventType::RoomMember
            && state_key != incoming_pdu.sender().as_str() //????
            && state_key.ends_with(&*format!(":{}", crate::config::server_name()))
        {
            // let state_at_incoming_event = state_at_incoming_degree_one(&incoming_pdu).await?;
            let state_at_incoming_event =
                state_at_incoming_resolved(&incoming_pdu, room_id, &version_rules)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();

            // let state_at_incoming_event = if let Some(state_at_incoming_event) =
            //     state_at_incoming_event
            // {
            //     state_at_incoming_event
            // } else {
            //     fetch_and_process_state(origin, room_id, room_version_id, &incoming_pdu.event_id)
            //         .await?
            //         .state_events
            // };

            // 13. Use state resolution to find new room state
            let state_lock = crate::room::lock_state(room_id).await;
            // Now that the event has passed all auth it is added into the timeline.
            // We use the `state_at_event` instead of `state_after` so we accurately
            // represent the state for this event.
            let event_id = incoming_pdu.event_id.clone();
            debug!("calculating extremities");
            let extremities: BTreeSet<_> = state::get_forward_extremities(room_id)?
                .into_iter()
                .collect();
            let extremities = extremities
                .iter()
                .map(Borrow::borrow)
                .chain(once(event_id.borrow()));
            debug!("compressing state at event");
            let compressed_state_ids = Arc::new(
                state_at_incoming_event
                    .iter()
                    .map(|(field_id, event_id)| {
                        state::compress_event(
                            room_id,
                            *field_id,
                            crate::event::ensure_event_sn(room_id, event_id)?.0,
                        )
                    })
                    .collect::<AppResult<_>>()?,
            );
            debug!("preparing for stateres to derive new room state");

            // We also add state after incoming event to the fork states
            // let mut state_after = state_at_incoming_event.clone();

            let state_key_id =
                state::ensure_field_id(&incoming_pdu.event_ty.to_string().into(), state_key)?;

            let compressed_event =
                state::compress_event(room_id, state_key_id, incoming_pdu.event_sn)?;
            let mut new_room_state = CompressedState::new();
            new_room_state.insert(compressed_event);

            // Set the new room state to the resolved state
            debug!("forcing new room state");
            let DeltaInfo {
                frame_id,
                appended,
                disposed,
            } = state::save_state(room_id, Arc::new(new_room_state))?;

            state::force_state(room_id, frame_id, appended, disposed)?;

            debug!("appended incoming pdu");
            timeline::append_pdu(&incoming_pdu, json_data, extremities, &state_lock).await?;
            state::set_event_state(
                &incoming_pdu.event_id,
                incoming_pdu.event_sn,
                &incoming_pdu.room_id,
                compressed_state_ids,
            )?;
            drop(state_lock);
        }
        return Ok(());
    }

    // let state_at_incoming_event = if incoming_pdu.prev_events.len() == 1 {
    //     state_at_incoming_degree_one(&incoming_pdu).await?
    // } else {
    //     state_at_incoming_resolved(&incoming_pdu, room_id, room_version_id).await?
    // };
    let state_at_incoming_event =
        state_at_incoming_resolved(&incoming_pdu, room_id, &version_rules).await?;
    let state_at_incoming_event = if let Some(state_at_incoming_event) = state_at_incoming_event {
        state_at_incoming_event
    } else {
        fetch_and_process_missing_state(origin, room_id, room_version_id, &incoming_pdu.event_id)
            .await?
            .state_events
    };

    if !state_at_incoming_event.is_empty() {
        debug!("performing auth check");
        // 11. Check the auth of the event passes based on the state of the event
        event_auth::auth_check(
            auth_rules,
            &incoming_pdu,
            &async |event_id| {
                timeline::get_pdu(&event_id)
                    .map_err(|_| StateError::other("missing pdu in auth check event fetch"))
            },
            &async |k, s| {
                let Ok(state_key_id) = state::get_field_id(&k.to_string().into(), &s) else {
                    error!("missing field id for state type: {k}, state_key: {s}");
                    return Err(StateError::other(format!(
                        "missing field id for state type: {k}, state_key: {s}"
                    )));
                };

                match state_at_incoming_event.get(&state_key_id) {
                    Some(event_id) => match timeline::get_pdu(event_id) {
                        Ok(pdu) => Ok(pdu),
                        Err(e) => {
                            error!("failed to get pdu for state resolution: {}", e);
                            Err(StateError::other(format!(
                                "failed to get pdu for state resolution: {}",
                                e
                            )))
                        }
                    },
                    None => {
                        error!(
                            "missing state key id {state_key_id} for state type: {k}, state_key: {s}, room: {room_id}"
                        );
                        Err(StateError::other(format!(
                            "missing state key id {state_key_id} for state type: {k}, state_key: {s}, room: {room_id}"
                        )))
                    }
                }
            },
        )
        .await?;
        debug!("auth check succeeded");
    }

    debug!("gathering auth events");
    let auth_events = state::get_auth_events(
        room_id,
        &incoming_pdu.event_ty,
        &incoming_pdu.sender,
        incoming_pdu.state_key.as_deref(),
        &incoming_pdu.content,
        auth_rules,
    )?;
    event_auth::auth_check(
        auth_rules,
        &incoming_pdu,
        &async |event_id| {
            timeline::get_pdu(&event_id).map_err(|_| StateError::other("missing pdu 3"))
        },
        &async |k, s| {
            if let Some(pdu) = auth_events.get(&(k.clone(), s.to_string())).cloned() {
                return Ok(pdu);
            }
            if auth_rules.room_create_event_id_as_room_id && k == StateEventType::RoomCreate {
                let pdu = crate::room::get_create(room_id)
                    .map_err(|_| StateError::other("missing create event"))?;
                if pdu.room_id != *room_id {
                    Err(StateError::other("mismatched room id in create event"))
                } else {
                    Ok(pdu.into_inner())
                }
            } else {
                Err(StateError::other(format!(
                    "failed auth check when process to timeline, missing state event, event_type: {k}, state_key:{s}"
                )))
            }
        },
    )
    .await?;

    // Soft fail check before doing state res
    debug!("performing soft-fail check");
    let soft_fail = match incoming_pdu.redacts_id(room_version_id) {
        None => false,
        Some(redact_id) => {
            !state::user_can_redact(
                &redact_id,
                &incoming_pdu.sender,
                &incoming_pdu.room_id,
                true,
            )
            .await?
        }
    };

    // 13. Use state resolution to find new room state
    let state_lock = crate::room::lock_state(room_id).await;

    // We start looking at current room state now, so lets lock the room
    // Now we calculate the set of extremities this room has after the incoming event has been
    // applied. We start with the previous extremities (aka leaves)
    debug!("calculating extremities");
    let mut extremities: BTreeSet<_> = state::get_forward_extremities(room_id)?
        .into_iter()
        .collect();

    // Remove any forward extremities that are referenced by this incoming event's prev_events
    for prev_event in &incoming_pdu.prev_events {
        if extremities.contains(prev_event) {
            extremities.remove(prev_event);
        }
    }

    // Only keep those extremities were not referenced yet
    // extremities.retain(|id| !matches!(crate::room::pdu_metadata::is_event_referenced(room_id, id), Ok(true)));

    debug!("compressing state at event");
    let compressed_state_ids = Arc::new(
        state_at_incoming_event
            .iter()
            .map(|(field_id, event_id)| {
                state::compress_event(
                    room_id,
                    *field_id,
                    crate::event::ensure_event_sn(room_id, event_id)?.0,
                )
            })
            .collect::<AppResult<_>>()?,
    );

    let guards = if let Some(state_key) = &incoming_pdu.state_key {
        debug!("preparing for stateres to derive new room state");

        // We also add state after incoming event to the fork states
        let mut state_after = state_at_incoming_event.clone();
        let state_key_id =
            state::ensure_field_id(&incoming_pdu.event_ty.to_string().into(), state_key)?;
        state_after.insert(state_key_id, incoming_pdu.event_id.clone());
        let (new_room_state, guards) = resolve_state(room_id, room_version_id, state_after).await?;

        // Set the new room state to the resolved state
        debug!("Forcing new room state");

        let DeltaInfo {
            frame_id,
            appended,
            disposed,
        } = state::save_state(room_id, new_room_state)?;

        state::force_state(room_id, frame_id, appended, disposed)?;
        guards
    } else {
        vec![]
    };

    // Now that the event has passed all auth it is added into the timeline.
    // We use the `state_at_event` instead of `state_after` so we accurately
    // represent the state for this event.
    let event_id = incoming_pdu.event_id.clone();
    let extremities = extremities
        .iter()
        .map(Borrow::borrow)
        .chain(once(event_id.borrow()));
    // 14. Check if the event passes auth based on the "current state" of the room, if not soft fail it
    if soft_fail {
        debug!("starting soft fail auth check");
        state::set_forward_extremities(&incoming_pdu.room_id, extremities, &state_lock)?;
        // Soft fail, we keep the event as an outlier but don't add it to the timeline
        warn!("event was soft failed: {:?}", incoming_pdu);
        crate::room::pdu_metadata::mark_event_soft_failed(&incoming_pdu.event_id)?;
        return Err(MatrixError::invalid_param("event has been soft failed").into());
    } else {
        debug!("appended incoming pdu");
        timeline::append_pdu(&incoming_pdu, json_data, extremities, &state_lock).await?;
        state::set_event_state(
            &incoming_pdu.event_id,
            incoming_pdu.event_sn,
            &incoming_pdu.room_id,
            compressed_state_ids,
        )?;
    }
    drop(guards);

    // Event has passed all auth/stateres checks
    drop(state_lock);
    Ok(())
}

async fn resolve_state(
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    incoming_state: IndexMap<i64, OwnedEventId>,
) -> AppResult<(Arc<CompressedState>, Vec<SeqnumQueueGuard>)> {
    debug!("loading current room state ids");
    let current_state_ids = if let Ok(current_frame_id) = crate::room::get_frame_id(room_id, None) {
        state::get_full_state_ids(current_frame_id)?
    } else {
        IndexMap::new()
    };

    debug!("loading fork states");
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
                    state::get_field(k)
                        .map(
                            |DbRoomStateField {
                                 event_ty,
                                 state_key,
                                 ..
                             }| {
                                ((event_ty.to_string().into(), state_key), event_id)
                            },
                        )
                        .ok()
                })
                .collect::<StateMap<_>>()
        })
        .collect();
    debug!("resolving state");

    let version_rules = crate::room::get_version_rules(room_version_id)?;
    let state = match crate::core::state::resolve(
        &version_rules.authorization,
        version_rules
            .state_resolution
            .v2_rules()
            .unwrap_or(StateResolutionV2Rules::V2_0),
        &fork_states,
        auth_chain_sets
            .iter()
            .map(|set| set.iter().map(|id| id.to_owned()).collect::<HashSet<_>>())
            .collect::<Vec<_>>(),
        &async |id| timeline::get_pdu(&id).map_err(|_| StateError::other("missing pdu 4")),
        |map| {
            let mut subgraph = HashSet::new();
            for event_ids in map.values() {
                for event_id in event_ids {
                    if let Ok(pdu) = timeline::get_pdu(event_id) {
                        subgraph.extend(pdu.auth_events.iter().cloned());
                        subgraph.extend(pdu.prev_events.iter().cloned());
                    }
                }
            }
            let subgraph = events::table
                .filter(events::id.eq_any(subgraph))
                .filter(events::state_key.is_not_null())
                .select(events::id)
                .load::<OwnedEventId>(&mut connect().unwrap())
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>();
            Some(subgraph)
        },
    )
    .await
    {
        Ok(new_state) => new_state,
        Err(e) => {
            error!("state resolution failed: {}", e);
            return Err(AppError::internal(
                "state resolution failed, either an event could not be found or deserialization",
            ));
        }
    };

    debug!("state resolution done, compressing state");

    let mut new_room_state = BTreeSet::new();
    let mut guards = Vec::new();
    for ((event_type, state_key), event_id) in state {
        let state_key_id = state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
        let (event_sn, guard) = crate::event::ensure_event_sn(room_id, &event_id)?;
        if let Some(guard) = guard {
            guards.push(guard);
        }
        new_room_state.insert(state::compress_event(room_id, state_key_id, event_sn)?);
    }

    Ok((Arc::new(new_room_state), guards))
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
pub(crate) async fn fetch_and_process_outliers(
    origin: &ServerName,
    events: &[OwnedEventId],
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
) -> AppResult<
    Vec<(
        SnPduEvent,
        Option<CanonicalJsonObject>,
        Option<SeqnumQueueGuard>,
    )>,
> {
    let back_off = |id| match crate::BAD_EVENT_RATE_LIMITER.write().unwrap().entry(id) {
        hash_map::Entry::Vacant(e) => {
            e.insert((Instant::now(), 1));
        }
        hash_map::Entry::Occupied(mut e) => *e.get_mut() = (Instant::now(), e.get().1 + 1),
    };

    let mut events_with_auth_events = Vec::with_capacity(events.len());
    for event_id in events {
        // a. Look in the main timeline (pduid_pdu tree)
        // b. Look at outlier pdu tree (get_pdu_json checks both)
        if let Ok(local_pdu) = timeline::get_pdu(event_id) {
            trace!("found {} in db", event_id);
            events_with_auth_events.push((event_id, Some(local_pdu), vec![]));
            continue;
        }

        // c. Ask origin server over federation
        // We also handle its auth chain here so we don't get a stack overflow in process_to_outlier_pdu.
        let mut todo_auth_events: VecDeque<_> = [event_id.clone()].into();
        let mut events_in_reverse_order = Vec::new();
        let mut events_all = HashSet::new();
        while let Some(next_id) = todo_auth_events.pop_front() {
            if let Some((time, tries)) =
                crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&*next_id)
            {
                // Exponential backoff
                let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
                if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                    min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
                }

                if time.elapsed() < min_elapsed_duration {
                    info!("backing off from {}", next_id);
                    continue;
                }
            }

            if events_all.contains(&next_id) {
                continue;
            }

            if timeline::has_pdu(&next_id) {
                trace!("found {} in db", next_id);
                continue;
            }

            info!("fetching event {} over federation", next_id);
            let request =
                event_request(&origin.origin().await, EventReqArgs::new(next_id.clone()))?
                    .into_inner();

            let response =
                match crate::sending::send_federation_request(origin, request, None).await {
                    Ok(res) => res,
                    Err(e) => {
                        warn!("failed to fetch event {}: {}", next_id, e);
                        continue;
                    }
                };
            match response.json::<EventResBody>().await {
                Ok(res) => {
                    info!("got event {} over federation", next_id);

                    let Ok((calculated_event_id, value)) =
                        crate::event::gen_event_id_canonical_json(&res.pdu, room_version_id)
                    else {
                        back_off((*next_id).to_owned());
                        continue;
                    };

                    if calculated_event_id != *next_id {
                        warn!(
                            "server didn't return event id we requested: requested: {}, we got {}. Event: {:?}",
                            next_id, calculated_event_id, &res.pdu
                        );
                    }

                    if let Some(auth_events) = value.get("auth_events").and_then(|c| c.as_array()) {
                        for auth_event in auth_events {
                            if let Ok(auth_event) =
                                serde_json::from_value(auth_event.clone().into())
                            {
                                let a: OwnedEventId = auth_event;
                                todo_auth_events.push_back(a);
                            } else {
                                warn!("auth event id is not valid");
                            }
                        }
                    } else {
                        warn!("auth event list invalid");
                    }

                    events_in_reverse_order.push((next_id.clone(), value));
                    events_all.insert(next_id);
                }
                Err(_) => {
                    warn!("failed to fetch event: {}", next_id);
                    back_off((*next_id).to_owned());
                }
            }
        }
        events_with_auth_events.push((event_id, None, events_in_reverse_order));
    }

    let mut pdus = Vec::with_capacity(events_with_auth_events.len());
    for (event_id, local_pdu, events_in_reverse_order) in events_with_auth_events {
        // a. Look in the main timeline (pduid_pdu tree)
        // b. Look at outlier pdu tree (get_pdu_json checks both)
        if let Some(local_pdu) = local_pdu {
            trace!("found {event_id} in db");
            pdus.push((local_pdu.clone(), None, None));
        }
        for (next_id, value) in events_in_reverse_order.into_iter().rev() {
            if let Some((time, tries)) =
                crate::BAD_EVENT_RATE_LIMITER.read().unwrap().get(&*next_id)
            {
                // Exponential backoff
                let mut min_elapsed_duration = Duration::from_secs(5 * 60) * (*tries) * (*tries);
                if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                    min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
                }

                if time.elapsed() < min_elapsed_duration {
                    info!("backing off from {}", next_id);
                    continue;
                }
            }

            if let Ok(pdu) = timeline::get_pdu(&next_id) {
                pdus.push((pdu, Some(value), None));
                continue;
            }
            match process_to_outlier_pdu(origin, &next_id, room_id, room_version_id, value).await {
                Ok(Some((pdu, json, guard))) => {
                    if next_id == *event_id {
                        pdus.push((pdu, Some(json), guard));
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("authentication of event {} failed: {:?}", next_id, e);
                    back_off((*next_id).to_owned());
                }
            }
        }
    }
    Ok(pdus)
}

pub async fn fetch_and_process_missing_prev_events(
    origin: &ServerName,
    room_id: &RoomId,
    _room_version_id: &RoomVersionId,
    incoming_pdu: &PduEvent,
    known_events: &mut HashSet<OwnedEventId>,
) -> AppResult<()> {
    let room_version_id = &room::get_version(room_id)?;

    let min_depth = timeline::first_pdu_in_room(room_id)
        .ok()
        .and_then(|pdu| pdu.map(|p| p.depth))
        .unwrap_or(0);
    let forward_extremities = room::state::get_forward_extremities(room_id)?;
    let mut fetched_events = IndexMap::with_capacity(10);

    let mut earliest_events = forward_extremities.clone();
    earliest_events.extend(known_events.iter().cloned());

    let mut missing_events = Vec::with_capacity(incoming_pdu.prev_events.len());
    for prev_id in &incoming_pdu.prev_events {
        let pdu = timeline::get_pdu(&prev_id);
        if let Ok(pdu) = &pdu
            && !pdu.is_rejected
        {
            known_events.insert(prev_id.to_owned());
        } else if !earliest_events.contains(&prev_id) && !fetched_events.contains_key(prev_id) {
            missing_events.push(prev_id.to_owned());
        }
    }
    if missing_events.is_empty() {
        return Ok(());
    }

    let request = missing_events_request(
        &origin.origin().await,
        room_id,
        MissingEventsReqBody {
            limit: 10,
            min_depth,
            earliest_events,
            latest_events: vec![incoming_pdu.event_id.clone()],
        },
    )?
    .into_inner();

    known_events.insert(incoming_pdu.event_id.clone());
    let response = crate::sending::send_federation_request(origin, request, None).await?;
    let res_body = response.json::<MissingEventsResBody>().await?;

    for event in res_body.events {
        let (event_id, event_val, _room_id, _room_version_id) = crate::parse_incoming_pdu(&event)?;

        if known_events.contains(&event_id) {
            continue;
        }

        if fetched_events.contains_key(&event_id) || timeline::get_pdu(&event_id).is_ok() {
            known_events.insert(event_id.clone());
            continue;
        }

        let prev_events = event_val
            .get("prev_events")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().and_then(|id| OwnedEventId::try_from(id).ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        fetched_events.insert(event_id.clone(), event_val);
        known_events.insert(event_id.clone());

        if !prev_events.contains(&incoming_pdu.event_id) {
            let prev_events = prev_events
                .into_iter()
                .filter_map(|id| {
                    if !fetched_events.contains_key(&id)
                        && incoming_pdu.event_id != id
                        && !known_events.contains(&id)
                        && !missing_events.contains(&id)
                    {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let exists_events = events::table
                .filter(events::id.eq_any(&prev_events))
                .select(events::id)
                .load::<OwnedEventId>(&mut connect()?)?;
            // missing_events.extend(
            //     prev_events
            //         .into_iter()
            //         .filter(|id| !exists_events.contains(id)),
            // );
        }

        missing_events.retain(|e| e != &event_id);
    }

    for missing_id in missing_events {
        let mut desired_events = HashSet::new();
        if let Ok(RoomStateIdsResBody {
            auth_chain_ids,
            pdu_ids,
        }) = fetch_state_ids(origin, room_id, &missing_id).await
        {
            desired_events.extend(pdu_ids.into_iter());
            desired_events.extend(auth_chain_ids.into_iter());
        }
        desired_events.insert(missing_id.clone());
        let desired_count = desired_events.len();

        let exist_events = events::table
            .filter(events::id.eq_any(&desired_events))
            .select(events::id)
            .load::<OwnedEventId>(&mut connect()?)?;
        known_events.extend(exist_events.iter().cloned());
        let missing_events = desired_events
            .into_iter()
            .filter(|id| !exist_events.contains(id))
            .collect::<Vec<_>>();
        // Same as synapse
        // Making an individual request for each of 1000s of events has a lot of
        // overhead. On the other hand, we don't really want to fetch all of the events
        // if we already have most of them.
        //
        // As an arbitrary heuristic, if we are missing more than 10% of the events, then
        // we fetch the whole state.
        if missing_events.len() * 10 >= desired_count {
            println!("call fetch_and_process_state 1: {}", missing_id);
            fetch_and_process_missing_state(origin, room_id, room_version_id, &missing_id).await?;
        } else {
            fetch_and_process_missing_events(origin, room_id, room_version_id, &missing_events)
                .await?;
        }
        known_events.extend(missing_events.into_iter());
    }

    known_events.insert(incoming_pdu.event_id.clone());
    fetched_events.sort_by(|_x1, v1, _k2, v2| {
        let depth1 = v1.get("depth").and_then(|v| v.as_integer()).unwrap_or(0);
        let depth2 = v2.get("depth").and_then(|v| v.as_integer()).unwrap_or(0);
        depth1.cmp(&depth2)
    });
    for (event_id, event_val) in fetched_events {
        let is_exists = diesel_exists!(
            events::table
                .filter(events::id.eq(&event_id))
                .filter(events::room_id.eq(&room_id)),
            &mut connect()?
        )?;
        if !is_exists {
            println!(
                "=================processing fetched missing prev event: {event_id}  known_events:{known_events:?}"
            );
            if let Err(e) = process_pulled_pdu(
                origin,
                &event_id,
                room_id,
                room_version_id,
                event_val.clone(),
                known_events,
            )
            .await
            {
                error!(
                    "failed to process fetched missing prev event {}: {}",
                    event_id, e
                );
            }
        }
    }

    Ok(())
}

/// Returns Ok if the acl allows the server
pub fn acl_check(server_name: &ServerName, room_id: &RoomId) -> AppResult<()> {
    let acl_event = match room::get_state(room_id, &StateEventType::RoomServerAcl, "", None) {
        Ok(acl) => acl,
        Err(_) => return Ok(()),
    };

    let acl_event_content: RoomServerAclEventContent =
        match acl_event.get_content::<RoomServerAclEventContent>() {
            Ok(content) => content,
            Err(_) => {
                warn!("invalid ACL event");
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
        info!(
            "server {} was denied by room ACL in {}",
            server_name, room_id
        );
        Err(MatrixError::forbidden("server was denied by room ACL", None).into())
    }
}

fn check_room_id(room_id: &RoomId, pdu: &PduEvent) -> AppResult<()> {
    if pdu.room_id != room_id {
        warn!("found event from room {} in room {}", pdu.room_id, room_id);
        return Err(MatrixError::invalid_param("Event has wrong room id").into());
    }
    Ok(())
}
