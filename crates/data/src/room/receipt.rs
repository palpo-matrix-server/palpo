use std::collections::BTreeMap;

use diesel::prelude::*;

use crate::core::events::AnySyncEphemeralRoomEvent;
use crate::core::events::receipt::{Receipt, ReceiptEvent, ReceiptEventContent, ReceiptType, Receipts};
use crate::core::federation::transaction::Edu;
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{Seqnum, UnixMillis};
use crate::room::{DbReceipt, NewDbReceipt};
use crate::schema::*;
use crate::{connect, DataResult, next_sn};

/// Replaces the previous read receipt.
#[tracing::instrument]
pub fn update_read(user_id: &UserId, room_id: &RoomId, event: &ReceiptEvent) -> DataResult<()> {
    let occur_sn = next_sn()?;
    for (event_id, receipts) in event.content.clone() {
        for (receipt_ty, user_receipts) in receipts {
            if let Some(receipt) = user_receipts.get(user_id) {
                let receipt_at = receipt.ts.unwrap_or_else(|| UnixMillis::now());
                let receipt = NewDbReceipt {
                    ty: receipt_ty.to_string(),
                    room_id: room_id.to_owned(),
                    user_id: user_id.to_owned(),
                    event_id: event_id.clone(),
                    occur_sn,
                    json_data: serde_json::to_value(receipt)?,
                    receipt_at,
                };
                diesel::insert_into(event_receipts::table)
                    .values(&receipt)
                    .execute(&mut connect()?)?;
            }
        }
    }

    Ok(())
}

/// Returns an iterator over the most recent read_receipts in a room that happened after the event with id `since`.
pub fn read_receipts(room_id: &RoomId, since_sn: Seqnum) -> DataResult<BTreeMap<OwnedUserId, ReceiptEventContent>> {
    let list: Vec<(OwnedUserId, Seqnum, RawJson<AnySyncEphemeralRoomEvent>)> = Vec::new();
    let receipts = event_receipts::table
        .filter(event_receipts::occur_sn.ge(since_sn))
        .filter(event_receipts::room_id.eq(room_id))
        .load::<DbReceipt>(&mut connect()?)?;
    let mut grouped: BTreeMap<OwnedUserId, Vec<_>> = BTreeMap::new();
    for receipt in receipts {
        grouped.entry(receipt.user_id.clone()).or_default().push(receipt);
    }

    let mut receipts = BTreeMap::new();
    for (user_id, items) in grouped {
        let mut event_content: BTreeMap<OwnedEventId, BTreeMap<ReceiptType, BTreeMap<OwnedUserId, Receipt>>> =
            BTreeMap::new();

        for item in items {
            event_content.entry(item.event_id.clone()).or_default().insert(
                ReceiptType::from(item.ty),
                BTreeMap::from_iter([(
                    item.user_id.clone(),
                    serde_json::from_value(item.json_data).unwrap_or_default(),
                )]),
            );
        }
        receipts.insert(user_id.clone(), ReceiptEventContent(event_content));
    }

    Ok(receipts)
}

/// Sets a private read marker at `count`.
#[tracing::instrument]
pub fn set_private_read(room_id: &RoomId, user_id: &UserId, event_id: &EventId, event_sn: i64) -> DataResult<()> {
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

pub fn last_private_read_update_sn(user_id: &UserId, room_id: &RoomId) -> DataResult<Seqnum> {
    let occur_sn = event_receipts::table
        .filter(event_receipts::room_id.eq(room_id))
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::ty.eq(ReceiptType::ReadPrivate.to_string()))
        .order_by(event_receipts::id.desc())
        .select(event_receipts::occur_sn)
        .first::<Seqnum>(&mut connect()?)?;

    Ok(occur_sn)
}
