use std::collections::BTreeMap;

use diesel::prelude::*;
use palpo_core::JsonValue;
use palpo_core::MatrixError;

use crate::core::Seqnum;
use crate::core::UnixMillis;
use crate::core::events::receipt::{Receipt, Receipts,ReceiptEvent, ReceiptEventContent, ReceiptType};
use crate::core::events::{AnySyncEphemeralRoomEvent, SyncEphemeralRoomEvent};
use crate::core::identifiers::*; 
use crate::core::serde::RawJson;
use crate::schema::*;
use crate::{AppResult, db};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_receipts)]
pub struct DbReceipt {
    pub id: i64,
    pub ty: String,

    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub json_data: JsonValue,
    pub receipt_at: UnixMillis,
}
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = event_receipts)]
pub struct NewDbReceipt {
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub json_data: JsonValue,
    pub receipt_at: UnixMillis,
}

/// Replaces the previous read receipt.
#[tracing::instrument]
pub fn update_read(user_id: &UserId, room_id: &RoomId, event: ReceiptEvent) -> AppResult<()> {
    for (event_id, receipts) in event.content {
        if let Ok(event_sn) = crate::event::get_event_sn(&event_id) {
            for (receipt_ty, user_receipts) in receipts {
                if let Some(receipt) = user_receipts.get(user_id) {
                    let receipt_at = receipt.ts.unwrap_or_else(|| UnixMillis::now());
                    let receipt = NewDbReceipt {
                        ty: receipt_ty.to_string(),
                        room_id: room_id.to_owned(),
                        user_id: user_id.to_owned(),
                        event_id: event_id.clone(),
                        event_sn,
                        json_data: serde_json::to_value(receipt)?,
                        receipt_at,
                    };
                    diesel::insert_into(event_receipts::table)
                        .values(&receipt)
                        .on_conflict((event_receipts::ty, event_receipts::room_id, event_receipts::user_id))
                        .do_update()
                        .set(&receipt)
                        .execute(&mut *db::connect()?)?;
                }
            }
        }
    }
    Ok(())
}

/// Returns an iterator over the most recent read_receipts in a room that happened after the event with id `since`.
pub fn read_receipts(room_id: &RoomId, since_sn: Seqnum) -> AppResult<SyncEphemeralRoomEvent<ReceiptEventContent>> {
    let mut event_content: BTreeMap<OwnedEventId, BTreeMap<ReceiptType, BTreeMap<OwnedUserId, Receipt>>> =
        BTreeMap::new();
    let receipts = event_receipts::table
        .filter(event_receipts::room_id.eq(room_id))
        .filter(event_receipts::event_sn.ge(since_sn))
        .load::<DbReceipt>(&mut *db::connect()?)?;
    for receipt in receipts {
        let DbReceipt {
            ty,
            user_id,
            event_id,
            json_data,
            ..
        } = receipt;
        let event_map = event_content.entry(event_id).or_default();
        let receipt_type = ReceiptType::from(ty);
        let type_map = event_map.entry(receipt_type).or_default();
        type_map.insert(user_id, serde_json::from_value(json_data).unwrap_or_default());
    }

    Ok(SyncEphemeralRoomEvent {
        content: ReceiptEventContent(event_content),
    })
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
            event_sn,
            json_data: JsonValue::default(),
            receipt_at: UnixMillis::now(),
        })
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)?;
    Ok(())
}

/// Gets the latest private read receipt from the user in the room
pub fn latest_private_read(room_id: &RoomId, user_id: &UserId) -> AppResult<RawJson<AnySyncEphemeralRoomEvent>> {
    let event_id = event_receipts::table
        .filter(event_receipts::room_id.eq(room_id))
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::ty.eq(ReceiptType::ReadPrivate.to_string()))
        .order_by(event_receipts::id.desc())
        .select(event_receipts::event_id)
        .first::<OwnedEventId>(&mut *db::connect()?)?;

    // let room_sn = crate::room::get_room_sn(room_id)
    //     .map_err(|e| MatrixError::bad_state(format!("room does not exist in database for {room_id}: {e}")))?;

    let pdu = crate::room::timeline.get_pdu(&event_id)?;

    let event_id: OwnedEventId = pdu.event_id;
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
    let receipt_event_content = ReceiptEventContent(content);
    let receipt_sync_event = SyncEphemeralRoomEvent {
        content: receipt_event_content,
    };

    let event = serde_json::value::to_raw_value(&receipt_sync_event).expect("receipt created manually");

    Ok(RawJson::from_json(event))
}

// /// Returns the count of the last typing update in this room.
// #[tracing::instrument]
// pub fn update_last_private_read(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
//     let mut key = room_id.as_bytes().to_vec();
//     key.push(0xff);
//     key.extend_from_slice(user_id.as_bytes());

//     Ok(self
//         .roomuser_id_lastprivatereadupdate
//         .get(&key)?
//         .map(|bytes| {
//             utils::u64_from_bytes(&bytes)
//                 .map_err(|_| AppError::public("Count in roomuser_id_lastprivatereadupdate is invalid."))
//         })
//         .transpose()?
//         .unwrap_or(0))
// }

pub fn pack_receipts<I>(receipts: I) -> RawJson<SyncEphemeralRoomEvent<ReceiptEventContent>>
where
    I: Iterator<Item = RawJson<AnySyncEphemeralRoomEvent>>,
{
    let mut json = BTreeMap::new();
    for value in receipts {
        let receipt = serde_json::from_str::<SyncEphemeralRoomEvent<ReceiptEventContent>>(value.inner().get());
        match receipt {
            Ok(value) => {
                for (event, receipt) in value.content {
                    json.insert(event, receipt);
                }
            }
            _ => {
                debug!("failed to parse receipt: {:?}", receipt);
            }
        }
    }
    let content = ReceiptEventContent::from_iter(json);

    RawJson::from_raw_value(
        serde_json::value::to_raw_value(&SyncEphemeralRoomEvent { content }).expect("received valid json"),
    )
}
