use palpo_core::federation::knock::{MakeKnockReqArgs, SendKnockReqArgs, SendKnockReqBody, SendKnockResBody};
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::directory::{PublicRoomsFilteredReqBody, PublicRoomsReqArgs};
use crate::core::directory::{PublicRoomFilter, PublicRoomsResBody, RoomNetwork};
use crate::core::events::StateEventType;
use crate::core::events::room::member::MembershipState;
use crate::core::events::room::member::RoomMemberEventContent;
use crate::core::federation::event::{
    RoomStateAtEventReqArgs, RoomStateIdsResBody, RoomStateReqArgs, RoomStateResBody,
};
use crate::core::federation::knock::MakeKnockResBody;
use crate::core::identifiers::*;
use crate::core::serde::JsonObject;
use crate::event::gen_event_id_canonical_json;
use crate::{
    AuthArgs, DepotExt, EmptyResult, IsRemoteOrLocal, JsonResult, MatrixError, PduBuilder, PduEvent, empty_ok, json_ok,
};
use serde_json::value::to_raw_value;

pub fn router() -> Router {
    Router::new()
        .push(Router::with_path("state/{room_id}").get(get_state))
        .push(
            Router::with_path("publicRooms")
                .get(get_public_rooms)
                .post(get_filtered_public_rooms),
        )
        .push(Router::with_path("send_knock/{room_id}/{event_id}").put(send_knock))
        .push(Router::with_path("make_knock/{room_id}/{user_id}").put(make_knock))
        .push(Router::with_path("state_ids/{room_id}").get(get_state_at_event))
}

/// #GET /_matrix/federation/v1/state/{room_id}
/// Retrieves the current state of the room.
#[endpoint]
async fn get_state(_aa: AuthArgs, args: RoomStateReqArgs, depot: &mut Depot) -> JsonResult<RoomStateResBody> {
    let origin = depot.origin()?;
    crate::federation::access_check(origin, &args.room_id, None)?;

    let state_hash =
        crate::room::state::get_pdu_frame_id(&args.event_id)?.ok_or(MatrixError::not_found("Pdu state not found."))?;

    let pdus = crate::room::state::get_full_state_ids(state_hash)?
        .into_values()
        .map(|id| {
            crate::sending::convert_to_outgoing_federation_event(
                crate::room::timeline::get_pdu_json(&id).unwrap().unwrap(),
            )
        })
        .collect();

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain_ids(&args.room_id, [&*args.event_id].into_iter())?;

    json_ok(RoomStateResBody {
        auth_chain: auth_chain_ids
            .into_iter()
            .filter_map(|id| match crate::room::timeline::get_pdu_json(&id).ok()? {
                Some(json) => Some(crate::sending::convert_to_outgoing_federation_event(json)),
                None => {
                    error!("Could not find event json for {id} in db::");
                    None
                }
            })
            .collect(),
        pdus,
    })
}

/// #GET /_matrix/federation/v1/publicRooms
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

/// #POST /_matrix/federation/v1/publicRooms
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

