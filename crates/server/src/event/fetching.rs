use std::collections::HashSet;

use indexmap::IndexMap;

use crate::core::ServerName;
use crate::core::federation::event::{
    EventReqArgs, EventResBody, RoomStateAtEventReqArgs, RoomStateIdsResBody, RoomStateReqArgs,
    RoomStateResBody, event_request, room_state_ids_request, room_state_request,
};
use crate::core::identifiers::*;
use crate::event::handler::{process_pulled_pdu, process_to_outlier_pdu};
use crate::room::state::ensure_field_id;
use crate::{AppResult, exts::*};

pub struct FetchedState {
    pub state_events: IndexMap<i64, OwnedEventId>,
    pub auth_events: IndexMap<i64, OwnedEventId>,
}
/// Call /state_ids to find out what the state at this pdu is. We trust the
/// server's response to some extend (sic), but we still do a lot of checks
/// on the events
pub async fn fetch_and_process_state(
    origin: &ServerName,
    room_id: &RoomId,
    _room_version_id: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<FetchedState> {
    debug!("fetching state events at event: {event_id}");
    println!("fetching state events at event: {event_id}");
    let request = room_state_request(
        &origin.origin().await,
        RoomStateReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
    )?
    .into_inner();
    let res_body = crate::sending::send_federation_request(origin, request, None)
        .await?
        .json::<RoomStateResBody>()
        .await?;

    let mut state_events: IndexMap<_, OwnedEventId> = IndexMap::new();
    let mut auth_events: IndexMap<_, OwnedEventId> = IndexMap::new();
    for pdu in &res_body.pdus {
        let (event_id, event_val, _room_id, _room_version_id) = crate::parse_incoming_pdu(pdu)?;
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
        let (event_id, event_val, _room_id, _room_version_id) = crate::parse_incoming_pdu(event)?;
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

    // // The original create event must still be in the state
    // let create_state_key_id = state::ensure_field_id(&StateEventType::RoomCreate, "")?;

    // if state.get(&create_state_key_id).map(|id| id.as_ref()) != Some(&create_event.event_id) {
    //     return Err(AppError::internal("Incoming event refers to wrong create event."));
    // }

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
    origin: &ServerName,
    room_id: &RoomId,
    event_id: &EventId,
) -> AppResult<RoomStateIdsResBody> {
    debug!("calling /state_ids");
    // Call /state_ids to find out what the state at this pdu is. We trust the server's
    // response to some extend, but we still do a lot of checks on the events
    let request = room_state_ids_request(
        &origin.origin().await,
        RoomStateAtEventReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
    )?
    .into_inner();
    let res_body = crate::sending::send_federation_request(origin, request, None)
        .await?
        .json::<RoomStateIdsResBody>()
        .await?;
    debug!("fetching state events at event: {event_id}");

    Ok(res_body)
}

pub async fn fetch_and_process_events(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_ids: &[OwnedEventId],
) -> AppResult<Vec<OwnedEventId>> {
    println!("fetching events: {event_ids:?}");
    let mut done_ids = Vec::new();
    for event_id in event_ids {
        match fetch_and_process_event(remote_server, room_id, room_version_id, event_id).await {
            Ok(_) => done_ids.push(event_id.clone()),
            Err(e) => {
                error!("failed to fetch/process event {event_id} : {e}");
            }
        }
    }

    Ok(done_ids)
}

pub async fn fetch_and_process_event(
    remote_server: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<()> {
    println!("fetching event: {event_id}");
    let request =
        event_request(&remote_server.origin().await, EventReqArgs::new(event_id))?.into_inner();
    let res_body = crate::sending::send_federation_request(&remote_server, request, None)
        .await?
        .json::<EventResBody>()
        .await?;
    process_to_outlier_pdu(
        &remote_server,
        &event_id,
        room_id,
        &room_version_id,
        serde_json::from_str(res_body.pdu.get())?,
        &mut HashSet::new(),
        false,
        false,
    )
    .await?;
    Ok(())
}
