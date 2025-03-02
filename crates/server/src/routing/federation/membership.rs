use diesel::prelude::*;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::value::to_raw_value;
use ulid::Ulid;

use crate::core::events::room::join_rules::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::*;
use crate::core::room::RoomEventReqArgs;
use crate::core::serde::{CanonicalJsonValue, JsonObject};
use crate::core::{EventId, OwnedRoomId, OwnedUserId, RoomVersionId, UnixMillis};
use crate::room::NewDbRoom;
use crate::{AppError, EmptyResult, JsonResult, MatrixError, PduBuilder, PduEvent, db, empty_ok, json_ok, utils};
use crate::{DepotExt, schema::*};

pub fn router_v1() -> Router {
    Router::new()
        .push(Router::with_path("make_join/{room_id}/{user_id}").get(make_join))
        .push(Router::with_path("invite/{room_id}/{event_id}").put(invite_user))
        .push(Router::with_path("make_leave/{room_id}/{user_id}").get(make_leave))
        .push(Router::with_path("send_join/{room_id}/{event_id}").put(send_join_v1))
        .push(Router::with_path("send_leave/{room_id}/{event_id}").put(send_leave))
}
pub fn router_v2() -> Router {
    Router::new()
        .push(Router::with_path("make_join/{room_id}/{user_id}").get(make_join))
        .push(Router::with_path("invite/{room_id}/{event_id}").put(invite_user))
        .push(Router::with_path("make_leave/{room_id}/{user_id}").get(make_leave))
        .push(Router::with_path("send_join/{room_id}/{event_id}").put(send_join_v2))
        .push(Router::with_path("send_leave/{room_id}/{event_id}").put(send_leave))
}

/// #GET /_matrix/federation/v1/make_join/{room_id}/{user_id}
/// Creates a join template.
#[endpoint]
async fn make_join(args: MakeJoinReqArgs) -> JsonResult<MakeJoinResBody> {
    if !crate::room::room_exists(&args.room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }
    crate::event::handler::acl_check(args.user_id.server_name(), &args.room_id)?;
    // TODO: Palpo does not implement restricted join rules yet, we always reject
    let join_rules_event = crate::room::state::get_state(&args.room_id, &StateEventType::RoomJoinRules, "", None)?;
    let join_rules_event_content: Option<RoomJoinRulesEventContent> = join_rules_event
        .as_ref()
        .map(|join_rules_event| {
            serde_json::from_str(join_rules_event.content.get()).map_err(|e| {
                warn!("Invalid join rules event: {}", e);
                AppError::internal("Invalid join rules event in db::")
            })
        })
        .transpose()?;
    if let Some(join_rules_event_content) = join_rules_event_content {
        if matches!(
            join_rules_event_content.join_rule,
            JoinRule::Restricted { .. } | JoinRule::KnockRestricted { .. }
        ) {
            return Err(MatrixError::unable_to_authorize_join("Palpo does not support restricted rooms yet.").into());
        }
    }
    let room_version_id = crate::room::state::get_room_version(&args.room_id)?;
    if !args.ver.contains(&room_version_id) {
        return Err(MatrixError::incompatible_room_version(room_version_id, "Room version not supported.").into());
    }
    let content = to_raw_value(&RoomMemberEventContent {
        avatar_url: None,
        blurhash: None,
        display_name: None,
        is_direct: None,
        membership: MembershipState::Join,
        third_party_invite: None,
        reason: None,
        join_authorized_via_users_server: None,
    })
    .expect("member event is valid value");
    let (_pdu, mut pdu_json) = crate::room::timeline::create_hash_and_sign_event(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content,
            state_key: Some(args.user_id.to_string()),
            ..Default::default()
        },
        &args.user_id,
        &args.room_id,
    )?;
    pdu_json.remove("event_id");
    json_ok(MakeJoinResBody {
        room_version: Some(room_version_id),
        event: to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
    })
}

