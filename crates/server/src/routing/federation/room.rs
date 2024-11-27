use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::directory::{PublicRoomsFilteredReqBody, PublicRoomsReqArgs};
use crate::core::directory::{PublicRoomFilter, PublicRoomsResBody, RoomNetwork};
use crate::core::federation::event::{
    RoomStateAtEventReqArgs, RoomStateIdsResBody, RoomStateReqArgs, RoomStateResBody,
};
use crate::{empty_ok, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, PduEvent};

pub fn router() -> Router {
    Router::new()
        .push(Router::with_path("state/<room_id>").get(get_state))
        .push(
            Router::with_path("publicRooms")
                .get(get_public_rooms)
                .post(get_filtered_public_rooms),
        )
        .push(Router::with_path("send_knock/<room_id>/<event_id>").put(send_knock))
        .push(Router::with_path("make_knock/<room_id>/<user_id>").put(make_knock))
        .push(Router::with_path("state_ids/<room_id>").get(get_state_at_event))
}

// #GET /_matrix/federation/v1/state/{room_id}
/// Retrieves the current state of the room.
#[endpoint]
async fn get_state(_aa: AuthArgs, args: RoomStateReqArgs, depot: &mut Depot) -> JsonResult<RoomStateResBody> {
    let server_name = &crate::config().server_name;

    if !crate::room::is_server_in_room(server_name, &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room.").into());
    }

    crate::event::handler::acl_check(server_name, &args.room_id)?;

    let state_hash =
        crate::room::state::get_pdu_frame_id(&args.event_id)?.ok_or(MatrixError::not_found("Pdu state not found."))?;

    let pdus = crate::room::state::get_full_state_ids(state_hash)?
        .into_values()
        .map(|id| {
            PduEvent::convert_to_outgoing_federation_event(crate::room::timeline::get_pdu_json(&id).unwrap().unwrap())
        })
        .collect();

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain(&args.room_id, &args.event_id)?;

    json_ok(RoomStateResBody {
        auth_chain: auth_chain_ids
            .into_iter()
            .filter_map(|id| match crate::room::timeline::get_pdu_json(&id).ok()? {
                Some(json) => Some(PduEvent::convert_to_outgoing_federation_event(json)),
                None => {
                    error!("Could not find event json for {id} in db::");
                    None
                }
            })
            .collect(),
        pdus,
    })
}

// #GET /_matrix/federation/v1/publicRooms
/// Lists the public rooms on this server.
#[endpoint]
async fn get_public_rooms(_aa: AuthArgs, args: PublicRoomsReqArgs) -> JsonResult<PublicRoomsResBody> {
    let body = crate::directory::get_public_rooms(
        None,
        args.limit,
        args.since.as_deref(),
        &PublicRoomFilter::default(),
        &RoomNetwork::Matrix,
    )
    .await?;
    json_ok(body)
}

// #POST /_matrix/federation/v1/publicRooms
/// Lists the public rooms on this server.
#[endpoint]
async fn get_filtered_public_rooms(
    _aa: AuthArgs,
    args: JsonBody<PublicRoomsFilteredReqBody>,
) -> JsonResult<PublicRoomsResBody> {
    let body = crate::directory::get_public_rooms(
        args.server.as_deref(),
        args.limit,
        args.since.as_deref(),
        &args.filter,
        &args.room_network,
    )
    .await?;
    json_ok(body)
}
#[endpoint]
async fn send_knock(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
async fn make_knock(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

// #GET /_matrix/federation/v1/state_ids/{room_id}
/// Retrieves the current state of the room.
#[endpoint]
fn get_state_at_event(
    _aa: AuthArgs,
    args: RoomStateAtEventReqArgs,
    depot: &mut Depot,
) -> JsonResult<RoomStateIdsResBody> {
    let server_name = &crate::config().server_name;

    if !crate::room::is_server_in_room(server_name, &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room.").into());
    }

    crate::event::handler::acl_check(server_name, &args.room_id)?;

    let frame_id =
        crate::room::state::get_pdu_frame_id(&args.event_id)?.ok_or(MatrixError::not_found("Pdu state not found."))?;

    let pdu_ids = crate::room::state::get_full_state_ids(frame_id)?
        .into_values()
        .map(|id| (*id).to_owned())
        .collect();

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain(&args.room_id, &args.event_id)?;

    json_ok(RoomStateIdsResBody {
        auth_chain_ids: auth_chain_ids.into_iter().map(|id| (*id).to_owned()).collect(),
        pdu_ids,
    })
}
