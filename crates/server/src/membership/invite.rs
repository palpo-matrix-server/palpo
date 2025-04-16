use crate::core::events::TimelineEventType;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::federation::membership::InviteUserResBodyV2;
use crate::core::identifiers::*;
use crate::core::serde::to_raw_json_value;
use crate::event::{PduBuilder, gen_event_id_canonical_json};
use crate::membership::federation::membership::{InviteUserReqArgs, InviteUserReqBodyV2};
use crate::{AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError};

pub async fn invite_user(
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
