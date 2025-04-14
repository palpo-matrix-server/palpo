use std::collections::HashSet;

use diesel::prelude::*;

use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::{MakeLeaveResBody, SendLeaveReqBody, make_leave_request};
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, to_raw_json_value};
use crate::core::{Seqnum, UnixMillis};

use crate::event::{DbEventData, NewDbEvent, PduBuilder};
use crate::membership::federation::membership::{SendLeaveReqArgsV2, send_leave_request_v2};
use crate::schema::*;
use crate::{AppError, AppResult, GetUrlOrigin, MatrixError, db};

// Make a user leave all their joined rooms
pub async fn leave_all_rooms(user_id: &UserId) -> AppResult<()> {
    let all_room_ids = crate::user::joined_rooms(user_id, 0)?
        .into_iter()
        .chain(crate::user::invited_rooms(user_id, 0)?.into_iter().map(|t| t.0))
        .collect::<Vec<_>>();
    for room_id in all_room_ids {
        leave_room(user_id, &room_id, None).await.ok();
    }
    Ok(())
}

pub async fn leave_room(user_id: &UserId, room_id: &RoomId, reason: Option<String>) -> AppResult<()> {
    // Ask a remote server if we don't have this room
    if !crate::room::is_server_in_room(crate::server_name(), room_id)? {
        match leave_room_remote(user_id, room_id).await {
            Err(e) => {
                warn!("Failed to leave room {} remotely: {}", user_id, e);
            }
            Ok((event_id, event_sn)) => {
                let last_state = crate::room::state::get_user_state(user_id, room_id)?;

                // We always drop the invite, we can't rely on other servers
                crate::membership::update_membership(
                    &event_id,
                    event_sn,
                    room_id,
                    user_id,
                    MembershipState::Leave,
                    user_id,
                    last_state,
                )?;
            }
        }
    } else {
        let member_event =
            crate::room::state::get_room_state(room_id, &StateEventType::RoomMember, user_id.as_str()).ok();

        // Fix for broken rooms
        let Some(member_event) = member_event else {
            warn!("Trying to leave a room you are not a member of.");
            // crate::room::timeline::build_and_append_pdu(
            //     PduBuilder::state(
            //         user_id.to_string(),
            //         &RoomMemberEventContent::new(MembershipState::Leave),
            //     ),
            //     user_id,
            //     room_id,
            // )?;
            let event_id_sns = room_users::table
                .filter(room_users::room_id.eq(room_id))
                .filter(room_users::user_id.eq(user_id))
                .order_by(room_users::id.desc())
                .select((room_users::event_id, room_users::event_sn))
                .first::<(OwnedEventId, i64)>(&mut *db::connect()?)
                .optional()?;
            if let Some((event_id, event_sn)) = event_id_sns {
                crate::membership::update_membership(
                    &event_id,
                    event_sn,
                    room_id,
                    &user_id,
                    MembershipState::Leave,
                    user_id,
                    None,
                )?;
            }
            return Ok(());
        };

        let mut event: RoomMemberEventContent = serde_json::from_str(member_event.content.get())
            .map_err(|_| AppError::public("Invalid member event in database."))?;

        event.membership = MembershipState::Leave;
        event.reason = reason;

        crate::room::timeline::build_and_append_pdu(
            PduBuilder {
                event_type: TimelineEventType::RoomMember,
                content: to_raw_json_value(&event).expect("event is valid, we just created it"),
                state_key: Some(user_id.to_string()),
                ..Default::default()
            },
            user_id,
            room_id,
        )?;
    }

    Ok(())
}

