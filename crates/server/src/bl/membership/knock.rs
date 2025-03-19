use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::once;
use std::sync::Arc;
use std::time::Duration;

use diesel::prelude::*;
use palpo_core::appservice::{event, third_party};
use palpo_core::events::key::verification::request;
use palpo_core::federation::knock::{
    MakeKnockResBody, SendKnockReqArgs, SendKnockReqBody, SendKnockResBody, send_knock_request,
};
use palpo_core::federation::room;
use salvo::http::StatusError;
use tokio::sync::RwLock;

use crate::appservice::RegistrationInfo;
use crate::core::client::membership::{JoinRoomResBody, ThirdPartySigned};
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::knock::MakeKnockReqArgs;
use crate::core::federation::membership::{
    InviteUserResBodyV2, MakeJoinReqArgs, MakeLeaveResBody, SendJoinArgs, SendJoinResBodyV2, SendLeaveReqBody,
    make_leave_request,
};
use crate::core::identifiers::*;
use crate::core::serde::{
    CanonicalJsonObject, CanonicalJsonValue, RawJsonValue, to_canonical_value, to_raw_json_value,
};
use crate::core::{UnixMillis, federation};
use crate::event::{
    DbEventData, NewDbEvent, PduBuilder, PduEvent, ensure_event_sn, gen_event_id, gen_event_id_canonical_json,
    get_event_sn,
};
use crate::federation::maybe_strip_event_id;
use crate::membership::federation::membership::{
    InviteUserReqArgs, InviteUserReqBodyV2, MakeJoinResBody, RoomStateV1, RoomStateV2, SendJoinReqBody,
    SendLeaveReqArgsV2, send_leave_request_v2,
};
use crate::membership::state::{CompressedState, DeltaInfo};
use crate::room::state::{self, CompressedEvent};
use crate::schema::*;
use crate::user::DbUser;
use crate::{AppError, AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError, Seqnum, SigningKeys, db, diesel_exists};

pub async fn knock_room_by_id(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
) -> AppResult<()> {
    // let state_lock = services.rooms.state.mutex.lock(room_id).await;

    if crate::room::is_invited(sender_id, room_id)? {
        warn!("{sender_id} is already invited in {room_id} but attempted to knock");
        return Err(MatrixError::forbidden("You cannot knock on a room you are already invited/accepted to.").into());
    }

    if crate::room::is_joined(sender_id, room_id)? {
        warn!("{sender_id} is already joined in {room_id} but attempted to knock");
        return Err(MatrixError::forbidden("You cannot knock on a room you are already joined in.").into());
    }

    if crate::room::is_knocked(sender_id, room_id)? {
        warn!("{sender_id} is already knocked in {room_id}");
        return Ok(());
    }

    if let Ok(Some(memeber)) = state::get_member(room_id, sender_id) {
        if memeber.membership == MembershipState::Ban {
            warn!("{sender_id} is banned from {room_id} but attempted to knock");
            return Err(MatrixError::forbidden("You cannot knock on a room you are banned from.").into());
        }
    }

    let server_in_room = crate::room::is_server_in_room(crate::server_name(), room_id)?;
    let local_knock = server_in_room || servers.is_empty() || (servers.len() == 1 && servers[0].is_local());

    if local_knock {
        knock_room_local(sender_id, room_id, reason, servers).await?;
    } else {
        knock_room_remote(sender_id, room_id, reason, servers).await?;
    }

    Ok(())
}

async fn knock_room_local(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
) -> AppResult<()> {
    info!("We can knock locally");
    println!("We can knock locally");

    let room_version_id = crate::room::state::get_room_version(room_id)?;

    if matches!(
        room_version_id,
        RoomVersionId::V1
            | RoomVersionId::V2
            | RoomVersionId::V3
            | RoomVersionId::V4
            | RoomVersionId::V5
            | RoomVersionId::V6
    ) {
        return Err(MatrixError::forbidden("This room does not support knocking.").into());
    }

    let content = RoomMemberEventContent {
        display_name: crate::user::display_name(sender_id)?,
        avatar_url: crate::user::avatar_url(sender_id)?,
        blurhash: crate::user::blurhash(sender_id)?,
        reason: reason.clone(),
        ..RoomMemberEventContent::new(MembershipState::Knock)
    };

    // Try normal knock first
    let Err(error) = crate::room::timeline::build_and_append_pdu(
        PduBuilder::state(sender_id.to_string(), &content),
        sender_id,
        room_id,
    ) else {
        return Ok(());
    };

    if servers.is_empty() || (servers.len() == 1 && servers[0].is_local()) {
        return Err(error);
    }

    warn!("We couldn't do the knock locally, maybe federation can help to satisfy the knock");

    let (make_knock_responseponse, remote_server) = make_knock_request(sender_id, room_id, servers).await?;

    info!("make_knock finished");

    let room_version_id = make_knock_responseponse.room_version;

    if !crate::supports_room_version(&room_version_id) {
        return Err(
            MatrixError::forbidden("Remote room version {room_version_id} is not supported by conduwuit").into(),
        );
    }

    let mut knock_event_stub = serde_json::from_str::<CanonicalJsonObject>(make_knock_responseponse.event.get())
        .map_err(|e| {
            StatusError::internal_server_error()
                .brief(format!("Invalid make_knock event json received from server: {e:?}"))
        })?;

    knock_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
    );
    knock_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    knock_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            display_name: crate::user::display_name(sender_id)?,
            avatar_url: crate::user::avatar_url(sender_id)?,
            blurhash: crate::user::blurhash(sender_id)?,
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
    crate::room::update_membership(
        &event_id,
        event_sn,
        room_id,
        sender_id,
        MembershipState::Knock,
        sender_id,
        Some(send_knock_body.knock_room_state),
    )?;

    info!("Appending room knock event locally");
    crate::room::timeline::append_pdu(&parsed_knock_pdu, knock_event, once(parsed_knock_pdu.event_id.borrow()))?;

    Ok(())
}