/// #PUT /_matrix/federation/v2/invite/{room_id}/{event_id}
/// Invites a remote user to a room.
#[endpoint]
async fn invite_user(
    args: RoomEventReqArgs,
    body: JsonBody<InviteUserReqBodyV2>,
    depot: &mut Depot,
) -> JsonResult<InviteUserResBodyV2> {
    let body = body.into_inner();
    let server_name = &crate::config().server_name;
    crate::event::handler::acl_check(&server_name, &args.room_id)?;

    if !crate::supported_room_versions().contains(&body.room_version) {
        return Err(MatrixError::incompatible_room_version(
            body.room_version.clone(),
            "Server does not support this room version.",
        )
        .into());
    }

    let mut signed_event =
        utils::to_canonical_object(&body.event).map_err(|_| MatrixError::invalid_param("Invite event is invalid."))?;

    crate::core::signatures::hash_and_sign_event(
        server_name.as_str(),
        crate::keypair(),
        &mut signed_event,
        &body.room_version,
    )
    .map_err(|_| MatrixError::invalid_param("Failed to sign event."))?;

    // Generate event id
    let event_id = EventId::parse(format!(
        "${}",
        crate::core::signatures::reference_hash(&signed_event, &body.room_version)
            .expect("palpo can calculate reference hashes")
    ))
    .expect("palpo's reference hashes are valid event ids");

    // Add event_id back
    signed_event.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.to_string()));

    let sender: OwnedUserId = serde_json::from_value(
        signed_event
            .get("sender")
            .ok_or(MatrixError::invalid_param("Event had no sender field."))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::invalid_param("sender is not a user id."))?;

    let invited_user: Box<_> = serde_json::from_value(
        signed_event
            .get("state_key")
            .ok_or(MatrixError::invalid_param("Event had no state_key field."))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::invalid_param("state_key is not a user id."))?;

    let mut invite_state = body.invite_room_state.clone();

    let mut event: JsonObject = serde_json::from_str(body.event.get())
        .map_err(|_| MatrixError::invalid_param("Invalid invite event bytes."))?;

    event.insert(
        "event_id".to_owned(),
        format!("$dummy_{}", Ulid::new().to_string()).into(),
    );

    let pdu: PduEvent = serde_json::from_value(event.into()).map_err(|e| {
        warn!("Invalid invite event: {}", e);
        MatrixError::invalid_param("Invalid invite event.")
    })?;

    invite_state.push(pdu.to_stripped_state_event());

    // If we are active in the room, the remote server will notify us about the join via /send.
    // If we are not in the room, we need to manually
    // record the invited state for client /sync through update_membership(), and
    // send the invite PDU to the relevant appservices.
    if !crate::room::is_server_in_room(&crate::config().server_name, &args.room_id)? {
        crate::room::update_membership(
            &pdu.event_id,
            pdu.event_sn,
            &args.room_id,
            &invited_user,
            MembershipState::Invite,
            &sender,
            Some(invite_state),
        )?;
    }

    diesel::insert_into(rooms::table)
        .values(NewDbRoom {
            id: args.room_id.clone(),
            version: body.room_version.to_string(),
            is_public: false,
            min_depth: 0,
            has_auth_chain_index: false,
            created_by: sender.clone(),
            created_at: UnixMillis::now(),
        })
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)?;

    json_ok(InviteUserResBodyV2 {
        event: PduEvent::convert_to_outgoing_federation_event(signed_event),
    })
}

/// # `GET /_matrix/federation/v1/make_leave/{roomId}/userId}`
#[endpoint]
async fn make_leave(args: MakeLeaveReqArgs) -> JsonResult<MakeLeaveResBody> {
    let server_name = &crate::config().server_name;
    if !crate::room::is_server_in_room(server_name, &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room").into());
    }

    // if args.user_id.server_name() != server_name {
    //     return Err(MatrixError::invalid_param(
    //         "Not allowed to leave on behalf of another server/user",
    //     ).into());
    // }

    // ACL check origin
    crate::event::handler::acl_check(server_name, &args.room_id)?;

    let room_version_id = crate::room::state::get_room_version(&args.room_id)?;
    // let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;

    let (_pdu, mut pdu_json) = crate::room::timeline::create_hash_and_sign_event(
        PduBuilder::state(
            args.user_id.to_string(),
            &RoomMemberEventContent::new(MembershipState::Leave),
        ),
        &args.user_id,
        &args.room_id,
    )?;

    // drop(state_lock);

    // room v3 and above removed the "event_id" field from remote PDU format
    match room_version_id {
        RoomVersionId::V1 | RoomVersionId::V2 => {}
        _ => {
            pdu_json.remove("event_id");
        }
    };

    json_ok(MakeLeaveResBody {
        room_version: Some(room_version_id),
        event: to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
    })
}

