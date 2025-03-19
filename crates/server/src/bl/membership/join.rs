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
use crate::core::{UnixMillis, federation};

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
use crate::{AppError, AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError, Seqnum, SigningKeys, db, diesel_exists};

pub async fn send_join_v1(origin: &ServerName, room_id: &RoomId, pdu: &RawJsonValue) -> AppResult<RoomStateV1> {
    if !crate::room::room_exists(room_id)? {
        return Err(MatrixError::not_found("Room is unknown to this server.").into());
    }

    crate::event::handler::acl_check(origin, room_id)?;

    // We need to return the state prior to joining, let's keep a reference to that here
    let frame_id =
        crate::room::state::get_room_frame_id(room_id, None)?.ok_or(MatrixError::not_found("Pdu state not found."))?;

    // We do not add the event_id field to the pdu here because of signature and hashes checks
    let room_version_id = crate::room::state::get_room_version(room_id)?;

    let (event_id, mut value) = match gen_event_id_canonical_json(pdu, &room_version_id) {
        Ok(t) => t,
        Err(_) => {
            // Event could not be converted to canonical json
            return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
        }
    };

    let event_room_id: OwnedRoomId = serde_json::from_value(
        value
            .get("room_id")
            .ok_or_else(|| MatrixError::bad_json("Event missing room_id property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json("room_id field is not a valid room ID: {e}"))?;

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
    .map_err(|e| MatrixError::bad_json("Event has invalid state event type: {e}"))?;

    if event_type != StateEventType::RoomMember {
        return Err(MatrixError::bad_json("Not allowed to send non-membership state event to join endpoint.").into());
    }

    let content: RoomMemberEventContent = serde_json::from_value(
        value
            .get("content")
            .ok_or_else(|| MatrixError::bad_json("Event missing content property"))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json("Event content is empty or invalid: {e}"))?;

    if content.membership != MembershipState::Join {
        return Err(MatrixError::bad_json("Not allowed to send a non-join membership event to join endpoint.").into());
    }

    // ACL check sender user server name
    let sender: OwnedUserId = serde_json::from_value(
        value
            .get("sender")
            .ok_or_else(|| MatrixError::bad_json("Event missing sender property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json("sender property is not a valid user ID: {e}"))?;

    crate::event::handler::acl_check(sender.server_name(), room_id)?;

    // check if origin server is trying to send for another server
    if sender.server_name() != origin {
        return Err(MatrixError::forbidden("Not allowed to join on behalf of another server.").into());
    }

    let state_key: OwnedUserId = serde_json::from_value(
        value
            .get("state_key")
            .ok_or_else(|| MatrixError::bad_json("Event missing state_key property."))?
            .clone()
            .into(),
    )
    .map_err(|e| MatrixError::bad_json("State key is not a valid user ID: {e}"))?;
    if state_key != sender {
        return Err(MatrixError::bad_json("State key does not match sender user.").into());
    };

    if let Some(authorising_user) = content.join_authorized_via_users_server {
        use crate::core::RoomVersionId::*;

        if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
            return Err(MatrixError::invalid_param(
                "Room version {room_version_id} does not support restricted rooms but \
				 join_authorised_via_users_server ({authorising_user}) was found in the event.",
            )
            .into());
        }

        if !crate::user::is_local(&authorising_user) {
            return Err(MatrixError::invalid_param(
                "Cannot authorise membership event through {authorising_user} as they do not \
				 belong to this homeserver",
            )
            .into());
        }

        if !crate::room::is_joined(&authorising_user, room_id)? {
            return Err(MatrixError::invalid_param(
                "Authorising user {authorising_user} is not in the room you are trying to join, \
				 they cannot authorise your join.",
            )
            .into());
        }

        if !crate::federation::user_can_perform_restricted_join(&state_key, room_id, &room_version_id).await? {
            return Err(
                MatrixError::unable_to_authorize_join("Joining user did not pass restricted room's rules.").into(),
            );
        }
    }

    crate::server_key::hash_and_sign_event(&mut value, &room_version_id)
        .map_err(|e| MatrixError::invalid_param("Failed to sign send_join event: {e}"))?;

    let origin: OwnedServerName = serde_json::from_value(
        serde_json::to_value(
            value
                .get("origin")
                .ok_or(MatrixError::invalid_param("Event needs an origin field."))?,
        )
        .expect("CanonicalJson is valid json value"),
    )
    .map_err(|_| MatrixError::invalid_param("Origin field is invalid."))?;

    // let mutex = Arc::clone(
    //     crate::ROOMID_MUTEX_FEDERATION
    //         .write()
    //         .await
    //         .entry(room_id.to_owned())
    //         .or_default(),
    // );
    // let mutex_lock = mutex.lock().await;
    println!("==ddd  handle_incoming_pdu 0  {event_id}");
    crate::event::handler::handle_incoming_pdu(&origin, &event_id, room_id, value, true).await?;
    // drop(mutex_lock);

    let state_ids = crate::room::state::get_full_state_ids(frame_id)?;

    let mut state = state_ids
        .iter()
        .filter_map(|(_, id)| crate::room::timeline::get_pdu_json(id).ok().flatten())
        .map(crate::sending::convert_to_outgoing_federation_event)
        .collect();

    let auth_chain_ids = crate::room::auth_chain::get_auth_chain_ids(room_id, state_ids.values().map(|id| &**id))?;
    let auth_chain = auth_chain_ids
        .into_iter()
        .filter_map(|id| crate::room::timeline::get_pdu_json(&id).ok().flatten())
        .map(crate::sending::convert_to_outgoing_federation_event)
        .collect();

    //     let join_rules_event = crate::room::state::get_state(room_id, &StateEventType::RoomJoinRules, "", None)?;

    //     let join_rules_event_content: Option<RoomJoinRulesEventContent> = join_rules_event
    //         .as_ref()
    //         .map(|join_rules_event| {
    //             serde_json::from_str(join_rules_event.content.get()).map_err(|e| {
    //                 warn!("Invalid join rules event: {}", e);
    //                 AppError::public("Invalid join rules event in db.")
    //             })
    //         })
    //         .transpose()?;

    //     if let Some(join_rules_event_content) = join_rules_event_content {
    //         if matches!(
    //             join_rules_event_content.join_rule,
    //             JoinRule::Restricted { .. } | JoinRule::KnockRestricted { .. }
    //         ) {
    //             return Err(MatrixError::unable_to_authorize_join("Palpo does not support restricted rooms yet.").into());
    //         }
    //     }

    //     // let pub_key_map = RwLock::new(BTreeMap::new());
    //     // let mut auth_cache = EventMap::new();

    crate::sending::send_pdu_room(room_id, &event_id)?;
    Ok(RoomStateV1 {
        auth_chain,
        state,
        event: None, // TODO: handle restricted joins
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
pub async fn join_room(
    user: &DbUser,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    third_party_signed: Option<&ThirdPartySigned>,
    appservice: Option<&RegistrationInfo>,
) -> AppResult<JoinRoomResBody> {
    // TODO: state lock
    if user.is_guest && appservice.is_none() && !crate::room::state::guest_can_join(room_id)? {
        return Err(MatrixError::forbidden("Guests are not allowed to join this room").into());
    }

    if crate::room::is_joined(&user.id, room_id)? {
        return Ok(JoinRoomResBody {
            room_id: room_id.into(),
        });
    }

    let local_join = crate::room::is_server_in_room(crate::server_name(), room_id)?
        || servers.is_empty()
        || (servers.len() == 1 && servers[0] == crate::server_name());
    // Ask a remote server if we are not participating in this room
    if local_join {
        local_join_room(&user.id, room_id, reason, servers, third_party_signed).await?;
    } else {
        remote_join_room(&user.id, room_id, reason, servers, third_party_signed).await?;
    }

    println!("IIIIIIIIIIIIIIII====7");
    Ok(JoinRoomResBody::new(room_id.to_owned()))
}

async fn local_join_room(
    user_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    _third_party_signed: Option<&ThirdPartySigned>,
) -> AppResult<()> {
    println!("================join local");
    info!("We can join locally");
    let join_rules_event = crate::room::state::get_room_state(room_id, &StateEventType::RoomJoinRules, "")?;
    // let power_levels_event = crate::room::state::get_state(room_id, &StateEventType::RoomPowerLevels, "", None)?;

    let join_rules_event_content: Option<RoomJoinRulesEventContent> = join_rules_event
        .as_ref()
        .map(|join_rules_event| {
            serde_json::from_str(join_rules_event.content.get()).map_err(|e| {
                warn!("Invalid join rules event: {}", e);
                AppError::public("Invalid join rules event in database.")
            })
        })
        .transpose()?;

    let restriction_rooms = match join_rules_event_content {
        Some(RoomJoinRulesEventContent {
            join_rule: JoinRule::Restricted(restricted),
        })
        | Some(RoomJoinRulesEventContent {
            join_rule: JoinRule::KnockRestricted(restricted),
        }) => restricted
            .allow
            .into_iter()
            .filter_map(|a| match a {
                AllowRule::RoomMembership(r) => Some(r.room_id),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    let authorized_user = if restriction_rooms
        .iter()
        .any(|restriction_room_id| crate::room::is_joined(user_id, restriction_room_id).unwrap_or(false))
    {
        let mut auth_user = None;
        for joined_user in crate::room::get_joined_users(room_id, None)? {
            if joined_user.server_name() == crate::server_name()
                && state::user_can_invite(room_id, &joined_user, user_id).unwrap_or(false)
            {
                auth_user = Some(joined_user);
                break;
            }
        }
        auth_user
    } else {
        None
    };

    let event = RoomMemberEventContent {
        membership: MembershipState::Join,
        display_name: crate::user::display_name(user_id)?,
        avatar_url: crate::user::avatar_url(user_id)?,
        is_direct: None,
        third_party_invite: None,
        blurhash: crate::user::blurhash(user_id)?,
        reason: reason.clone(),
        join_authorized_via_users_server: authorized_user,
    };

    // Try normal join first
    let error = match crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content: to_raw_json_value(&event).expect("event is valid, we just created it"),
            state_key: Some(user_id.to_string()),
            ..Default::default()
        },
        user_id,
        room_id,
    ) {
        Ok(_event_id) => return Ok(()),
        Err(e) => e,
    };

    if !restriction_rooms.is_empty() && servers.iter().filter(|s| *s != crate::server_name()).count() > 0 {
        info!("We couldn't do the join locally, maybe federation can help to satisfy the restricted join requirements");
        let (make_join_response, remote_server) = make_join_request(user_id, room_id, servers).await?;

        let room_version_id = match make_join_response.room_version {
            Some(room_version_id) if crate::supported_room_versions().contains(&room_version_id) => room_version_id,
            _ => return Err(AppError::public("Room version is not supported")),
        };
        let mut join_event_stub: CanonicalJsonObject = serde_json::from_str(make_join_response.event.get())
            .map_err(|_| AppError::public("Invalid make_join event json received from server."))?;
        let join_authorized_via_users_server = join_event_stub
            .get("content")
            .map(|s| s.as_object()?.get("join_authorised_via_users_server")?.as_str())
            .and_then(|s| OwnedUserId::try_from(s.unwrap_or_default()).ok());
        // TODO: Is origin needed?
        join_event_stub.insert(
            "origin".to_owned(),
            CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
        );
        join_event_stub.insert(
            "origin_server_ts".to_owned(),
            CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
        );
        join_event_stub.insert(
            "content".to_owned(),
            to_canonical_value(RoomMemberEventContent {
                membership: MembershipState::Join,
                display_name: crate::user::display_name(user_id)?,
                avatar_url: crate::user::avatar_url(user_id)?,
                is_direct: None,
                third_party_invite: None,
                blurhash: crate::user::blurhash(user_id)?,
                reason,
                join_authorized_via_users_server,
            })
            .expect("event is valid, we just created it"),
        );

        // We don't leave the event id in the pdu because that's only allowed in v1 or v2 rooms
        join_event_stub.remove("event_id");

        // In order to create a compatible ref hash (EventID) the `hashes` field needs to be present
        crate::server_key::hash_and_sign_event(&mut join_event_stub, &room_version_id)
            .expect("event is valid, we just created it");

        // Generate event id
        let event_id = crate::event::gen_event_id(&join_event_stub, &room_version_id)?;

        // Add event_id back
        join_event_stub.insert(
            "event_id".to_owned(),
            CanonicalJsonValue::String(event_id.as_str().to_owned()),
        );

        // It has enough fields to be called a proper event now
        let join_event = join_event_stub;

        let send_join_request = crate::core::federation::membership::send_join_request(
            &room_id.server_name().map_err(AppError::public)?.origin().await,
            SendJoinArgs {
                room_id: room_id.to_owned(),
                event_id: event_id.to_owned(),
                omit_members: false,
            },
            SendJoinReqBody(crate::sending::convert_to_outgoing_federation_event(join_event.clone())),
        )?
        .into_inner();

        let send_join_response = crate::sending::send_federation_request(&remote_server, send_join_request)
            .await?
            .json::<SendJoinResBodyV2>()
            .await?;

        if let Some(signed_raw) = send_join_response.0.event {
            let (signed_event_id, signed_value) = match gen_event_id_canonical_json(&signed_raw, &room_version_id) {
                Ok(t) => t,
                Err(_) => {
                    // Event could not be converted to canonical json
                    return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
                }
            };

            if signed_event_id != event_id {
                return Err(MatrixError::invalid_param("Server sent event with wrong event id").into());
            }

            println!("==ddd  handle_incoming_pdu 2  {event_id}");
            // let pub_key_map = RwLock::new(BTreeMap::new());
            crate::event::handler::handle_incoming_pdu(
                &remote_server,
                &signed_event_id,
                room_id,
                signed_value,
                true,
                // &pub_key_map,
            )
            .await?;
        } else {
            return Err(error);
        }
    } else {
        return Err(error);
    }
    Ok(())
}

async fn remote_join_room(
    user_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    _third_party_signed: Option<&ThirdPartySigned>,
) -> AppResult<()> {
    println!("================join over federation");
    info!("Joining {room_id} over federation.");

    let (make_join_response, remote_server) = make_join_request(user_id, room_id, servers).await?;

    info!("make_join finished");

    let room_version_id = match make_join_response.room_version {
        Some(room_version) if crate::supported_room_versions().contains(&room_version) => room_version,
        _ => return Err(AppError::public("Room version is not supported")),
    };

    let mut join_event_stub: CanonicalJsonObject = serde_json::from_str(make_join_response.event.get())
        .map_err(|_| AppError::public("Invalid make_join event json received from server."))?;

    let join_authorized_via_users_server = join_event_stub
        .get("content")
        .map(|s| s.as_object()?.get("join_authorised_via_users_server")?.as_str())
        .and_then(|s| OwnedUserId::try_from(s.unwrap_or_default()).ok());

    // TODO: Is origin needed?
    join_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
    );
    join_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    join_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            membership: MembershipState::Join,
            display_name: crate::user::display_name(user_id)?,
            avatar_url: crate::user::avatar_url(user_id)?,
            is_direct: None,
            third_party_invite: None,
            blurhash: crate::user::blurhash(user_id)?,
            reason,
            join_authorized_via_users_server,
        })
        .expect("event is valid, we just created it"),
    );

    // We keep the "event_id" in the pdu only in v1 or v2 rooms
    maybe_strip_event_id(&mut join_event_stub, &room_version_id);

    // In order to create a compatible ref hash (EventID) the `hashes` field needs to be present
    crate::server_key::hash_and_sign_event(&mut join_event_stub, &room_version_id)
        .expect("event is valid, we just created it");

    // Generate event id
    let event_id = crate::event::gen_event_id(&join_event_stub, &room_version_id)?;

    // Add event_id back
    join_event_stub.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(event_id.as_str().to_owned()),
    );

    // It has enough fields to be called a proper event now
    let mut join_event = join_event_stub;

    let body = SendJoinReqBody(crate::sending::convert_to_outgoing_federation_event(join_event.clone()));
    info!("Asking {remote_server} for send_join");
    let send_join_request = crate::core::federation::membership::send_join_request(
        &remote_server.origin().await,
        SendJoinArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
            omit_members: false,
        },
        body,
    )?
    .into_inner();

    let send_join_response = crate::sending::send_federation_request(&remote_server, send_join_request)
        .await?
        .json::<SendJoinResBodyV2>()
        .await?;

    info!("send_join finished");

    if let Some(signed_raw) = &send_join_response.0.event {
        info!("There is a signed event. This room is probably using restricted joins. Adding signature to our event");
        let (signed_event_id, signed_value) = match gen_event_id_canonical_json(signed_raw, &room_version_id) {
            Ok(t) => t,
            Err(_) => {
                // Event could not be converted to canonical json
                return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
            }
        };

        if signed_event_id != event_id {
            return Err(MatrixError::invalid_param("Server sent event with wrong event id").into());
        }

        match signed_value["signatures"]
            .as_object()
            .ok_or(MatrixError::invalid_param("Server sent invalid signatures type"))
            .and_then(|e| {
                e.get(remote_server.as_str())
                    .ok_or(MatrixError::invalid_param("Server did not send its signature"))
            }) {
            Ok(signature) => {
                join_event
                    .get_mut("signatures")
                    .expect("we created a valid pdu")
                    .as_object_mut()
                    .expect("we created a valid pdu")
                    .insert(remote_server.to_string(), signature.clone());
            }
            Err(e) => {
                warn!(
                    "Server {remote_server} sent invalid signature in sendjoin signatures for event {signed_value:?}: {e:?}",
                );
            }
        }
    }

    crate::room::ensure_room(room_id, &room_version_id)?;

    info!("Parsing join event");
    let parsed_join_pdu =
        PduEvent::from_canonical_object(&event_id, crate::event::ensure_event_sn(room_id, &event_id)?, join_event.clone())
            .map_err(|e| {
                warn!("Invalid PDU in send_join response: {}", e);
                AppError::public("Invalid join event PDU.")
            })?;
    diesel::insert_into(events::table)
        .values(NewDbEvent::from_canonical_json(
            &event_id,
            parsed_join_pdu.event_sn,
            &join_event,
        )?)
        .on_conflict_do_nothing()
        .execute(&mut *db::connect()?)?;

    let mut state = HashMap::new();
    let pub_key_map = RwLock::new(BTreeMap::new());

    info!("Acquiring server signing keys for response events");
    let resp_events = &send_join_response.0;
    let resp_state = &resp_events.state;
    let resp_auth = &resp_events.auth_chain;
    crate::server_key::acquire_events_pubkeys(resp_auth.iter().chain(resp_state.iter())).await;

    info!("Going through send_join response room_state");
    for result in send_join_response
        .0
        .state
        .iter()
        .map(|pdu| super::validate_and_add_event_id(pdu, &room_version_id, &pub_key_map))
    {
        let (event_id, value) = match result.await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let pdu = if let Some(pdu) = crate::room::timeline::get_pdu(&event_id)? {
            pdu
        } else {
            let pdu = PduEvent::from_canonical_object(&event_id, crate::event::ensure_event_sn(room_id, &event_id)?, value.clone())
                .map_err(|e| {
                    warn!("Invalid PDU in send_join response: {} {:?}", e, value);
                    AppError::public("Invalid PDU in send_join response.")
                })?;

            diesel::insert_into(events::table)
                .values(NewDbEvent::from_canonical_json(&event_id, pdu.event_sn, &value)?)
                .on_conflict_do_nothing()
                .execute(&mut *db::connect()?)?;

            let event_data = DbEventData {
                event_id: pdu.event_id.to_owned().into(),
                event_sn: pdu.event_sn,
                room_id: pdu.room_id.clone(),
                internal_metadata: None,
                json_data: serde_json::to_value(&value)?,
                format_version: None,
            };
            diesel::insert_into(event_datas::table)
                .values(&event_data)
                .on_conflict((event_datas::event_id, event_datas::event_sn))
                .do_update()
                .set(&event_data)
                .execute(&mut db::connect()?)?;
            pdu
        };

        if let Some(state_key) = &pdu.state_key {
            let state_key_id = crate::room::state::ensure_field_id(&pdu.event_ty.to_string().into(), state_key)?;
            state.insert(state_key_id, (pdu.event_id.clone(), pdu.event_sn));
        }
    }

    info!("Going through send_join response auth_chain");
    for result in send_join_response
        .0
        .auth_chain
        .iter()
        .map(|pdu| super::validate_and_add_event_id(pdu, &room_version_id, &pub_key_map))
    {
        let (event_id, value) = match result.await {
            Ok(t) => t,
            Err(_) => continue,
        };

        if !crate::room::timeline::has_pdu(&event_id)? {
            let event_sn = crate::event::ensure_event_sn(room_id, &event_id)?;
            let db_event = NewDbEvent::from_canonical_json(&event_id, event_sn, &value)?;
            diesel::insert_into(events::table)
                .values(&db_event)
                .on_conflict_do_nothing()
                .execute(&mut *db::connect()?)?;
            let event_data = DbEventData {
                event_id: event_id.to_owned(),
                event_sn,
                room_id: db_event.room_id.clone(),
                internal_metadata: None,
                json_data: serde_json::to_value(&value)?,
                format_version: None,
            };

            diesel::insert_into(event_datas::table)
                .values(&event_data)
                .on_conflict((event_datas::event_id, event_datas::event_sn))
                .do_update()
                .set(&event_data)
                .execute(&mut db::connect()?)?;
        }
    }

    info!("Running send_join auth check");
    // TODO: Authcheck
    // if !event_auth::auth_check(
    //     &RoomVersion::new(&room_version_id)?,
    //     &parsed_join_pdu,
    //     None::<PduEvent>, // TODO: third party invite
    //     |k, s| {
    //         crate::room::timeline::get_pdu(
    //             state.get(&crate::room::state::ensure_field_id(&k.to_string().into(), s).ok()?)?,
    //         )
    //         .ok()?
    //     },
    // )
    // .map_err(|e| {
    //     warn!("Auth check failed when running send_json auth check: {e}");
    //     MatrixError::invalid_param("Auth check failed when running send_json auth check")
    // })? {
    //     return Err(MatrixError::invalid_param("Auth check failed when running send_json auth check").into());
    // }

    // let prev_events = state.iter().map(|(_, event_id)| event_id.clone()).collect::<Vec<_>>();
    // crate::event::handler::fetch_missing_prev_events(&remote_server, room_id, &room_version_id, prev_events).await?;

    info!("Saving state from send_join");
    let DeltaInfo {
        frame_id,
        appended,
        disposed,
    } = crate::room::state::save_state(
        room_id,
        Arc::new(
            state
                .into_iter()
                .map(|(k, (event_id, event_sn))| {
                    Ok(CompressedEvent::new(k, event_sn))
                })
                .collect::<AppResult<_>>()?,
        ),
    )?;

    crate::room::state::force_state(room_id, frame_id, appended, disposed)?;

    // info!("Updating joined counts for new room");
    // crate::room::update_room_servers(room_id)?;
    // crate::room::update_room_currents(room_id)?;

    // We append to state before appending the pdu, so we don't have a moment in time with the
    // pdu without it's state. This is okay because append_pdu can't fail.
    let frame_id_after_join = crate::room::state::append_to_state(&parsed_join_pdu)?;

    info!("Appending new room join event");
    crate::room::timeline::append_pdu(&parsed_join_pdu, join_event, once(parsed_join_pdu.event_id.borrow())).unwrap();

    info!("Setting final room state for new room");
    // We set the room state after inserting the pdu, so that we never have a moment in time
    // where events in the current room state do not exist
    crate::room::state::set_room_state(room_id, frame_id_after_join)?;
    Ok(())
}

async fn make_join_request(
    user_id: &UserId,
    room_id: &RoomId,
    servers: &[OwnedServerName],
) -> AppResult<(MakeJoinResBody, OwnedServerName)> {
    let mut make_join_res_body_and_server = Err(StatusError::bad_request()
        .brief("No server available to assist in joining.")
        .into());

    for remote_server in servers {
        if remote_server == crate::server_name() {
            continue;
        }
        info!("Asking {remote_server} for make_join");

        let make_join_request = crate::core::federation::membership::make_join_request(
            &remote_server.origin().await,
            MakeJoinReqArgs {
                room_id: room_id.to_owned(),
                user_id: user_id.to_owned(),
                ver: crate::supported_room_versions(),
            },
        )?
        .into_inner();
        let make_join_response = crate::sending::send_federation_request(remote_server, make_join_request).await;
        match make_join_response {
            Ok(make_join_response) => {
                let res_body = make_join_response.json::<MakeJoinResBody>().await;
                make_join_res_body_and_server = res_body.map(|r| (r, remote_server.clone())).map_err(Into::into);
            }
            Err(e) => {
                tracing::error!("make_join_request failed: {e:?}");
            }
        }

        if make_join_res_body_and_server.is_ok() {
            break;
        }
    }

    make_join_res_body_and_server
}
