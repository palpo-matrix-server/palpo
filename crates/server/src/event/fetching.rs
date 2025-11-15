use std::collections::HashSet;

use diesel::prelude::*;
use indexmap::IndexMap;

use crate::core::ServerName;
use crate::core::federation::authorization::{EventAuthResBody, event_auth_request};
use crate::core::federation::event::{
    EventReqArgs, EventResBody, RoomStateAtEventReqArgs, RoomStateIdsResBody, RoomStateReqArgs,
    RoomStateResBody, event_request, room_state_ids_request, room_state_request,
};
use crate::core::identifiers::*;
use crate::data::diesel_exists;
use crate::data::schema::*;
use crate::event::handler::process_to_outlier_pdu;
use crate::event::{connect, parse_fetched_pdu, seen_event_ids};
use crate::room::state::ensure_field_id;
use crate::room::timeline;
use crate::sending::send_federation_request;
use crate::{AppResult, SnPduEvent, exts::*};

pub struct FetchedState {
    pub state_events: IndexMap<i64, OwnedEventId>,
    pub auth_events: IndexMap<i64, OwnedEventId>,
}

pub async fn fetch_and_process_auth_chain(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<Vec<SnPduEvent>> {
    let request =
        event_auth_request(&remote_server.origin().await, room_id, event_id)?.into_inner();
    let res_body = crate::sending::send_federation_request(remote_server, request, None)
        .await?
        .json::<EventAuthResBody>()
        .await?;
    Box::pin(async move {
        let mut auth_events = Vec::new();
        let mut known_events = HashSet::new();
        for event in res_body.auth_chain {
            let (event_id, event_value) =
                crate::event::parse_fetched_pdu(room_id, room_version, &event)?;
            if let Ok(pdu) = timeline::get_pdu(&event_id) {
                auth_events.push(pdu);
                known_events.insert(event_id);
                continue;
            }
            if !diesel_exists!(
                events::table
                    .filter(events::id.eq(&event_id))
                    .filter(events::room_id.eq(&room_id)),
                &mut connect()?
            )? {
                let Some(outlier_pdu) = process_to_outlier_pdu(
                    remote_server,
                    &event_id,
                    &room_id,
                    &room_version,
                    event_value,
                )
                .await?
                else {
                    continue;
                };
                let pdu = outlier_pdu.process_pulled().await?.0;
                auth_events.push(pdu);
                known_events.insert(event_id);
            }
        }
        Ok(auth_events)
    })
    .await
}

/// Call /state_ids to find out what the state at this pdu is. We trust the
/// server's response to some extend (sic), but we still do a lot of checks
/// on the events
pub(super) async fn fetch_and_process_missing_state_by_ids(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<Vec<OwnedEventId>> {
    debug!("calling /state_ids");
    // Call /state_ids to find out what the state at this pdu is. We trust the server's
    // response to some extend, but we still do a lot of checks on the events
    let RoomStateIdsResBody {
        pdu_ids,
        auth_chain_ids,
    } = fetch_state_ids(remote_server, room_id, event_id).await?;
    debug!("fetching state events at event");

    let mut desired_events = pdu_ids;
    desired_events.push(event_id.to_owned());
    desired_events.extend(auth_chain_ids.into_iter());

    let desired_count = desired_events.len();
    let mut failed_missing_events = Vec::new();
    let seen_events = seen_event_ids(room_id, &desired_events)?;
    let missing_events: Vec<_> = desired_events
        .into_iter()
        .filter(|e| !seen_events.contains(e))
        .collect();
    // Same as synapse
    // Making an individual request for each of 1000s of events has a lot of
    // overhead. On the other hand, we don't really want to fetch all of the events
    // if we already have most of them.
    //
    // As an arbitrary heuristic, if we are missing more than 10% of the events, then
    // we fetch the whole state.
    if missing_events.len() * 10 >= desired_count {
        debug!("requesting complete state from remote");
        println!("cccccccccprocess_to_timeline_pdu1");
        fetch_and_process_missing_state(remote_server, room_id, room_version, event_id).await?;
    } else {
        debug!("fetching {} events from remote", missing_events.len());
        println!("================fetch and process missing prev events===================");
        let failed_events =
            fetch_and_process_missing_events(remote_server, room_id, room_version, &missing_events)
                .await?;
        if !failed_events.is_empty() {
            failed_missing_events.extend(failed_events);
        }
    }

    Ok(failed_missing_events)
}

pub async fn fetch_and_process_missing_state(
    origin: &ServerName,
    room_id: &RoomId,
    room_version: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<FetchedState> {
    println!("===========fetch_and_process_missing_state");
    debug!("fetching state events at event: {event_id}");
    let request = room_state_request(
        &origin.origin().await,
        RoomStateReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
    )?
    .into_inner();
    let res_body = send_federation_request(origin, request, None)
        .await?
        .json::<RoomStateResBody>()
        .await?;

    let mut state_events: IndexMap<_, OwnedEventId> = IndexMap::new();
    let mut auth_events: IndexMap<_, OwnedEventId> = IndexMap::new();
    for pdu in &res_body.pdus {
        let (event_id, event_val) = parse_fetched_pdu(room_id, room_version, pdu)?;
        let event_type = match event_val.get("type") {
            Some(v) => v.as_str().unwrap_or(""),
            None => continue,
        };
        let state_key = match event_val.get("state_key") {
            Some(v) => v.as_str().unwrap_or(""),
            None => continue,
        };
        let field_id = ensure_field_id(&event_type.into(), state_key)?;
        state_events.insert(field_id, event_id);
    }

    for event in &res_body.auth_chain {
        let (event_id, event_val) = parse_fetched_pdu(room_id, room_version, event)?;
        let event_type = match event_val.get("type") {
            Some(v) => v.as_str().unwrap_or(""),
            None => continue,
        };
        let state_key = match event_val.get("state_key") {
            Some(v) => v.as_str().unwrap_or(""),
            None => continue,
        };
        let field_id = ensure_field_id(&event_type.into(), state_key)?;
        auth_events.insert(field_id, event_id);
    }

    Ok(FetchedState {
        state_events,
        auth_events,
    })
}

pub struct FetchedStateIds {
    pub state_events: Vec<OwnedEventId>,
    pub auth_events: Vec<OwnedEventId>,
}
pub async fn fetch_state_ids(
    remote_server: &ServerName,
    room_id: &RoomId,
    event_id: &EventId,
) -> AppResult<RoomStateIdsResBody> {
    debug!("calling /state_ids");
    // Call /state_ids to find out what the state at this pdu is. We trust the server's
    // response to some extend, but we still do a lot of checks on the events
    let request = room_state_ids_request(
        &remote_server.origin().await,
        RoomStateAtEventReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
    )?
    .into_inner();
    let res_body = send_federation_request(remote_server, request, None)
        .await?
        .json::<RoomStateIdsResBody>()
        .await?;
    debug!("fetching state events at event: {event_id}");

    Ok(res_body)
}

pub async fn fetch_event(
    remote_server: &ServerName,
    event_id: &EventId,
) -> AppResult<EventResBody> {
    let request =
        event_request(&remote_server.origin().await, EventReqArgs::new(event_id))?.into_inner();

    let body = crate::sending::send_federation_request(remote_server, request, None)
        .await?
        .json::<EventResBody>()
        .await?;
    Ok(body)
}

pub async fn fetch_and_process_missing_events(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_ids: &[OwnedEventId],
) -> AppResult<Vec<OwnedEventId>> {
    let mut done_ids = Vec::new();
    for event_id in event_ids {
        println!("================fetch_and_process_missing_events===================");
        match fetch_and_process_missing_event(remote_server, room_id, room_version_id, event_id)
            .await
        {
            Ok(_) => done_ids.push(event_id.clone()),
            Err(e) => {
                error!("failed to fetch/process event {event_id} : {e}");
            }
        }
    }

    Ok(event_ids
        .into_iter()
        .filter(|e| !done_ids.contains(e))
        .cloned()
        .collect())
}

pub async fn fetch_and_process_missing_event(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<()> {
    let res_body = fetch_event(remote_server, event_id).await?;
    let Some(outlier_pdu) = process_to_outlier_pdu(
        remote_server,
        event_id,
        room_id,
        room_version_id,
        serde_json::from_str(res_body.pdu.get())?,
    )
    .await?
    else {
        return Ok(());
    };
    outlier_pdu.save_to_database()?;
    Ok(())
}
