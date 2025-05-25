use std::borrow::Borrow;
use std::collections::HashMap;
use std::iter::once;
use std::sync::Arc;

use salvo::http::StatusError;

use crate::core::UnixMillis;
use crate::core::events::StateEventType;
use crate::core::events::room::join_rules::JoinRule;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::federation::event::{EventReqArgs, EventResBody, event_request};
use crate::core::federation::knock::{
    MakeKnockReqArgs, MakeKnockResBody, SendKnockReqArgs, SendKnockReqBody, SendKnockResBody, send_knock_request,
};
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, to_canonical_value};
use crate::event::{PduBuilder, PduEvent, ensure_event_sn, gen_event_id};
use crate::room::state::{CompressedEvent, DeltaInfo};
use crate::room::{self, state, timeline};
use crate::{
    AppError, AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError, OptionalExtension, RoomMutexGuard, config, data,
};

pub async fn knock_room_by_id(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
) -> AppResult<()> {
    let state_lock = room::lock_state(&room_id).await;
    if room::user::is_invited(sender_id, room_id)? {
        warn!("{sender_id} is already invited in {room_id} but attempted to knock");
        return Err(
            MatrixError::forbidden("You cannot knock on a room you are already invited/accepted to.", None).into(),
        );
    }

    if room::user::is_joined(sender_id, room_id)? {
        warn!("{sender_id} is already joined in {room_id} but attempted to knock");
        return Err(MatrixError::forbidden("You cannot knock on a room you are already joined in.", None).into());
    }

    if room::user::is_knocked(sender_id, room_id)? {
        warn!("{sender_id} is already knocked in {room_id}");
        return Ok(());
    }

    if let Ok(memeber) = room::get_member(room_id, sender_id) {
        if memeber.membership == MembershipState::Ban {
            warn!("{sender_id} is banned from {room_id} but attempted to knock");
            return Err(MatrixError::forbidden("You cannot knock on a room you are banned from.", None).into());
        }
    }

    let local_knock = room::can_local_work_for(room_id, servers)?;
    if local_knock {
        knock_room_local(sender_id, room_id, reason, servers, state_lock).await?;
    } else {
        knock_room_remote(sender_id, room_id, reason, servers, state_lock).await?;
    }

    Ok(())
}

async fn knock_room_local(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    state_lock: RoomMutexGuard,
) -> AppResult<()> {
    use RoomVersionId::*;
    info!("We can knock locally");
    let room_version_id = room::get_version(room_id)?;
    if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6) {
        return Err(MatrixError::forbidden("This room version does not support knocking.", None).into());
    }

    let join_rule = room::get_join_rule(room_id)?;
    if !matches!(
        join_rule,
        JoinRule::Invite | JoinRule::Knock | JoinRule::KnockRestricted(..)
    ) {
        return Err(MatrixError::forbidden("This room does not support knocking.", None).into());
    }

    let content = RoomMemberEventContent {
        display_name: data::user::display_name(sender_id).ok().flatten(),
        avatar_url: data::user::avatar_url(sender_id).ok().flatten(),
        blurhash: data::user::blurhash(sender_id).ok().flatten(),
        reason: reason.clone(),
        ..RoomMemberEventContent::new(MembershipState::Knock)
    };

    // Try normal knock first
    let Err(error) = timeline::build_and_append_pdu(
        PduBuilder::state(sender_id.to_string(), &content),
        sender_id,
        room_id,
        &state_lock,
    ) else {
        return Ok(());
    };
    if servers.is_empty() || (servers.len() == 1 && servers[0].is_local()) {
        return Err(error);
    }

    warn!("We couldn't do the knock locally, maybe federation can help to satisfy the knock");
    let (make_knock_body, remote_server) = make_knock_request(sender_id, room_id, servers).await?;
    info!("make_knock finished");
    let room_version_id = make_knock_body.room_version;
    if !config::supports_room_version(&room_version_id) {
        return Err(
            MatrixError::forbidden("Remote room version {room_version_id} is not supported by palpo", None).into(),
        );
    }
    let mut knock_event_stub =
        serde_json::from_str::<CanonicalJsonObject>(make_knock_body.event.get()).map_err(|e| {
            StatusError::internal_server_error()
                .brief(format!("Invalid make_knock event json received from server: {e:?}"))
        })?;

    knock_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(config::server_name().as_str().to_owned()),
    );
    knock_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    knock_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            display_name: data::user::display_name(sender_id).ok().flatten(),
            avatar_url: data::user::avatar_url(sender_id).ok().flatten(),
            blurhash: data::user::blurhash(sender_id).ok().flatten(),
            reason,
            ..RoomMemberEventContent::new(MembershipState::Knock)
        })
        .expect("event is valid, we just created it"),
    );

    // In order to create a compatible ref hash (EventID) the `hashes` field needs
    // to be present
    crate::server_key::hash_and_sign_event(&mut knock_event_stub, &room_version_id)?;

    // Generate event id
    let event_id = gen_event_id(&knock_event_stub, &room_version_id)?;

    // Add event_id
    knock_event_stub.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(event_id.clone().into()),
    );

    // It has enough fields to be called a proper event now
    let knock_event = knock_event_stub;

    info!("Asking {remote_server} for send_knock in room {room_id}");

    let request = send_knock_request(
        &remote_server.origin().await,
        SendKnockReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
        SendKnockReqBody::new(crate::sending::convert_to_outgoing_federation_event(
            knock_event.clone(),
        )),
    )?
    .into_inner();

    let send_knock_body = crate::sending::send_federation_request(&remote_server, request)
        .await?
        .json::<SendKnockResBody>()
        .await?;

    info!("send_knock finished");

    info!("Parsing knock event");
    let event_sn = crate::event::ensure_event_sn(room_id, &event_id)?;
    let parsed_knock_pdu = PduEvent::from_canonical_object(&event_id, event_sn, knock_event.clone())
        .map_err(|e| StatusError::internal_server_error().brief(format!("Invalid knock event PDU: {e:?}")))?;

    info!("Updating membership locally to knock state with provided stripped state events");
    crate::membership::update_membership(
        &event_id,
        event_sn,
        room_id,
        sender_id,
        MembershipState::Knock,
        sender_id,
        Some(send_knock_body.knock_room_state),
    )?;

    info!("Appending room knock event locally");
    timeline::append_pdu(
        &parsed_knock_pdu,
        knock_event,
        once(parsed_knock_pdu.event_id.borrow()),
        &state_lock,
    )?;

    Ok(())
}