/// # `PUT /_matrix/federation/v1/send_knock/{roomId}/{eventId}`
///
/// Submits a signed knock event.
#[endpoint]
async fn send_knock(
    _aa: AuthArgs,
    args: SendKnockReqArgs,
    body: JsonBody<SendKnockReqBody>,
    depot: &mut Depot,
) -> JsonResult<SendKnockResBody> {
    use crate::core::RoomVersionId::*;

    let origin = depot.origin()?;
    let body = body.into_inner();

    if args.room_id.is_remote() {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }

    // ACL check origin server
    crate::event::handler::acl_check(origin, &args.room_id)?;

    let room_version_id = crate::room::state::get_room_version(&args.room_id)?;

    if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6) {
        return Err(MatrixError::forbidden("Room version does not support knocking.").into());
    }

    let Ok((event_id, value)) = gen_event_id_canonical_json(&body.pdu, &room_version_id) else {
        // Event could not be converted to canonical json
        return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
    };

    let event_type: StateEventType = serde_json::from_value(
        value
            .get("type")
            .ok_or_else(|| MatrixError::invalid_param("Event has no event type."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::invalid_param("Event has invalid event type: {e}"))?;

    if event_type != StateEventType::RoomMember {
        return Err(
            MatrixError::invalid_param("Not allowed to send non-membership state event to knock endpoint.").into(),
        );
    }

    let content: RoomMemberEventContent = serde_json::from_value(
        value
            .get("content")
            .ok_or_else(|| MatrixError::invalid_param("Membership event has no content"))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::invalid_param(format!("Event has invalid membership content: {e}")))?;

    if content.membership != MembershipState::Knock {
        return Err(
            MatrixError::invalid_param("Not allowed to send a non-knock membership event to knock endpoint.").into(),
        );
    }

    // ACL check sender server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::invalid_param("Event has no sender user ID."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::invalid_param(format!("Event sender is not a valid user ID: {e}")))?;

    crate::event::handler::acl_check(sender.server_name(), &args.room_id)?;

    // check if origin server is trying to send for another server
    if sender.server_name() != origin {
        return Err(MatrixError::bad_json("Not allowed to knock on behalf of another server/user.").into());
    }

    let state_key: OwnedUserId = serde_json::from_value(
        value
            .get("state_key")
            .ok_or_else(|| MatrixError::invalid_param("Event does not have a state_key"))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("Event does not have a valid state_key: {e}")))?;

    if state_key != sender {
        return Err(MatrixError::invalid_param("state_key does not match sender user of event.").into());
    };

    let origin: OwnedServerName = serde_json::from_value(
        value
            .get("origin")
            .ok_or_else(|| MatrixError::bad_json("Event does not have an origin server name."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("Event has an invalid origin server name: {e}")))?;

    let mut event: JsonObject = serde_json::from_str(body.pdu.get())
        .map_err(|e| MatrixError::invalid_param(format!("Invalid knock event PDU: {e}")))?;

    event.insert("event_id".to_owned(), "$placeholder".into());

    let pdu: PduEvent = serde_json::from_value(event.into())
        .map_err(|e| MatrixError::invalid_param(format!("Invalid knock event PDU: {e}")))?;

    // let mutex_lock = crate::event::mutex_federation.lock(&body.room_id).await;

    crate::event::handler::handle_incoming_pdu(&origin, &event_id, &args.room_id, value.clone(), true)
        .await
        .map_err(|_| MatrixError::invalid_param("Could not accept as timeline event."))?;

    // drop(mutex_lock);

    crate::sending::send_pdu_room(&args.room_id, &event_id)?;

    let knock_room_state = crate::room::state::summary_stripped(&pdu)?;

    json_ok(SendKnockResBody { knock_room_state })
}

/// # `GET /_matrix/federation/v1/make_knock/{roomId}/{userId}`
///
/// Creates a knock template.
#[endpoint]
async fn make_knock(_aa: AuthArgs, args: MakeKnockReqArgs, depot: &mut Depot) -> JsonResult<MakeKnockResBody> {
    use crate::core::RoomVersionId::*;

    let origin = depot.origin()?;
    if !crate::room::room_exists(&args.room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }

    if args.user_id.server_name() != origin {
        return Err(MatrixError::bad_json("Not allowed to knock on behalf of another server/user.").into());
    }

    // ACL check origin server
    crate::event::handler::acl_check(origin, &args.room_id)?;

    let room_version_id = crate::room::state::get_room_version(&args.room_id)?;

    if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6) {
        return Err(
            MatrixError::incompatible_room_version(room_version_id, "Room version does not support knocking.").into(),
        );
    }

    // if !args.ver.contains(&room_version_id) {
    //     return Err(MatrixError::incompatible_room_version(
    //         room_version_id,
    //         "Your homeserver does not support the features required to knock on this room.",
    //     ));
    // }

    // let state_lock = crate::room::state::mutex.lock(&body.room_id).await;

    if let Ok(Some(member)) = crate::room::state::get_member(&args.room_id, &args.user_id) {
        if member.membership == MembershipState::Ban {
            warn!(
                "Remote user {} is banned from {} but attempted to knock",
                &args.user_id, &args.room_id
            );
            return Err(MatrixError::forbidden("You cannot knock on a room you are banned from.").into());
        }
    }

    let (_pdu, mut pdu_json) = crate::room::timeline::create_hash_and_sign_event(
        PduBuilder::state(
            args.user_id.to_string(),
            &RoomMemberEventContent::new(MembershipState::Knock),
        ),
        &args.user_id,
        &args.room_id,
        // &state_lock,
    )?;

    // drop(state_lock);

    // room v3 and above removed the "event_id" field from remote PDU format
    crate::federation::maybe_strip_event_id(&mut pdu_json, &room_version_id);

    json_ok(MakeKnockResBody {
        room_version: room_version_id,
        event: to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
    })
}

/// #GET /_matrix/federation/v1/state_ids/{room_id}
/// Retrieves the current state of the room.
#[endpoint]
fn get_state_at_event(depot: &mut Depot, args: RoomStateAtEventReqArgs) -> JsonResult<RoomStateIdsResBody> {
    let origin = depot.origin()?;

    crate::federation::access_check(origin, &args.room_id, Some(&args.event_id))?;

    let frame_id =
        crate::room::state::get_pdu_frame_id(&args.event_id)?.ok_or(MatrixError::not_found("Pdu state not found."))?;

    let pdu_ids = crate::room::state::get_full_state_ids(frame_id)?
        .into_values()
        .map(|id| (*id).to_owned())
        .collect();

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain_ids(&args.room_id, [&*args.event_id].into_iter())?;

    json_ok(RoomStateIdsResBody {
        auth_chain_ids: auth_chain_ids.into_iter().map(|id| (*id).to_owned()).collect(),
        pdu_ids,
    })
}
