use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::once;
use std::sync::Arc;
use std::time::Duration;

use diesel::prelude::*;
use palpo_core::appservice::third_party;
use salvo::http::StatusError;
use tokio::sync::RwLock;

use crate::core::client::membership::{JoinRoomResBody, ThirdPartySigned};
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::{
    InviteUserResBodyV2, MakeJoinReqArgs, MakeLeaveResBody, SendJoinArgs, SendJoinResBodyV2, SendLeaveReqBody,
    make_leave_request,
};
use crate::core::identifiers::*;
use crate::core::serde::{
    CanonicalJsonObject, CanonicalJsonValue, RawJsonValue, to_canonical_value, to_raw_json_value,
};
use crate::core::{UnixMillis, Seqnum, federation};

use crate::appservice::RegistrationInfo;
use crate::event::{DbEventData, NewDbEvent, PduBuilder, PduEvent, gen_event_id_canonical_json};
use crate::federation::maybe_strip_event_id;
use crate::membership::federation::membership::{
    InviteUserReqArgs, InviteUserReqBodyV2, MakeJoinResBody, RoomStateV1, RoomStateV2, SendJoinReqBody,
    SendLeaveReqArgsV2, send_leave_request_v2,
};
use crate::membership::state::DeltaInfo;
use crate::room::state::{self, CompressedEvent};
use crate::schema::*;
use crate::user::DbUser;
use crate::{AppError, AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError,  SigningKeys, db, diesel_exists};

pub(crate) async fn invite_user(
    inviter_id: &UserId,
    invitee_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    is_direct: bool,
) -> AppResult<()> {
    if invitee_id.server_name().is_remote() {
        let (pdu, pdu_json, invite_room_state) = {
            let content = RoomMemberEventContent {
                avatar_url: None,
                display_name: None,
                is_direct: Some(is_direct),
                membership: MembershipState::Invite,
                third_party_invite: None,
                blurhash: None,
                reason,
                join_authorized_via_users_server: None,
            };

            let (pdu, pdu_json) = crate::room::timeline::create_hash_and_sign_event(
                PduBuilder::state(invitee_id.to_string(), &content),
                inviter_id,
                room_id,
            )?;

            let invite_room_state = crate::room::state::summary_stripped(&pdu)?;

            (pdu, pdu_json, invite_room_state)
        };

        let room_version_id = crate::room::state::get_room_version(room_id)?;

        let invite_request = crate::core::federation::membership::invite_user_request_v2(
            &invitee_id.server_name().origin().await,
            InviteUserReqArgs {
                room_id: room_id.to_owned(),
                event_id: (&*pdu.event_id).to_owned(),
            },
            InviteUserReqBodyV2 {
                room_version: room_version_id.clone(),
                event: crate::sending::convert_to_outgoing_federation_event(pdu_json.clone()),
                invite_room_state,
                via: crate::room::state::servers_route_via(room_id).ok(),
            },
        )?
        .into_inner();
        let send_join_response = crate::sending::send_federation_request(invitee_id.server_name(), invite_request)
            .await?
            .json::<InviteUserResBodyV2>()
            .await?;

        // We do not add the event_id field to the pdu here because of signature and hashes checks
        let (event_id, value) =
            gen_event_id_canonical_json(&send_join_response.event, &room_version_id).map_err(|e| {
                tracing::error!("Could not convert event to canonical json: {e}");
                MatrixError::invalid_param("Could not convert event to canonical json.")
            })?;

        if *pdu.event_id != *event_id {
            warn!(
                "Server {} changed invite event, that's not allowed in the spec: ours: {:?}, theirs: {:?}",
                invitee_id.server_name(),
                pdu_json,
                value
            );
            return Err(MatrixError::bad_json(format!(
                "Server `{}` sent event with wrong event ID",
                invitee_id.server_name()
            ))
            .into());
        }

        let origin: OwnedServerName = serde_json::from_value(
            serde_json::to_value(
                value
                    .get("origin")
                    .ok_or(MatrixError::bad_json("Event needs an origin field."))?,
            )
            .expect("CanonicalJson is valid json value"),
        )
        .map_err(|e| MatrixError::bad_json(format!("Origin field in event is not a valid server name: {e}")))?;

        println!("==ddd  handle_incoming_pdu 1  {event_id}");
        crate::event::handler::handle_incoming_pdu(&origin, &event_id, room_id, value, true).await?;
        return crate::sending::send_pdu_room(room_id, &event_id);
    }

    if !crate::room::is_joined(inviter_id, room_id)? {
        return Err(MatrixError::forbidden("You must be joined in the room you are trying to invite from.").into());
    }

    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content: to_raw_json_value(&RoomMemberEventContent {
                membership: MembershipState::Invite,
                display_name: crate::user::display_name(invitee_id)?,
                avatar_url: crate::user::avatar_url(invitee_id)?,
                is_direct: Some(is_direct),
                third_party_invite: None,
                blurhash: crate::user::blurhash(invitee_id)?,
                reason,
                join_authorized_via_users_server: None,
            })
            .expect("event is valid, we just created it"),
            state_key: Some(invitee_id.to_string()),
            ..Default::default()
        },
        inviter_id,
        room_id,
    )?;

    Ok(())
}
