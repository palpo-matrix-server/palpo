use std::collections::BTreeMap;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::client::room::ReportContentReqBody;
use crate::core::client::state::{
    SendStateEventReqBody, SendStateEventResBody, StateEventsForEmptyKeyReqArgs, StateEventsForKeyReqArgs,
    StateEventsForKeyResBody, StateEventsResBody,
};
use crate::core::client::typing::{CreateTypingEventReqBody, Typing};
use crate::core::events::RoomAccountDataEventType;
use crate::core::events::receipt::{
    Receipt, ReceiptEvent, ReceiptEventContent, ReceiptThread, ReceiptType, SendReceiptReqArgs,
};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::identifiers::*;
use crate::core::room::{RoomEventReqArgs, RoomEventTypeReqArgs, RoomTypingReqArgs};
use crate::room::state::UserCanSeeEvent;
use crate::utils::HtmlEscape;
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, empty_ok, json_ok};

/// #POST /_matrix/client/r0/rooms/{room_id}/receipt/{receipt_type}/{event_id}
/// Sets private read marker and public read receipt EDU.
#[endpoint]
pub(super) fn send_receipt(_aa: AuthArgs, args: SendReceiptReqArgs, depot: &mut Depot) -> EmptyResult {
    println!("=========send receipt");
    let authed = depot.authed_info()?;

    if matches!(&args.receipt_type, ReceiptType::Read | ReceiptType::ReadPrivate) {
        crate::room::user::reset_notification_counts(authed.user_id(), &args.room_id)?;
    }

    // ping presence
    if crate::config().allow_local_presence {
        crate::user::ping_presence(authed.user_id(), &crate::core::presence::PresenceState::Online)?;
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
                serde_json::to_value(fully_read_event.content).expect("to json value always works"),
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
            let event_sn = crate::event::get_event_sn(&args.event_id)?;
            crate::room::receipt::set_private_read(&args.room_id, authed.user_id(), &args.event_id, event_sn)?;
        }
        _ => return Err(AppError::internal("Unsupported receipt type")),
    }
    empty_ok()
}
