use std::collections::BTreeMap;

use salvo::oapi::extract::JsonBody;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::events::RoomAccountDataEventType;
use crate::core::events::receipt::{
    CreateReceiptReqBody, Receipt, ReceiptEvent, ReceiptEventContent, ReceiptThread, ReceiptType, SendReceiptReqArgs,
};
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, data, empty_ok};

/// #POST /_matrix/client/r0/rooms/{room_id}/receipt/{receipt_type}/{event_id}
/// Sets private read marker and public read receipt EDU.
#[endpoint]
pub(super) fn send_receipt(
    _aa: AuthArgs,
    args: SendReceiptReqArgs,
    body: JsonBody<CreateReceiptReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;
    let body = body.into_inner();

    crate::user::ping_presence(authed.user_id(), &crate::core::presence::PresenceState::Online)?;

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
                    thread: body.thread.clone(),
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
            // let count = timeline::get_event_sn(&args.event_id)?
            //     .ok_or(MatrixError::invalid_param("Event does not exist."))?;
            let event_sn = crate::event::get_event_sn(&args.event_id)?;
            data::room::receipt::set_private_read(&args.room_id, authed.user_id(), &args.event_id, event_sn)?;
        }
        _ => return Err(AppError::internal("Unsupported receipt type")),
    }
    if matches!(&args.receipt_type, ReceiptType::Read | ReceiptType::ReadPrivate) {
        crate::room::user::update_notify_summary(authed.user_id(), &args.room_id)?;
    }
    empty_ok()
}
