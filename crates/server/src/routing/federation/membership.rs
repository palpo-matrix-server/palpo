use std::sync::Arc;

use palpo_core::RawJson;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::value::to_raw_value;

use crate::core::client::uiaa::AuthData;
use crate::core::events::room::join_rules::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::*;
use crate::core::http::{RoomEventReqArgs, RoomUserReqArgs};
use crate::core::serde::{CanonicalJsonValue, JsonObject};
use crate::core::{EventId, OwnedUserId};
use crate::{
    db, empty_ok, hoops, json_ok, utils, AppError, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult,
    MatrixError, PduBuilder, PduEvent,
};

pub fn router_v1() -> Router {
    Router::new()
        .push(Router::with_path("make_join/<room_id>/<user_id>").get(make_join_event))
        .push(Router::with_path("invite/<room_id>/<user_id>").put(invite_user))
        .push(Router::with_path("make_leave/<room_id>/<user_id>").get(make_leave_event))
        .push(Router::with_path("send_join/<room_id>/<user_id>").get(send_join_event_v1))
        .push(Router::with_path("send_leave/<room_id>/<user_id>").get(send_leave_event))
}
pub fn router_v2() -> Router {
    Router::new()
        .push(Router::with_path("make_join/<room_id>/<user_id>").get(make_join_event))
        .push(Router::with_path("invite/<room_id>/<user_id>").put(invite_user))
        .push(Router::with_path("make_leave/<room_id>/<user_id>").get(make_leave_event))
        .push(Router::with_path("send_join/<room_id>/<user_id>").get(send_join_event_v2))
        .push(Router::with_path("send_leave/<room_id>/<user_id>").get(send_leave_event))
}

// #GET /_matrix/federation/v1/make_join/{room_id}/{user_id}
/// Creates a join template.
#[endpoint]
async fn make_join_event(
    _aa: AuthArgs,
    args: MakeJoinEventReqArgs,
    depot: &mut Depot,
) -> JsonResult<MakeJoinEventResBody> {
    let authed = depot.authed_info()?;

    if !crate::room::exists(&args.room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }
    crate::event::handler::acl_check(authed.server_name(), &args.room_id)?;

    // TODO: Palpo does not implement restricted join rules yet, we always reject
    let join_rules_event = crate::room::state::get_state(&args.room_id, &StateEventType::RoomJoinRules, "")?;

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
            unsigned: None,
            state_key: Some(args.user_id.to_string()),
            redacts: None,
        },
        &args.user_id,
        &args.room_id,
    )?;

    pdu_json.remove("event_id");

    json_ok(MakeJoinEventResBody {
        room_version: Some(room_version_id),
        event: to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
    })
}

// #PUT /_matrix/federation/v2/invite/{room_id}/{event_id}
/// Invites a remote user to a room.
#[endpoint]
fn invite_user(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<InviteUserReqBodyV2>,
    depot: &mut Depot,
) -> JsonResult<InviteUserResBodyV2> {
    let authed = depot.authed_info()?;
    crate::event::handler::acl_check(authed.server_name(), &args.room_id)?;

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
        &crate::config().server_name.as_str(),
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

    event.insert("event_id".to_owned(), "$dummy".into());

    let pdu: PduEvent = serde_json::from_value(event.into()).map_err(|e| {
        warn!("Invalid invite event: {}", e);
        MatrixError::invalid_param("Invalid invite event.")
    })?;

    invite_state.push(pdu.to_stripped_state_event());

    // If we are active in the room, the remote server will notify us about the join via /send
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

    json_ok(InviteUserResBodyV2 {
        event: PduEvent::convert_to_outgoing_federation_event(signed_event),
    })
}

#[endpoint]
async fn make_leave_event(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    panic!("make_leave_eventNot implemented")
}

// #PUT /_matrix/federation/v2/send_join/{room_id}/{event_id}
/// Invites a remote user to a room.
#[endpoint]
async fn send_join_event_v2(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<SendJoinEventReqBodyV2>,
    depot: &mut Depot,
) -> JsonResult<SendJoinEventResBodyV2> {
    let authed = depot.authed_info()?;

    crate::event::handler::acl_check(authed.server_name(), &args.room_id)?;

    let room_state = crate::membership::send_join_event_v2(authed.server_name(), &args.room_id, &body.pdu).await?;

    json_ok(SendJoinEventResBodyV2 { room_state })
}

// #PUT /_matrix/federation/v1/send_join/{room_id}/{event_id}
/// Submits a signed join event.
#[endpoint]
async fn send_join_event_v1(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<SendJoinEventReqBodyV1>,
    depot: &mut Depot,
) -> JsonResult<SendJoinEventResBodyV2> {
    let authed = depot.authed_info()?;

    let room_state = crate::membership::send_join_event_v2(authed.server_name(), &args.room_id, &body.pdu).await?;
    json_ok(SendJoinEventResBodyV2 { room_state })
}
#[endpoint]
async fn send_leave_event(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