async fn knock_room_remote(
    sender_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
) -> AppResult<()> {
    println!("Knocking {room_id} over federation.");
    info!("Knocking {room_id} over federation.");

    let (make_knock_responseponse, remote_server) = make_knock_request(sender_id, room_id, servers).await?;

    info!("make_knock finished");

    let room_version_id = make_knock_responseponse.room_version;

    if !crate::supports_room_version(&room_version_id) {
        return Err(StatusError::internal_server_error()
            .brief("Remote room version {room_version_id} is not supported by conduwuit")
            .into());
    }

    let mut knock_event_stub: CanonicalJsonObject = serde_json::from_str(make_knock_responseponse.event.get())
        .map_err(|e| {
            StatusError::internal_server_error()
                .brief(format!("Invalid make_knock event json received from server: {e:?}"))
        })?;

    knock_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
    );
    knock_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    knock_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            display_name: crate::user::display_name(sender_id)?,
            avatar_url: crate::user::avatar_url(sender_id)?,
            blurhash: crate::user::blurhash(sender_id)?,
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
    let event_sn = ensure_event_sn(room_id, &event_id)?;

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
    let parsed_knock_pdu =
        PduEvent::from_canonical_object(&event_id, ensure_event_sn(&room_id, &event_id)?, knock_event.clone())
            .map_err(|e| StatusError::internal_server_error().brief(format!("Invalid knock event PDU: {e:?}")))?;

    info!("Going through send_knock response knock state events");
    let state = send_knock_body
        .knock_room_state
        .iter()
        .map(|event| serde_json::from_str::<CanonicalJsonObject>(event.clone().into_inner().get()))
        .filter_map(Result::ok);

    let mut state_map: HashMap<i64, Seqnum> = HashMap::new();

    for event in state {
        let Some(state_key) = event.get("state_key") else {
            warn!("send_knock stripped state event missing state_key: {event:?}");
            continue;
        };
        let Some(event_type) = event.get("type") else {
            warn!("send_knock stripped state event missing event type: {event:?}");
            continue;
        };

        let Ok(state_key) = serde_json::from_value::<String>(state_key.clone().into()) else {
            warn!("send_knock stripped state event has invalid state_key: {event:?}");
            continue;
        };
        let Ok(event_type) = serde_json::from_value::<StateEventType>(event_type.clone().into()) else {
            warn!("send_knock stripped state event has invalid event type: {event:?}");
            continue;
        };

        let event_id = gen_event_id(&event, &room_version_id)?;
        let event_sn = ensure_event_sn(room_id, &event_id)?;
        let new_db_event = NewDbEvent {
            id: event_id.clone(),
            sn: event_sn,
            ty: MembershipState::Leave.to_string(),
            room_id: room_id.to_owned(),
            unrecognized_keys: None,
            depth: 0,
            origin_server_ts: Some(UnixMillis::now()),
            received_at: None,
            sender_id: Some(sender_id.to_owned()),
            contains_url: false,
            worker_id: None,
            state_key: Some(sender_id.to_string()),
            is_outlier: true,
            soft_failed: false,
            rejection_reason: None,
        };
        diesel::insert_into(events::table)
            .values(&new_db_event)
            .on_conflict_do_nothing()
            .returning(events::sn)
            .get_result::<Seqnum>(&mut *db::connect()?)?;
        let event_data = DbEventData {
            event_id: event_id.clone(),
            event_sn,
            room_id: room_id.to_owned(),
            internal_metadata: None,
            json_data: serde_json::to_value(&event)?,
            format_version: None,
        };
        diesel::insert_into(event_datas::table)
            .values(&event_data)
            .on_conflict_do_nothing()
            .execute(&mut db::connect()?)?;

        let field_id = crate::room::state::ensure_field_id(&event_type, &state_key)?;
        state_map.insert(field_id, event_sn);
    }

    info!("Compressing state from send_knock");
    let compressed = state::compress_events(room_id, state_map.into_iter())?;

    debug!("Saving compressed state");
    let delta = state::save_state(room_id, Arc::new(compressed))?;

    info!("Updating membership locally to knock state with provided stripped state events");
    crate::room::update_membership(
        &event_id,
        event_sn,
        room_id,
        sender_id,
        MembershipState::Knock,
        sender_id,
        Some(send_knock_body.knock_room_state),
    )?;

    info!("Appending room knock event locally");
    crate::room::timeline::append_pdu(&parsed_knock_pdu, knock_event, once(parsed_knock_pdu.event_id.borrow()))?;

    info!("Setting final room state for new room");
    // We set the room state after inserting the pdu, so that we never have a moment
    // in time where events in the current room state do not exist
    state::set_room_state(room_id, delta.frame_id);

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

        let request = crate::core::federation::knock::make_knock_request(
            &remote_server.origin().await,
            MakeKnockReqArgs {
                room_id: room_id.to_owned(),
                user_id: sender_id.to_owned(),
                ver: crate::supported_room_versions(),
            },
        )?
        .into_inner();

        let make_knock_response = crate::sending::send_federation_request(remote_server, request)
            .await?
            .json::<MakeKnockResBody>()
            .await.map_err(Into::into);

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
