use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::value::to_raw_value;

use crate::core::client::room::ReportContentReqBody;
use crate::core::client::state::{
    SendStateEventResBody, StateEventsForKeyReqArgs, StateEventsForKeyResBody, StateEventsResBody,
};
use crate::core::client::typing::{CreateTypingEventReqBody, Typing};
use crate::core::client::uiaa::AuthData;
use crate::core::events::receipt::{
    Receipt, ReceiptEvent, ReceiptEventContent, ReceiptThread, ReceiptType, SendReceiptReqArgs,
};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::events::{AnyStateEventContent, RoomAccountDataEventType, StateEventType, TimelineEventType};
use crate::core::http::{RoomEventReqArgs, RoomEventTypeReqArgs, RoomTypingReqArgs};
use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::utils::HtmlEscape;
use crate::{
    db, empty_ok, hoops, json_ok, utils, AppError, AppResult, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult,
    MatrixError, PduBuilder,
};

// #GET /_matrix/client/r0/rooms/{room_id}/state
/// Get all state events for a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) fn get_state(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    depot: &mut Depot,
) -> JsonResult<StateEventsResBody> {
    let authed = depot.authed_info()?;
    let room_id = room_id.into_inner();

    if !crate::room::state::user_can_see_state_events(&authed.user_id(), &room_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view the room state.").into());
    }

    let frame_id = crate::room::state::get_room_frame_id(&room_id)?.ok_or(AppError::public("state delta not found"))?;

    json_ok(StateEventsResBody {
        room_state: crate::room::state::get_full_state(frame_id)?
            .values()
            .map(|pdu| pdu.to_state_event())
            .collect(),
    })
}
// #POST /_matrix/client/r0/rooms/{room_id}/report/{event_id}
/// Reports an inappropriate event to homeserver admins
#[endpoint]
pub fn report(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<ReportContentReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let pdu = match crate::room::timeline::get_pdu(&args.event_id)? {
        Some(pdu) => pdu,
        _ => return Err(MatrixError::invalid_param("Invalid Event ID").into()),
    };

    if let Some(true) = body.score.map(|s| s > 0 || s < -100) {
        return Err(MatrixError::invalid_param("Invalid score, must be within 0 to -100").into());
    };

    if let Some(true) = body.reason.clone().map(|s| s.chars().count() > 250) {
        return Err(MatrixError::invalid_param("Reason too long, should be 250 characters or fewer").into());
    };

    crate::admin::send_message(RoomMessageEventContent::text_html(
        format!(
            "Report received from: {}\n\n\
                Event ID: {:?}\n\
                Room ID: {:?}\n\
                Sent By: {:?}\n\n\
                Report Score: {:?}\n\
                Report Reason: {:?}",
            authed.user_id(),
            pdu.event_id,
            pdu.room_id,
            pdu.sender,
            body.score,
            body.reason
        ),
        format!(
            "<details><summary>Report received from: <a href=\"https://matrix.to/#/{0:?}\">{0:?}\
                </a></summary><ul><li>Event Info<ul><li>Event ID: <code>{1:?}</code>\
                <a href=\"https://matrix.to/#/{2:?}/{1:?}\">🔗</a></li><li>Room ID: <code>{2:?}</code>\
                </li><li>Sent By: <a href=\"https://matrix.to/#/{3:?}\">{3:?}</a></li></ul></li><li>\
                Report Info<ul><li>Report Score: {4:?}</li><li>Report Reason: {5}</li></ul></li>\
                </ul></details>",
            authed.user_id(),
            pdu.event_id,
            pdu.room_id,
            pdu.sender,
            body.score,
            HtmlEscape(body.reason.as_deref().unwrap_or(""))
        ),
    ));
    empty_ok()
}
// #GET /_matrix/client/r0/rooms/{room_id}/state/{event_type}/{state_key}
/// Get single state event of a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) fn state_for_key(
    _aa: AuthArgs,
    args: StateEventsForKeyReqArgs,
    depot: &mut Depot,
) -> JsonResult<StateEventsForKeyResBody> {
    let authed = depot.authed_info()?;
    if !crate::room::state::user_can_see_state_events(&authed.user_id(), &args.room_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view the room state.").into());
    }

    let event = crate::room::state::get_state(&args.room_id, &args.event_type, &args.state_key)?.ok_or_else(|| {
        warn!(
            "State event {:?} not found in room {:?}",
            &args.event_type, &args.room_id
        );
        MatrixError::not_found("State event not found.")
    })?;

    json_ok(StateEventsForKeyResBody {
        content: serde_json::from_str(event.content.get())
            .map_err(|_| AppError::internal("Invalid event content in database"))?,
    })
}

// #GET /_matrix/client/r0/rooms/{room_id}/state/{event_type}
/// Get single state event of a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) async fn state_for_empty_key(
    _aa: AuthArgs,
    args: RoomEventTypeReqArgs,
    depot: &mut Depot,
) -> JsonResult<StateEventsForKeyResBody> {
    let authed = depot.authed_info()?;

    if !crate::room::state::user_can_see_state_events(&authed.user_id(), &args.room_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view the room state.").into());
    }

    let event = crate::room::state::get_state(&args.room_id, &args.event_type, "")?.ok_or_else(|| {
        warn!(
            "State event {:?} not found in room {:?}",
            &args.event_type, &args.room_id
        );
        MatrixError::not_found("State event not found.")
    })?;

    json_ok(StateEventsForKeyResBody {
        content: serde_json::from_str(event.content.get())
            .map_err(|_| AppError::internal("Invalid event content in database"))?,
    })
}

