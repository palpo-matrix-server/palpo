use std::collections::HashSet;

use diesel::prelude::*;
use indexmap::IndexMap;

use crate::core::ServerName;
use crate::core::federation::event::{
    RoomStateAtEventReqArgs, RoomStateIdsResBody, RoomStateReqArgs, RoomStateResBody,
    room_state_ids_request, room_state_request,
};
use crate::core::identifiers::*;
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::event::handler::{process_pulled_pdu, process_to_outlier_pdu};
use crate::room::state::ensure_field_id;
use crate::room::{state, timeline};
use crate::{AppError, AppResult, exts::*};

pub struct FetchedState {
    pub state_events: IndexMap<i64, OwnedEventId>,
    pub auth_events: IndexMap<i64, OwnedEventId>,
}
/// Call /state_ids to find out what the state at this pdu is. We trust the
/// server's response to some extend (sic), but we still do a lot of checks
/// on the events
pub async fn fetch_state(
    origin: &ServerName,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<FetchedState> {
    debug!("fetching state events at event: {event_id}");
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
        println!("====event_val: {:#?}", event_val);
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
        println!("====event_xval: {:#?}", event_val);
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

    println!("====================state_events: {:#?}", state_events);
    println!("====================auth_events: {:#?}", auth_events);

    Ok(FetchedState {
        state_events,
        auth_events,
    })
}

pub async fn fetch_state_ids(
    origin: &ServerName,
    room_id: &RoomId,
    event_id: &EventId,
) -> AppResult<Vec<OwnedEventId>> {
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

    Ok(res_body.pdu_ids)
}
