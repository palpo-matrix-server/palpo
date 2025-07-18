use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::federation::authorization::{EventAuthorizationReqArgs, EventAuthorizationResBody};
use crate::core::federation::event::{
    EventByTimestampReqArgs, EventByTimestampResBody, EventReqArgs, EventResBody, MissingEventsReqBody,
    MissingEventsResBody,
};
use crate::core::identifiers::*;
use crate::data::room::DbEvent;
use crate::room::{state, timeline};
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, config, empty_ok, json_ok};

pub fn router() -> Router {
    Router::new()
        .push(Router::with_path("event/{event_id}").get(get_event))
        .push(Router::with_path("event_auth/{room_id}/{event_id}").get(auth_chain))
        .push(Router::with_path("timestamp_to_event/{room_id}").get(event_by_timestamp))
        .push(Router::with_path("get_missing_events/{room_id}").post(missing_events))
        .push(Router::with_path("exchange_third_party_invite/{room_id}").put(exchange_third_party_invite))
}

/// #GET /_matrix/federation/v1/event/{event_id}
/// Retrieves a single event from the server.
///
/// - Only works if a user of this server is currently invited or joined the room
#[endpoint]
fn get_event(_aa: AuthArgs, args: EventReqArgs, depot: &mut Depot) -> JsonResult<EventResBody> {
    let origin = depot.origin()?;
    let event = DbEvent::get_by_id(&args.event_id)?;
    if event.rejection_reason.is_some() {
        warn!("event {} is rejected, returning 404", &args.event_id);
        return Err(MatrixError::not_found("event not found").into());
    }

    let event_json = timeline::get_pdu_json(&args.event_id)?.ok_or_else(|| {
        warn!("event not found, event id: {:?}", &args.event_id);
        MatrixError::not_found("event not found")
    })?;

    let room_id_str = event_json
        .get("room_id")
        .and_then(|val| val.as_str())
        .ok_or_else(|| AppError::internal("invalid event in database"))?;

    let room_id = <&RoomId>::try_from(room_id_str)
        .map_err(|_| AppError::internal("invalid room id field in event in database"))?;

    crate::federation::access_check(origin, room_id, Some(&args.event_id))?;
    json_ok(EventResBody {
        origin: config::server_name().to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdu: crate::sending::convert_to_outgoing_federation_event(event_json),
    })
}

/// #GET /_matrix/federation/v1/event_auth/{room_id}/{event_id}
/// Retrieves the auth chain for a given event.
///
/// - This does not include the event itself
#[endpoint]
fn auth_chain(
    _aa: AuthArgs,
    args: EventAuthorizationReqArgs,
    depot: &mut Depot,
) -> JsonResult<EventAuthorizationResBody> {
    let origin = depot.origin()?;
    crate::federation::access_check(origin, &args.room_id, None)?;

    let event = timeline::get_pdu_json(&args.event_id)?.ok_or_else(|| {
        warn!("event not found, event id: {:?}", &args.event_id);
        MatrixError::not_found("event not found")
    })?;

    let room_id_str = event
        .get("room_id")
        .and_then(|val| val.as_str())
        .ok_or_else(|| AppError::internal("invalid event in database"))?;

    let room_id = <&RoomId>::try_from(room_id_str)
        .map_err(|_| AppError::internal("invalid room id field in event in database"))?;

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain_ids(room_id, [&*args.event_id].into_iter())?;

    json_ok(EventAuthorizationResBody {
        auth_chain: auth_chain_ids
            .into_iter()
            .filter_map(|id| timeline::get_pdu_json(&id).ok()?)
            .map(crate::sending::convert_to_outgoing_federation_event)
            .collect(),
    })
}

#[endpoint]
async fn event_by_timestamp(
    _aa: AuthArgs,
    args: EventByTimestampReqArgs,
    depot: &mut Depot,
) -> JsonResult<EventByTimestampResBody> {
    let origin = depot.origin()?;
    crate::federation::access_check(origin, &args.room_id, None)?;

    let (event_id, origin_server_ts) = crate::event::get_event_for_timestamp(&args.room_id, args.ts, args.dir)?;
    json_ok(EventByTimestampResBody {
        event_id,
        origin_server_ts,
    })
}

/// #POST /_matrix/federation/v1/get_missing_events/{room_id}
/// Retrieves events that the sender is missing.
#[endpoint]
fn missing_events(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    body: JsonBody<MissingEventsReqBody>,
    depot: &mut Depot,
) -> JsonResult<MissingEventsResBody> {
    let origin = depot.origin()?;

    let room_id = room_id.into_inner();
    crate::federation::access_check(origin, &room_id, None)?;

    let mut queued_events = body.latest_events.clone();
    let mut events = Vec::new();

    let mut i = 0;
    while i < queued_events.len() && events.len() < usize::from(body.limit) as usize {
        let event_id = queued_events[i].clone();
        if let Some(pdu) = timeline::get_pdu_json(&event_id)? {
            let room_id_str = pdu
                .get("room_id")
                .and_then(|val| val.as_str())
                .ok_or_else(|| AppError::internal("invalid event in database"))?;

            let event_room_id = <&RoomId>::try_from(room_id_str)
                .map_err(|_| AppError::internal("invalid room id field in event in database"))?;

            if event_room_id != &room_id {
                warn!(
                    "evil event detected: Event {} found while searching in room {}",
                    event_id, &room_id
                );
                return Err(MatrixError::invalid_param("evil event detected").into());
            }

            if body.earliest_events.contains(&event_id) {
                i += 1;
                continue;
            }

            queued_events.extend_from_slice(
                &serde_json::from_value::<Vec<OwnedEventId>>(
                    serde_json::to_value(
                        pdu.get("prev_events")
                            .cloned()
                            .ok_or_else(|| AppError::internal("Event in db has no prev_events field."))?,
                    )
                    .expect("canonical json is valid json value"),
                )
                .map_err(|_| AppError::internal("invalid prev_events content in pdu in db::"))?,
            );
            if i >= body.latest_events.len() {
                events.push((event_id, crate::sending::convert_to_outgoing_federation_event(pdu)));
            }
        } else {
            warn!("event not found, event id: {:?}", event_id);
        }
        i += 1;
    }
    let events = events
        .into_iter()
        .rev()
        .filter_map(|(event_id, event)| {
            if state::server_can_see_event(origin, &room_id, &event_id).unwrap_or(false) {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    json_ok(MissingEventsResBody { events })
}

#[endpoint]
async fn exchange_third_party_invite(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
