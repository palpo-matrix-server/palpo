use std::collections::BTreeMap;

use diesel::prelude::*;

use crate::core::UnixMillis;
use crate::core::events::receipt::{
    Receipt, ReceiptContent, ReceiptData, ReceiptEvent, ReceiptEventContent, ReceiptMap, ReceiptType, Receipts,
};
use crate::core::federation::transaction::Edu;
use crate::core::identifiers::*;
use crate::core::serde::JsonValue;
use crate::data::room::NewDbReceipt;
use crate::data::schema::*;
use crate::data::{connect, next_sn};
use crate::room::timeline;
use crate::{AppResult, data, sending};

/// Replaces the previous read receipt.
#[tracing::instrument]
pub fn update_read(user_id: &UserId, room_id: &RoomId, event: ReceiptEvent) -> AppResult<()> {
    data::room::receipt::update_read(user_id, room_id, &event)?;

    let receipts = BTreeMap::from_iter([(
        room_id.to_owned(),
        ReceiptMap::new(BTreeMap::from_iter([(
            user_id.to_owned(),
            ReceiptData::new(
                Receipt::new(UnixMillis::now()),
                event.content.0.keys().cloned().collect(),
            ),
        )])),
    )]);
    let edu = Edu::Receipt(ReceiptContent::new(receipts));
    sending::send_edu_room(room_id, &edu)?;
    Ok(())
}

/// Sets a private read marker at `count`.
#[tracing::instrument]
pub fn set_private_read(room_id: &RoomId, user_id: &UserId, event_id: &EventId, event_sn: i64) -> AppResult<()> {
    diesel::insert_into(event_receipts::table)
        .values(&NewDbReceipt {
            ty: ReceiptType::ReadPrivate.to_string(),
            room_id: room_id.to_owned(),
            user_id: user_id.to_owned(),
            event_id: event_id.to_owned(),
            occur_sn: next_sn()?,
            json_data: JsonValue::default(),
            receipt_at: UnixMillis::now(),
        })
        .execute(&mut connect()?)?;
    Ok(())
}

/// Gets the latest private read receipt from the user in the room
pub fn last_private_read(user_id: &UserId, room_id: &RoomId) -> AppResult<ReceiptEventContent> {
    let event_id = event_receipts::table
        .filter(event_receipts::room_id.eq(room_id))
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::ty.eq(ReceiptType::ReadPrivate.to_string()))
        .order_by(event_receipts::id.desc())
        .select(event_receipts::event_id)
        .first::<OwnedEventId>(&mut connect()?)?;

    // let room_sn = crate::room::get_room_sn(room_id)
    //     .map_err(|e| MatrixError::bad_state(format!("room does not exist in database for {room_id}: {e}")))?;

    let pdu = timeline::get_pdu(&event_id)?;

    let event_id: OwnedEventId = (&*pdu.event_id).to_owned();
    let user_id: OwnedUserId = user_id.to_owned();
    let content: BTreeMap<OwnedEventId, Receipts> = BTreeMap::from_iter([(
        event_id,
        BTreeMap::from_iter([(
            crate::core::events::receipt::ReceiptType::ReadPrivate,
            BTreeMap::from_iter([(
                user_id,
                crate::core::events::receipt::Receipt {
                    ts: None, // TODO: start storing the timestamp so we can return one
                    thread: crate::core::events::receipt::ReceiptThread::Unthreaded,
                },
            )]),
        )]),
    )]);
    Ok(ReceiptEventContent(content))
}