async fn knock_room_remote(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    state_lock: RoomMutexGuard,
) -> AppResult<()> {
    info!("Knocking {room_id} over federation.");
    println!("Knocking {room_id} over federation.");

    let (make_knock_response, remote_server) = make_knock_request(sender_id, room_id, servers).await?;

    info!("make_knock finished");

    let room_version_id = make_knock_response.room_version;

    if !config::supports_room_version(&room_version_id) {
        return Err(StatusError::internal_server_error()
            .brief("Remote room version {room_version_id} is not supported by palpo")
            .into());
    }
    crate::room::ensure_room(room_id, &room_version_id)?;

    let mut knock_event_stub: CanonicalJsonObject =
        serde_json::from_str(make_knock_response.event.get()).map_err(|e| {
            StatusError::internal_server_error()
                .brief(format!("Invalid make_knock event json received from server: {e:?}"))
        })?;

    knock_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(config::server_name().as_str().to_owned()),
    );
    knock_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    knock_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            display_name: data::user::display_name(sender_id).ok().flatten(),
            avatar_url: data::user::avatar_url(sender_id).ok().flatten(),
            blurhash: data::user::blurhash(sender_id).ok().flatten(),
            reason,
            ..RoomMemberEventContent::new(MembershipState::Knock)
        })
        .expect("event is valid, we just created it"),
    );

    // In order to create a compatible ref hash (EventID) the `hashes` field needs
    // to be present
    crate::server_key::hash_and_sign_event(&mut knock_event_stub, &room_version_id)?;

    // Generate event id
    let event_id = gen_event_id(&knock_event_stub, &room_version_id)?;

    // Add event_id
    knock_event_stub.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(event_id.clone().into()),
    );

    // It has enough fields to be called a proper event now
    let knock_event = knock_event_stub;

    info!("Asking {remote_server} for send_knock in room {room_id}");
    let send_knock_request = send_knock_request(
        &remote_server.origin().await,
        SendKnockReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
        SendKnockReqBody::new(crate::sending::convert_to_outgoing_federation_event(
            knock_event.clone(),
        )),
    )?
    .into_inner();

    let send_knock_body = crate::sending::send_federation_request(&remote_server, send_knock_request)
        .await?
        .json::<SendKnockResBody>()
        .await?;

    info!("send_knock finished");

    info!("Parsing knock event");
    let event_sn = ensure_event_sn(&room_id, &event_id)?;
    let parsed_knock_pdu = PduEvent::from_canonical_object(&event_id, event_sn, knock_event.clone())
        .map_err(|e| StatusError::internal_server_error().brief(format!("Invalid knock event PDU: {e:?}")))?;

    info!("Going through send_knock response knock state events");

    // TODO: how to handle this? snpase save this state to unsigned field.
    let knock_state = send_knock_body
        .knock_room_state
        .iter()
        .map(|event| serde_json::from_str::<CanonicalJsonObject>(event.clone().into_inner().get()))
        .filter_map(Result::ok);

    let mut state_map = HashMap::new();

    for value in knock_state {
        let Some(state_key) = value.get("state_key") else {
            warn!("send_knock stripped state event missing state_key: {value:?}");
            continue;
        };
        let Some(event_type) = value.get("type") else {
            warn!("send_knock stripped state event missing event type: {value:?}");
            continue;
        };

        let Ok(state_key) = serde_json::from_value::<String>(state_key.clone().into()) else {
            warn!("send_knock stripped state event has invalid state_key: {value:?}");
            continue;
        };
        let Ok(event_type) = serde_json::from_value::<StateEventType>(event_type.clone().into()) else {
            warn!("send_knock stripped state event has invalid event type: {value:?}");
            continue;
        };

        let pdu = if let Some(pdu) = timeline::get_pdu(&event_id).optional()? {
            pdu
        } else {
            let request = event_request(&remote_server.origin().await, EventReqArgs::new(&event_id))?.into_inner();
            let res_body = crate::sending::send_federation_request(&remote_server, request)
                .await?
                .json::<EventResBody>()
                .await?;
            crate::event::handler::process_incoming_pdu(
                &remote_server,
                &event_id,
                &room_id,
                &room_version_id,
                serde_json::from_str(res_body.pdu.get())?,
                true,
            )
            .await
            .map(|_| ());
            timeline::get_pdu(&event_id)?
            // let pdu = PduEvent::from_json_value(
            //     &event_id,
            //     data::next_sn()?,
            //     serde_json::from_str::<JsonValue>(res_body.pdu.get())?,
            // )
            // .map_err(|e| {
            //     tracing::error!("Failed to parse event: {res_body:#?}");
            //     StatusError::internal_server_error().brief(format!("Invalid event json received from server: {e:?}"))
            // })?;
        };

        if let Some(state_key) = &pdu.state_key {
            let state_key_id = state::ensure_field_id(&pdu.event_ty.to_string().into(), state_key)?;
            state_map.insert(state_key_id, (pdu.event_id.clone(), pdu.event_sn));
        }
    }

    info!("Compressing state from send_knock");
    let compressed = state_map
        .into_iter()
        .map(|(k, (event_id, event_sn))| Ok(CompressedEvent::new(k, event_sn)))
        .collect::<AppResult<_>>()?;

    debug!("Saving compressed state");
    let DeltaInfo {
        frame_id,
        appended,
        disposed,
    } = state::save_state(room_id, Arc::new(compressed))?;

    debug!("Forcing state for new room");
    state::force_state(room_id, frame_id, appended, disposed)?;

    let frame_id = state::append_to_state(&parsed_knock_pdu)?;

    info!("Updating membership locally to knock state with provided stripped state events");
    crate::membership::update_membership(
        &event_id,
        event_sn,
        room_id,
        sender_id,
        MembershipState::Knock,
        sender_id,
        Some(send_knock_body.knock_room_state),
    )?;

    info!("Appending room knock event locally");
    timeline::append_pdu(
        &parsed_knock_pdu,
        knock_event,
        once(parsed_knock_pdu.event_id.borrow()),
        &state_lock,
    )?;

    info!("Setting final room state for new room");
    // We set the room state after inserting the pdu, so that we never have a moment
    // in time where events in the current room state do not exist
    state::set_room_state(room_id, frame_id);

    Ok(())
}