// #PUT /_matrix/client/r0/rooms/{room_id}/state/{event_type}/{state_key}
/// Sends a state event into the room.
///
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
/// - If event is new canonical_alias: Rejects if alias is incorrect
#[endpoint]
pub(super) async fn send_state_for_key(
    _aa: AuthArgs,
    args: StateEventsForKeyReqArgs,
    body: JsonBody<AnyStateEventContent>,
    depot: &mut Depot,
) -> JsonResult<SendStateEventResBody> {
    let authed = depot.authed_info()?;
    let body = body.into_inner();

    let event_id = crate::state::send_state_event_for_key(
        authed.user_id(),
        &args.room_id,
        &args.event_type,
        body,
        args.state_key.to_owned(),
    )
    .await?;

    let event_id = (*event_id).to_owned();
    json_ok(SendStateEventResBody { event_id })
}

// #PUT /_matrix/client/r0/rooms/{room_id}/state/{event_type}
/// Sends a state event into the room.
///
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
/// - If event is new canonical_alias: Rejects if alias is incorrect
#[endpoint]
pub(super) async fn send_state_for_empty_key(
    _aa: AuthArgs,
    args: RoomEventTypeReqArgs,
    body: JsonBody<AnyStateEventContent>,
    depot: &mut Depot,
) -> JsonResult<SendStateEventResBody> {
    let authed = depot.authed_info()?;
    let body = body.into_inner();

    // Forbid m.room.encryption if encryption is disabled
    if args.event_type == StateEventType::RoomEncryption && !crate::allow_encryption() {
        return Err(MatrixError::forbidden("Encryption has been disabled").into());
    }

    let event_id = crate::state::send_state_event_for_key(
        authed.user_id(),
        &args.room_id,
        &args.event_type.to_string().into(),
        body,
        "".to_owned(),
    )
    .await?;

    let event_id = (*event_id).to_owned();
    json_ok(SendStateEventResBody { event_id })
}

// #POST /_matrix/client/r0/rooms/{room_id}/receipt/{receipt_type}/{event_id}
/// Sets private read marker and public read receipt EDU.
#[endpoint]
pub(super) fn send_receipt(_aa: AuthArgs, args: SendReceiptReqArgs, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;

    if matches!(&args.receipt_type, ReceiptType::Read | ReceiptType::ReadPrivate) {
        crate::room::user::reset_notification_counts(authed.user_id(), &args.room_id)?;
    }
    match args.receipt_type {
        ReceiptType::FullyRead => {
            let fully_read_event = crate::core::events::fully_read::FullyReadEvent {
                content: crate::core::events::fully_read::FullyReadEventContent {
                    event_id: args.event_id.clone(),
                },
            };
            crate::user::set_data(
                authed.user_id(),
                Some(args.room_id.clone()),
                &RoomAccountDataEventType::FullyRead.to_string(),
                serde_json::to_value(fully_read_event).expect("to json value always works"),
            )?;
        }
        ReceiptType::Read => {
            let mut user_receipts = BTreeMap::new();
            user_receipts.insert(
                authed.user_id().clone(),
                Receipt {
                    ts: Some(UnixMillis::now()),
                    thread: ReceiptThread::Unthreaded,
                },
            );
            let mut receipts = BTreeMap::new();
            receipts.insert(ReceiptType::Read, user_receipts);

            let mut receipt_content = BTreeMap::new();
            receipt_content.insert(args.event_id.to_owned(), receipts);

            crate::room::receipt::update_read(
                authed.user_id(),
                &args.room_id,
                ReceiptEvent {
                    content: ReceiptEventContent(receipt_content),
                    room_id: args.room_id.clone(),
                },
            )?;
        }
        ReceiptType::ReadPrivate => {
            // let count = crate::room::timeline::get_event_sn(&args.event_id)?
            //     .ok_or(MatrixError::invalid_param("Event does not exist."))?;
            crate::room::receipt::set_private_read(&args.room_id, authed.user_id(), &args.event_id)?;
        }
        _ => return Err(AppError::internal("Unsupported receipt type")),
    }
    empty_ok()
}
// #PUT /_matrix/client/r0/rooms/{room_id}/typing/{user_id}
/// Sets the typing state of the sender user.
#[endpoint]
pub async fn send_typing(
    _aa: AuthArgs,
    args: RoomTypingReqArgs,
    body: JsonBody<CreateTypingEventReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    if !crate::room::is_joined(authed.user_id(), &args.room_id)? {
        return Err(MatrixError::forbidden("You are not in this room.").into());
    }

    if let Typing::Yes(duration) = body.state {
        crate::room::typing::add_typing(
            authed.user_id(),
            &args.room_id,
            duration.as_millis() as u64 + UnixMillis::now().get(),
        )
        .await?;
    } else {
        crate::room::typing::remove_typing(authed.user_id(), &args.room_id).await?;
    }
    empty_ok()
}
#[endpoint]
pub(super) async fn timestamp(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    //TODO:??
    let authed = depot.authed_info()?;
    empty_ok()
}
