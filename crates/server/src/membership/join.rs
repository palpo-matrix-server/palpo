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
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent};
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

pub async fn join_room(
    authed: &AuthedInfo,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    third_party_signed: Option<&ThirdPartySigned>,
    appservice: Option<&RegistrationInfo>,
    extra_data: BTreeMap<String, JsonValue>,
) -> AppResult<JoinRoomResBody> {
    if authed.user().is_guest && appservice.is_none() && !room::guest_can_join(room_id) {
        return Err(MatrixError::forbidden("Guests are not allowed to join this room", None).into());
    }
    let sender_id = authed.user_id();
    if room::user::is_joined(sender_id, room_id)? {
        return Ok(JoinRoomResBody {
            room_id: room_id.into(),
        });
    }

    if let Ok(membership) = room::get_member(room_id, sender_id) {
        if membership.membership == MembershipState::Ban {
            tracing::warn!("{} is banned from {room_id} but attempted to join", sender_id);
            return Err(MatrixError::forbidden("You are banned from the room.", None).into());
        }
    }

    // Ask a remote server if we are not participating in this room
    if room::can_local_work_for(room_id, servers)? {
        join_room_local(sender_id, room_id, reason, servers, third_party_signed, extra_data).await?;
    } else {
        join_room_remote(authed, room_id, reason, servers, third_party_signed, extra_data).await?;
    }

    Ok(JoinRoomResBody::new(room_id.to_owned()))
}