async fn leave_room_remote(user_id: &UserId, room_id: &RoomId) -> AppResult<(OwnedEventId, Seqnum)> {
    let mut make_leave_response_and_server = Err(AppError::public("No server available to assist in leaving."));
    let invite_state =
        crate::room::state::get_user_state(user_id, room_id)?.ok_or(MatrixError::bad_state("User is not invited."))?;

    let servers: HashSet<_> = invite_state
        .iter()
        .filter_map(|event| serde_json::from_str(event.as_str()).ok())
        .filter_map(|event: serde_json::Value| event.get("sender").cloned())
        .filter_map(|sender| sender.as_str().map(|s| s.to_owned()))
        .filter_map(|sender| UserId::parse(sender).ok())
        .map(|user| user.server_name().to_owned())
        .collect();

    for remote_server in servers {
        let request = make_leave_request(
            &room_id.server_name().map_err(AppError::internal)?.origin().await,
            room_id,
            user_id,
        )?
        .into_inner();
        let make_leave_response =
            crate::sending::send_federation_request(&room_id.server_name().map_err(AppError::internal)?, request)
                .await?
                .json::<MakeLeaveResBody>()
                .await;

        make_leave_response_and_server = make_leave_response.map(|r| (r, remote_server)).map_err(Into::into);

        if make_leave_response_and_server.is_ok() {
            break;
        }
    }

    let (make_leave_response, remote_server) = make_leave_response_and_server?;

    let room_version_id = match make_leave_response.room_version {
        Some(version) if crate::supported_room_versions().contains(&version) => version,
        _ => return Err(AppError::public("Room version is not supported")),
    };

    let mut leave_event_stub = serde_json::from_str::<CanonicalJsonObject>(make_leave_response.event.get())
        .map_err(|_| AppError::public("Invalid make_leave event json received from server."))?;

    // TODO: Is origin needed?
    leave_event_stub.insert(
        "origin".to_owned(),
        CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
    );
    leave_event_stub.insert(
        "origin_server_ts".to_owned(),
        CanonicalJsonValue::Integer(UnixMillis::now().get() as i64),
    );
    // We don't leave the event id in the pdu because that's only allowed in v1 or v2 rooms
    leave_event_stub.remove("event_id");

    // In order to create a compatible ref hash (EventID) the `hashes` field needs to be present
    crate::server_key::hash_and_sign_event(&mut leave_event_stub, &room_version_id)
        .expect("event is valid, we just created it");

    // Generate event id
    let event_id = crate::event::gen_event_id(&leave_event_stub, &room_version_id)?;

    let event_sn = crate::event::ensure_event_sn(room_id, &event_id)?;
    let new_db_event = NewDbEvent {
        id: event_id.to_owned(),
        sn: event_sn,
        ty: MembershipState::Leave.to_string(),
        room_id: room_id.to_owned(),
        unrecognized_keys: None,
        depth: 0,
        topological_ordering: 0,
        stream_ordering: 0,
        origin_server_ts: Some(UnixMillis::now()),
        received_at: None,
        sender_id: Some(user_id.to_owned()),
        contains_url: false,
        worker_id: None,
        state_key: Some(user_id.to_string()),
        is_outlier: true,
        soft_failed: false,
        rejection_reason: None,
    };
    diesel::insert_into(events::table)
        .values(&new_db_event)
        .on_conflict_do_nothing()
        .returning(events::sn)
        .get_result::<Seqnum>(&mut *db::connect()?)?;
    // Add event_id back
    leave_event_stub.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(event_id.as_str().to_owned()),
    );

    let event_data = DbEventData {
        event_id: event_id.clone(),
        event_sn,
        room_id: room_id.to_owned(),
        internal_metadata: None,
        json_data: serde_json::to_value(&leave_event_stub)?,
        format_version: None,
    };
    diesel::insert_into(event_datas::table)
        .values(&event_data)
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)?;

    // It has enough fields to be called a proper event now
    let leave_event = leave_event_stub;

    let request = send_leave_request_v2(
        &remote_server.origin().await,
        SendLeaveReqArgsV2 {
            room_id: room_id.to_owned(),
            event_id: event_id.clone(),
        },
        SendLeaveReqBody(crate::sending::convert_to_outgoing_federation_event(
            leave_event.clone(),
        )),
    )?
    .into_inner();

    crate::sending::send_federation_request(&remote_server, request).await?;

    Ok((event_id, event_sn))
}
