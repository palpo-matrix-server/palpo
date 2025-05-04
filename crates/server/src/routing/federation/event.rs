use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::federation::authorization::EventAuthorizationResBody;
use crate::core::federation::event::{EventResBody, MissingEventReqBody, MissingEventResBody};
use crate::core::identifiers::*;
use crate::core::room::RoomEventReqArgs;
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
fn get_event(_aa: AuthArgs, event_id: PathParam<OwnedEventId>, depot: &mut Depot) -> JsonResult<EventResBody> {
    let origin = depot.origin()?;
    let event = crate::room::timeline::get_pdu_json(&event_id)?.ok_or_else(|| {
        warn!("Event not found, event ID: {:?}", &event_id);
        MatrixError::not_found("Event not found.")
    })?;

    let room_id_str = event
        .get("room_id")
        .and_then(|val| val.as_str())
        .ok_or_else(|| AppError::internal("Invalid event in database"))?;

    let room_id = <&RoomId>::try_from(room_id_str)
        .map_err(|_| AppError::internal("Invalid room id field in event in database"))?;

    crate::federation::access_check(origin, room_id, Some(&event_id))?;
    json_ok(EventResBody {
        origin: config::server_name().to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdu: crate::sending::convert_to_outgoing_federation_event(event),
    })
}

/// #GET /_matrix/federation/v1/event_auth/{room_id}/{event_id}
/// Retrieves the auth chain for a given event.
///
/// - This does not include the event itself
#[endpoint]
fn auth_chain(_aa: AuthArgs, args: RoomEventReqArgs, depot: &mut Depot) -> JsonResult<EventAuthorizationResBody> {
    let origin = depot.origin()?;
    crate::federation::access_check(origin, &args.room_id, None)?;

    let event = crate::room::timeline::get_pdu_json(&args.event_id)?.ok_or_else(|| {
        warn!("Event not found, event ID: {:?}", &args.event_id);
        MatrixError::not_found("Event not found.")
    })?;

    let room_id_str = event
        .get("room_id")
        .and_then(|val| val.as_str())
        .ok_or_else(|| AppError::internal("Invalid event in database"))?;

    let room_id = <&RoomId>::try_from(room_id_str)
        .map_err(|_| AppError::internal("Invalid room id field in event in database"))?;

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain_ids(room_id, [&*args.event_id].into_iter())?;

    json_ok(EventAuthorizationResBody {
        auth_chain: auth_chain_ids
            .into_iter()
            .filter_map(|id| crate::room::timeline::get_pdu_json(&id).ok()?)
            .map(crate::sending::convert_to_outgoing_federation_event)
            .collect(),
    })
}

#[endpoint]
async fn event_by_timestamp(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}

/// #POST /_matrix/federation/v1/get_missing_events/{room_id}
/// Retrieves events that the sender is missing.
#[endpoint]
fn missing_events(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    body: JsonBody<MissingEventReqBody>,
    depot: &mut Depot,
) -> JsonResult<MissingEventResBody> {
    let origin = depot.origin()?;

    let room_id = room_id.into_inner();
    crate::federation::access_check(origin, &room_id, None)?;

    let mut queued_events = body.latest_events.clone();
    let mut events = Vec::new();

    let mut i = 0;
    while i < queued_events.len() && events.len() < usize::from(body.limit) as usize {
        if let Some(pdu) = crate::room::timeline::get_pdu_json(&queued_events[i])? {
            let room_id_str = pdu
                .get("room_id")
                .and_then(|val| val.as_str())
                .ok_or_else(|| AppError::internal("Invalid event in database"))?;

            let event_room_id = <&RoomId>::try_from(room_id_str)
                .map_err(|_| AppError::internal("Invalid room id field in event in database"))?;

            if event_room_id != &room_id {
                warn!(
                    "Evil event detected: Event {} found while searching in room {}",
                    queued_events[i], &room_id
                );
                return Err(MatrixError::invalid_param("Evil event detected").into());
            }

            if body.earliest_events.contains(&queued_events[i]) {
                i += 1;
                continue;
            }

            if !crate::room::state::server_can_see_event(origin, &room_id, &queued_events[i])? {
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
                .map_err(|_| AppError::internal("Invalid prev_events content in pdu in db::"))?,
            );
            events.push(crate::sending::convert_to_outgoing_federation_event(pdu));
        }
        i += 1;
    }

    json_ok(MissingEventResBody { events })
}

#[endpoint]
async fn exchange_third_party_invite(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
