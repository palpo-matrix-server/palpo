use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{AuthArgs, AuthedInfo};
use crate::{hoops, DepotExt, json_ok, empty_ok, JsonResult};

// #POST /_matrix/client/r0/rooms/{room_id}/receipt/{receiptType}/{event_id}
/// Sets private read marker and public read receipt EDU.
#[endpoint]
pub(super) async fn create(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;
     let authed = depot.authed_info()?;

    if matches!(
        &body.receipt_type,
        create_receipt::v3::ReceiptType::Read | create_receipt::v3::ReceiptType::ReadPrivate
    ) {
        crate::room::user::reset_notification_counts(authed.user_id(), &body.room_id)?;
    }

    match body.receipt_type {
        create_receipt::v3::ReceiptType::FullyRead => {
            let fully_read_event = crate::core::events::fully_read::FullyReadEvent {
                content: crate::core::events::fully_read::FullyReadEventContent {
                    event_id: body.event_id.clone(),
                },
            };
            crate::user::set_data(
                Some(&body.room_id),
                authed.user_id(),
                RoomAccountDataEventType::FullyRead,
                &serde_json::to_value(fully_read_event.content).expect("to json value always works"),
            )?;
        }
        create_receipt::v3::ReceiptType::Read => {
            let mut user_receipts = BTreeMap::new();
            user_receipts.insert(
                authed.user_id().clone(),
                crate::core::events::receipt::Receipt {
                    ts: Some(UnixMillis::now()),
                    thread: ReceiptThread::Unthreaded,
                },
            );
            let mut receipts = BTreeMap::new();
            receipts.insert(ReceiptType::Read, user_receipts);

            let mut receipt_content = BTreeMap::new();
            receipt_content.insert(body.event_id.to_owned(), receipts);

            crate::room::edus.read_receipt.update_read(
                authed.user_id(),
                &body.room_id,
                crate::core::events::receipt::ReceiptEvent {
                    content: crate::core::events::receipt::ReceiptEventContent(receipt_content),
                    room_id: body.room_id.clone(),
                },
            )?;
        }
        create_receipt::v3::ReceiptType::ReadPrivate => {
            let event_sn = crate::room::timeline::get_event_sn(&body.event_id)?.ok_or(MatrixError::invalid_param("Event does not exist."))?;
            crate::room::edus.read_receipt.set_private_read(&body.room_id, authed.user_id(), count)?;
        }
        _ => return Err(AppError::internal("Unsupported receipt type")),
    }
    empty_ok()
}
