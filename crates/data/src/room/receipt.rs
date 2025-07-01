use std::collections::{BTreeMap, HashSet};

use diesel::prelude::*;

use crate::core::events::receipt::{Receipt, ReceiptEventContent, ReceiptType};
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, };
use crate::core::{Seqnum, UnixMillis};
use crate::room::{DbReceipt, NewDbReceipt};
use crate::schema::*;
use crate::{DataResult, connect};

/// Returns an iterator over the most recent read_receipts in a room that happened after the event with id `since`.
pub fn read_receipts(room_id: &RoomId, since_sn: Seqnum) -> DataResult<BTreeMap<OwnedUserId, ReceiptEventContent>> {
    let receipts = event_receipts::table
        .filter(event_receipts::event_sn.ge(since_sn))
        .filter(event_receipts::room_id.eq(room_id))
        .order_by(event_receipts::id.desc())
        .load::<DbReceipt>(&mut connect()?)?;
    let unthread_receipts = receipts
        .iter()
        .filter(|r| r.thread_id.is_none())
        .map(|r| (r.user_id.clone(), r.event_id.clone()))
        .collect::<HashSet<_>>();

    let mut grouped: BTreeMap<OwnedUserId, Vec<_>> = BTreeMap::new();
    for mut receipt in receipts {
        if receipt.thread_id.is_some() {
            if unthread_receipts.contains(&(receipt.user_id.clone(), receipt.event_id.clone())) {
                receipt.thread_id = None;
            }
        }
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
pub fn set_private_read(room_id: &RoomId, user_id: &UserId, event_id: &EventId, event_sn: Seqnum) -> DataResult<()> {
    diesel::insert_into(event_receipts::table)
        .values(&NewDbReceipt {
            ty: ReceiptType::ReadPrivate.to_string(),
            room_id: room_id.to_owned(),
            user_id: user_id.to_owned(),
            event_id: event_id.to_owned(),
            event_sn,
            thread_id: None,
            json_data: JsonValue::default(),
            receipt_at: UnixMillis::now(),
        })
        .execute(&mut connect()?)?;
    Ok(())
}

pub fn last_private_read_update_sn(user_id: &UserId, room_id: &RoomId) -> DataResult<Seqnum> {
    let event_sn = event_receipts::table
        .filter(event_receipts::room_id.eq(room_id))
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::ty.eq(ReceiptType::ReadPrivate.to_string()))
        .order_by(event_receipts::event_sn.desc())
        .select(event_receipts::event_sn)
        .first::<Seqnum>(&mut connect()?)?;

    Ok(event_sn)
}