async fn make_knock_request(
    sender_id: &UserId,
    room_id: &RoomId,
    servers: &[OwnedServerName],
) -> AppResult<(MakeKnockResBody, OwnedServerName)> {
    let mut make_knock_response_and_server = Err(AppError::HttpStatus(
        StatusError::internal_server_error().brief("No server available to assist in knocking."),
    ));

    let mut make_knock_counter: usize = 0;

    for remote_server in servers {
        if remote_server.is_local() {
            continue;
        }

        info!("Asking {remote_server} for make_knock ({make_knock_counter})");

        println!("Asking {remote_server} for make_knock ({make_knock_counter})");
        let request = crate::core::federation::knock::make_knock_request(
            &remote_server.origin().await,
            MakeKnockReqArgs {
                room_id: room_id.to_owned(),
                user_id: sender_id.to_owned(),
                ver: config::supported_room_versions(),
            },
        )?
        .into_inner();

        let make_knock_response = crate::sending::send_federation_request(remote_server, request)
            .await?
            .json::<MakeKnockResBody>()
            .await
            .map_err(Into::into);

        trace!("make_knock response: {make_knock_response:?}");
        make_knock_counter = make_knock_counter.saturating_add(1);

        make_knock_response_and_server = make_knock_response.map(|r| (r, remote_server.clone()));

        if make_knock_response_and_server.is_ok() {
            break;
        }

        if make_knock_counter > 40 {
            warn!(
                "50 servers failed to provide valid make_knock response, assuming no server can \
				 assist in knocking."
            );
            make_knock_response_and_server = Err(StatusError::internal_server_error()
                .brief("No server available to assist in knocking.")
                .into());

            return make_knock_response_and_server;
        }
    }

    make_knock_response_and_server
}