/// #PUT /_matrix/federation/v2/send_join/{room_id}/{event_id}
/// Invites a remote user to a room.
#[endpoint]
async fn send_join_v2(
    depot: &mut Depot,
    args: RoomEventReqArgs,
    body: JsonBody<SendJoinReqBody>,
) -> JsonResult<SendJoinResBodyV2> {
    let body = body.into_inner();
    // let server_name = args.room_id.server_name().map_err(AppError::public)?;
    // crate::event::handler::acl_check(&server_name, &args.room_id)?;

    let room_state = crate::membership::send_join_v2(depot.origin()?, &args.room_id, &body.0).await?;

    json_ok(SendJoinResBodyV2(room_state))
}

/// #PUT /_matrix/federation/v1/send_join/{room_id}/{event_id}
/// Submits a signed join event.
#[endpoint]
async fn send_join_v1(
    depot: &mut Depot,
    args: RoomEventReqArgs,
    body: JsonBody<SendJoinReqBody>,
) -> JsonResult<SendJoinResBodyV1> {
    let body = body.into_inner();
    let room_state = crate::membership::send_join_v1(depot.origin()?, &args.room_id, &body.0).await?;
    json_ok(SendJoinResBodyV1(room_state))
}

/// # `PUT /_matrix/federation/v2/send_leave/{roomId}/{eventId}`
///
/// Submits a signed leave event.
#[endpoint]
async fn send_leave(depot: &mut Depot, args: SendLeaveReqArgsV2, body: JsonBody<SendLeaveReqBody>) -> EmptyResult {
    let server_name = &crate::config().server_name;
    let origin = depot.origin()?;
    let body = body.into_inner();
    if !crate::room::is_server_in_room(server_name, &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room").into());
    }

    crate::event::handler::acl_check(server_name, &args.room_id)?;

    // We do not add the event_id field to the pdu here because of signature and
    // hashes checks
    let room_version_id = crate::room::state::get_room_version(&args.room_id)?;
    let Ok((event_id, value)) = crate::event::gen_event_id_canonical_json(&body.0, &room_version_id) else {
        // Event could not be converted to canonical json
        return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
    };

    let event_room_id: OwnedRoomId = serde_json::from_value(
        serde_json::to_value(
            value
                .get("room_id")
                .ok_or_else(|| MatrixError::bad_json("Event missing room_id property."))?,
        )
        .expect("CanonicalJson is valid json value"),
    )
    .map_err(|e| MatrixError::bad_json("room_id field is not a valid room ID: {e}"))?;

    if event_room_id != args.room_id {
        return Err(MatrixError::bad_json("Event room_id does not match request path room ID.").into());
    }

    let content: RoomMemberEventContent = serde_json::from_value(
        value
            .get("content")
            .ok_or_else(|| MatrixError::bad_json("Event missing content property"))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("Event content is empty or invalid"))?;

    if content.membership != MembershipState::Leave {
        return Err(
            MatrixError::bad_json("Not allowed to send a non-leave membership event to leave endpoint.").into(),
        );
    }

    let event_type: StateEventType = serde_json::from_value(
        value
            .get("type")
            .ok_or_else(|| MatrixError::bad_json("Event missing type property."))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("Event does not have a valid state event type."))?;

    if event_type != StateEventType::RoomMember {
        return Err(
            MatrixError::invalid_param("Not allowed to send non-membership state event to leave endpoint.").into(),
        );
    }

    // ACL check sender server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::bad_json("Event missing sender property."))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("User ID in sender is invalid."))?;

    crate::event::handler::acl_check(sender.server_name(), &args.room_id)?;

    if sender.server_name() != origin {
        return Err(MatrixError::bad_json("Not allowed to leave on behalf of another server.").into());
    }

    let state_key: OwnedUserId = serde_json::from_value(
        value
            .get("state_key")
            .ok_or_else(|| MatrixError::invalid_param("Event missing state_key property."))?
            .clone()
            .into(),
    )
    .map_err(|_| MatrixError::bad_json("state_key is invalid or not a user ID"))?;

    if state_key != sender {
        return Err(MatrixError::bad_json("state_key does not match sender user.").into());
    }

    // let mutex_lock = services.rooms.event_handler.mutex_federation.lock(room_id).await;
    crate::event::handler::handle_incoming_pdu(origin, &event_id, &args.room_id, value, true).await?;
    // drop(mutex_lock);

    let servers = crate::room::get_room_servers(&args.room_id, false).unwrap();
    crate::sending::send_pdu(servers.into_iter(), &event_id).unwrap();
    empty_ok()
}
