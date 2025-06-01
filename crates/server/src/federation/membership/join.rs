use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::iter::once;
use std::sync::Arc;

use diesel::prelude::*;
use palpo_core::serde::JsonValue;
use salvo::http::StatusError;
use tokio::sync::RwLock;
use tracing_subscriber::fmt::format;

use crate::appservice::RegistrationInfo;
use crate::core::UnixMillis;
use crate::core::client::membership::{JoinRoomResBody, ThirdPartySigned};
use crate::core::device::DeviceListUpdateContent;
use crate::core::events::room::join_rule::{AllowRule, JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::{
    MakeJoinReqArgs, MakeJoinResBody, RoomStateV1, RoomStateV2, SendJoinArgs, SendJoinReqBody, SendJoinResBodyV2,
};
use crate::core::federation::transaction::Edu;
use crate::core::identifiers::*;
use crate::core::serde::{
    CanonicalJsonObject, CanonicalJsonValue, RawJsonValue, to_canonical_value, to_raw_json_value,
};
use crate::data::room::{DbEventData, NewDbEvent};
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::event::{PduBuilder, PduEvent, gen_event_id_canonical_json};
use crate::federation::maybe_strip_event_id;
use crate::room::state::CompressedEvent;
use crate::room::state::DeltaInfo;
use crate::room::{state, timeline};
use crate::sending::send_edu_server;
use crate::{
    AppError, AppResult, AuthedInfo, GetUrlOrigin, IsRemoteOrLocal, MatrixError, OptionalExtension, config, data, room,
};

pub async fn send_join_v1(origin: &ServerName, room_id: &RoomId, pdu: &RawJsonValue) -> AppResult<RoomStateV1> {
    println!("==========send join v1==========");
    if !room::room_exists(room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }

     println!("==========send join v1==========1");
    crate::event::handler::acl_check(origin, room_id)?;

    // We need to return the state prior to joining, let's keep a reference to that here
    let frame_id = room::get_frame_id(room_id, None)?;

    // We do not add the event_id field to the pdu here because of signature and hashes checks
    let room_version_id = room::get_version(room_id)?;

     println!("==========send join v1==========2");
    let (event_id, mut value) = gen_event_id_canonical_json(pdu, &room_version_id)
        .map_err(|_| MatrixError::invalid_param("Could not convert event to canonical json."))?;

    let event_room_id: OwnedRoomId = serde_json::from_value(
        value
            .get("room_id")
            .ok_or_else(|| MatrixError::bad_json("Event missing room_id property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("room_id field is not a valid room ID: {e}")))?;

     println!("==========send join v1==========3");
    if event_room_id != room_id {
        return Err(MatrixError::bad_json("Event room_id does not match request path room ID.").into());
    }

    let event_type: StateEventType = serde_json::from_value(
        value
            .get("type")
            .ok_or_else(|| MatrixError::bad_json("Event missing type property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("Event has invalid state event type: {e}")))?;

    if event_type != StateEventType::RoomMember {
        return Err(MatrixError::bad_json("Not allowed to send non-membership state event to join endpoint.").into());
    }

     println!("==========send join v1==========4");
    let content: RoomMemberEventContent = serde_json::from_value(
        value
            .get("content")
            .ok_or_else(|| MatrixError::bad_json("Event missing content property"))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("Event content is empty or invalid: {e}")))?;

    if content.membership != MembershipState::Join {
        return Err(MatrixError::bad_json("Not allowed to send a non-join membership event to join endpoint.").into());
    }

     println!("==========send join v1==========5");
    // ACL check sender user server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::bad_json("Event missing sender property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("sender property is not a valid user ID: {e}")))?;

    if room::user::is_banned(&sender, room_id)? {
        return Err(MatrixError::forbidden("User is banned from the room.", None).into());
    }

     println!("==========send join v1==========6");
    crate::event::handler::acl_check(sender.server_name(), room_id)?;

    // check if origin server is trying to send for another server
    if sender.server_name() != origin {
        return Err(MatrixError::forbidden("Not allowed to join on behalf of another server.", None).into());
    }

     println!("==========send join v1==========7");
    let state_key: OwnedUserId = serde_json::from_value(
        value
            .get("state_key")
            .ok_or_else(|| MatrixError::bad_json("Event missing state_key property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json(format!("State key is not a valid user ID: {e}")))?;
    if state_key != sender {
        return Err(MatrixError::bad_json("State key does not match sender user.").into());
    };

     println!("==========send join v1==========8");
    if let Some(authorising_user) = content.join_authorized_via_users_server {
        use crate::core::RoomVersionId::*;

        if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
            return Err(MatrixError::invalid_param(
                "Room version {room_version_id} does not support restricted rooms but \
				 join_authorised_via_users_server ({authorising_user}) was found in the event.",
            )
            .into());
        }

        if !authorising_user.is_local() {
            return Err(MatrixError::invalid_param(
                "Cannot authorise membership event through {authorising_user} as they do not \
				 belong to this homeserver",
            )
            .into());
        }

        if !room::user::is_joined(&authorising_user, room_id)? {
            println!(
                "aaaaaaaaaaaaaaaaAuthorising user {authorising_user} is not joined to room {room_id}, cannot authorise join."
            );
            return Err(MatrixError::invalid_param(
                "Authorising user {authorising_user} is not in the room you are trying to join, \
				 they cannot authorise your join.",
            )
            .into());
        }

        if !crate::federation::user_can_perform_restricted_join(&state_key, room_id, &room_version_id, None).await? {
            return Err(
                MatrixError::unable_to_authorize_join("Joining user did not pass restricted room's rules.").into(),
            );
        }
    }

     println!("==========send join v1==========9");
    crate::server_key::hash_and_sign_event(&mut value, &room_version_id)
        .map_err(|e| MatrixError::invalid_param(format!("Failed to sign send_join event: {e}")))?;

    let origin: OwnedServerName = serde_json::from_value(
        serde_json::to_value(
            value
                .get("origin")
                .ok_or(MatrixError::invalid_param("Event needs an origin field."))?,
        )
        .expect("CanonicalJson is valid json value"),
    )
    .map_err(|_| MatrixError::invalid_param("Origin field is invalid."))?;

     println!("==========send join v1==========10");
    let state_lock = room::lock_state(&room_id).await;
    crate::event::handler::process_incoming_pdu(&origin, &event_id, room_id, &room_version_id, value.clone(), true)
        .await?;
    drop(state_lock);

     println!("==========send join v1==========11");
    let state_ids = state::get_full_state_ids(frame_id)?;
    let state = state_ids
        .iter()
        .filter_map(|(_, id)| timeline::get_pdu_json(id).ok().flatten())
        .map(crate::sending::convert_to_outgoing_federation_event)
        .collect();

     println!("==========send join v1==========12");
    let auth_chain_ids = room::auth_chain::get_auth_chain_ids(room_id, state_ids.values().map(|id| &**id))?;
    let auth_chain = auth_chain_ids
        .into_iter()
        .filter_map(|id| timeline::get_pdu_json(&id).ok().flatten())
        .map(crate::sending::convert_to_outgoing_federation_event)
        .collect();
    crate::sending::send_pdu_room(room_id, &event_id)?;

     println!("==========send join v1==========13");
    Ok(RoomStateV1 {
        auth_chain,
        state,
        event: to_raw_json_value(&CanonicalJsonValue::Object(value)).ok(),
        // event: None,
    })
}
pub async fn send_join_v2(origin: &ServerName, room_id: &RoomId, pdu: &RawJsonValue) -> AppResult<RoomStateV2> {
    // let sender_servername = body.sender_servername.as_ref().expect("server is authenticated");

    let RoomStateV1 {
        auth_chain,
        state,
        event,
    } = send_join_v1(origin, room_id, pdu).await?;
    let room_state = RoomStateV2 {
        members_omitted: false,
        auth_chain,
        state,
        event,
        servers_in_room: None,
    };

    Ok(room_state)
}