async fn join_room_local(
    user_id: &UserId,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    _third_party_signed: Option<&ThirdPartySigned>,
    extra_data: BTreeMap<String, JsonValue>,
) -> AppResult<()> {
    info!("We can join locally");
    println!("JJJJJJJJJJJJJJJJJJJJJJJJJJJJJoin room_local: {user_id} {room_id}");
    let state_lock = room::lock_state(&room_id).await;
    let join_rules_event_content =
        room::get_state_content::<RoomJoinRulesEventContent>(room_id, &StateEventType::RoomJoinRules, "", None).ok();
    // let power_levels_event = state::get_state(room_id, &StateEventType::RoomPowerLevels, "", None)?;

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
        .any(|restriction_room_id| room::user::is_joined(user_id, restriction_room_id).unwrap_or(false))
    {
        let mut auth_user = None;
        for joined_user in room::get_joined_users(room_id, None)? {
            if joined_user.server_name() == config::server_name()
                && room::user_can_invite(room_id, &joined_user, user_id)
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
        display_name: data::user::display_name(user_id).ok().flatten(),
        avatar_url: data::user::avatar_url(user_id).ok().flatten(),
        is_direct: None,
        third_party_invite: None,
        blurhash: data::user::blurhash(user_id).ok().flatten(),
        reason: reason.clone(),
        join_authorized_via_users_server: authorized_user,
        extra_data: extra_data.clone(),
    };

    // Try normal join first
    let error = match timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content: to_raw_json_value(&event).expect("event is valid, we just created it"),
            state_key: Some(user_id.to_string()),
            ..Default::default()
        },
        user_id,
        room_id,
        &state_lock,
    ) {
        Ok(_event_id) => return Ok(()),
        Err(e) => e,
    };

    if !restriction_rooms.is_empty() && servers.iter().filter(|s| *s != config::server_name()).count() > 0 {
        info!("We couldn't do the join locally, maybe federation can help to satisfy the restricted join requirements");
        let (make_join_response, remote_server) = make_join_request(user_id, room_id, servers).await?;

        let room_version_id = match make_join_response.room_version {
            Some(room_version_id) if config::supported_room_versions().contains(&room_version_id) => room_version_id,
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
            CanonicalJsonValue::String(config::server_name().as_str().to_owned()),
        );
        join_event_stub.insert(
            "origin_server_ts".to_owned(),
            CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
        );

        join_event_stub.insert(
            "content".to_owned(),
            to_canonical_value(RoomMemberEventContent {
                membership: MembershipState::Join,
                display_name: data::user::display_name(user_id).ok().flatten(),
                avatar_url: data::user::avatar_url(user_id).ok().flatten(),
                is_direct: None,
                third_party_invite: None,
                blurhash: data::user::blurhash(user_id).ok().flatten(),
                reason,
                join_authorized_via_users_server,
                extra_data,
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

            // let pub_key_map = RwLock::new(BTreeMap::new());
            crate::event::handler::process_incoming_pdu(
                &remote_server,
                &signed_event_id,
                room_id,
                &room_version_id,
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

async fn join_room_remote(
    authed: &AuthedInfo,
    room_id: &RoomId,
    reason: Option<String>,
    servers: &[OwnedServerName],
    _third_party_signed: Option<&ThirdPartySigned>,
    extra_data: BTreeMap<String, JsonValue>,
) -> AppResult<()> {
    info!("Joining {room_id} over federation.");
    println!("JJJJJJJJJJJJJJJJJJJJJJJJJJJJJoin  {room_id} over federation.");

    let sender_id = authed.user_id();
    let (make_join_response, remote_server) = make_join_request(sender_id, room_id, servers).await?;

    info!("make_join finished");

    let room_version_id = match make_join_response.room_version {
        Some(room_version) if config::supported_room_versions().contains(&room_version) => room_version,
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
        CanonicalJsonValue::String(config::server_name().as_str().to_owned()),
    );
    join_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    join_event_stub.insert(
        "content".to_owned(),
        to_canonical_value(RoomMemberEventContent {
            membership: MembershipState::Join,
            display_name: data::user::display_name(sender_id)?,
            avatar_url: data::user::avatar_url(sender_id)?,
            is_direct: None,
            third_party_invite: None,
            blurhash: data::user::blurhash(sender_id)?,
            reason,
            join_authorized_via_users_server,
            extra_data: extra_data.clone(),
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

    let send_join_body = crate::sending::send_federation_request(&remote_server, send_join_request)
        .await?
        .json::<SendJoinResBodyV2>()
        .await?;

    info!("send_join finished");

    if let Some(signed_raw) = &send_join_body.0.event {
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

    room::ensure_room(room_id, &room_version_id)?;

    info!("Parsing join event");
    let parsed_join_pdu = PduEvent::from_canonical_object(
        &event_id,
        crate::event::ensure_event_sn(room_id, &event_id)?,
        join_event.clone(),
    )
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
        .execute(&mut connect()?)?;

    let mut state = HashMap::new();
    let pub_key_map = RwLock::new(BTreeMap::new());

    info!("Acquiring server signing keys for response events");
    let resp_events = &send_join_body.0;
    let resp_state = &resp_events.state;
    let resp_auth = &resp_events.auth_chain;
    crate::server_key::acquire_events_pubkeys(resp_auth.iter().chain(resp_state.iter())).await;

    info!("Going through send_join response room_state");
    for result in send_join_body
        .0
        .state
        .iter()
        .map(|pdu| super::validate_and_add_event_id(pdu, &room_version_id, &pub_key_map))
    {
        let (event_id, value) = match result.await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let pdu = if let Some(pdu) = timeline::get_pdu(&event_id).optional()? {
            pdu
        } else {
            let pdu = PduEvent::from_canonical_object(
                &event_id,
                crate::event::ensure_event_sn(room_id, &event_id)?,
                value.clone(),
            )
            .map_err(|e| {
                warn!("Invalid PDU in send_join response: {} {:?}", e, value);
                AppError::public("Invalid PDU in send_join response.")
            })?;

            diesel::insert_into(events::table)
                .values(NewDbEvent::from_canonical_json(&event_id, pdu.event_sn, &value)?)
                .on_conflict_do_nothing()
                .execute(&mut connect()?)?;

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
                .execute(&mut connect()?)?;
            pdu
        };

        if let Some(state_key) = &pdu.state_key {
            let state_key_id = state::ensure_field_id(&pdu.event_ty.to_string().into(), state_key)?;
            state.insert(state_key_id, (pdu.event_id.clone(), pdu.event_sn));
        }
    }

    info!("Going through send_join response auth_chain");
    for result in send_join_body
        .0
        .auth_chain
        .iter()
        .map(|pdu| super::validate_and_add_event_id(pdu, &room_version_id, &pub_key_map))
    {
        let (event_id, value) = match result.await {
            Ok(t) => t,
            Err(_) => continue,
        };

        if !timeline::has_pdu(&event_id) {
            let event_sn = crate::event::ensure_event_sn(room_id, &event_id)?;
            let db_event = NewDbEvent::from_canonical_json(&event_id, event_sn, &value)?;
            diesel::insert_into(events::table)
                .values(&db_event)
                .on_conflict_do_nothing()
                .execute(&mut connect()?)?;
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
                .execute(&mut connect()?)?;
        }
    }

    info!("Running send_join auth check");
    // TODO: Authcheck
    // if !event_auth::auth_check(
    //     &RoomVersion::new(&room_version_id)?,
    //     &parsed_join_pdu,
    //     None::<PduEvent>, // TODO: third party invite
    //     |k, s| {
    //         timeline::get_pdu(
    //             state.get(&state::ensure_field_id(&k.to_string().into(), s).ok()?)?,
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
    } = state::save_state(
        room_id,
        Arc::new(
            state
                .into_iter()
                .map(|(k, (event_id, event_sn))| Ok(CompressedEvent::new(k, event_sn)))
                .collect::<AppResult<_>>()?,
        ),
    )?;

    state::force_state(room_id, frame_id, appended, disposed)?;

    // info!("Updating joined counts for new room");
    room::update_joined_servers(room_id)?;
    // room::update_currents(room_id)?;

    // We append to state before appending the pdu, so we don't have a moment in time with the
    // pdu without it's state. This is okay because append_pdu can't fail.
    let frame_id_after_join = state::append_to_state(&parsed_join_pdu)?;

    info!("Appending new room join event");
    let state_lock = room::lock_state(&room_id).await;
    timeline::append_pdu(
        &parsed_join_pdu,
        join_event,
        once(parsed_join_pdu.event_id.borrow()),
        &state_lock,
    )
    .unwrap();

    info!("Setting final room state for new room");
    // We set the room state after inserting the pdu, so that we never have a moment in time
    // where events in the current room state do not exist
    state::set_room_state(room_id, frame_id_after_join)?;

    let room_server_id = room_id
        .server_name()
        .map_err(|e| AppError::public(format!("bad server name: {e}")))?;
    let query = room_users::table
        .filter(room_users::room_id.ne(room_id))
        .filter(room_users::user_id.eq(sender_id))
        .filter(room_users::room_server_id.eq(room_server_id));
    if !diesel_exists!(query, &mut connect()?)? {
        let content = DeviceListUpdateContent::new(
            sender_id.to_owned(),
            authed.device_id().to_owned(),
            data::next_sn()? as u64,
        );
        let edu = Edu::DeviceListUpdate(content);
        send_edu_server(room_server_id, &edu)?;
    }
    Ok(())
}

async fn make_join_request(
    user_id: &UserId,
    room_id: &RoomId,
    servers: &[OwnedServerName],
) -> AppResult<(MakeJoinResBody, OwnedServerName)> {
    let mut last_join_error = Err(StatusError::bad_request()
        .brief("No server available to assist in joining.")
        .into());

    for remote_server in servers {
        if remote_server == config::server_name() {
            continue;
        }
        info!("Asking {remote_server} for make_join");

        let make_join_request = crate::core::federation::membership::make_join_request(
            &remote_server.origin().await,
            MakeJoinReqArgs {
                room_id: room_id.to_owned(),
                user_id: user_id.to_owned(),
                ver: config::supported_room_versions(),
            },
        )?
        .into_inner();
        let make_join_response = crate::sending::send_federation_request(remote_server, make_join_request).await;
        match make_join_response {
            Ok(make_join_response) => {
                let res_body = make_join_response.json::<MakeJoinResBody>().await;
                last_join_error = res_body.map(|r| (r, remote_server.clone())).map_err(Into::into);
            }
            Err(e) => {
                tracing::error!("make_join_request failed: {e:?}");
                last_join_error = Err(e);
            }
        }

        if last_join_error.is_ok() {
            break;
        }
    }

    last_join_error
}
