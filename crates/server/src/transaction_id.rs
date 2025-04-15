use diesel::prelude::*;
use palpo_core::UnixMillis;

use crate::core::identifiers::*;
use crate::core::{DeviceId, TransactionId, UserId};
use crate::data::connect;
use crate::data::room::{DbEventTxnId, NewDbEventTxnId};
use crate::data::schema::*;
use crate::{AppResult, data, diesel_exists};

pub fn add_txn_id(
    txn_id: &TransactionId,
    user_id: &UserId,
    room_id: Option<&RoomId>,
    device_id: Option<&DeviceId>,
    event_id: Option<&EventId>,
) -> AppResult<()> {
    diesel::insert_into(event_txn_ids::table)
        .values(&NewDbEventTxnId {
            txn_id: txn_id.to_owned(),
            user_id: user_id.to_owned(),
            room_id: room_id.map(|r| r.to_owned()),
            device_id: device_id.map(|d| d.to_owned()),
            event_id: event_id.map(|e| e.to_owned()),
            created_at: UnixMillis::now(),
        })
        .execute(&mut connect()?)?;
    Ok(())
}

pub fn txn_id_exists(txn_id: &TransactionId, user_id: &UserId, device_id: Option<&DeviceId>) -> AppResult<bool> {
    if let Some(device_id) = device_id {
        let query = event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.eq(device_id))
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id);
        diesel_exists!(query, &mut connect()?).map_err(Into::into)
    } else {
        let query = event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.is_null())
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id);
        diesel_exists!(query, &mut connect()?).map_err(Into::into)
    }
}

pub fn get_event_id(
    txn_id: &TransactionId,
    user_id: &UserId,
    device_id: Option<&DeviceId>,
) -> AppResult<Option<OwnedEventId>> {
    if let Some(device_id) = device_id {
        event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.eq(device_id))
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id)
            .first::<Option<OwnedEventId>>(&mut connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    } else {
        event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.is_null())
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id)
            .first::<Option<OwnedEventId>>(&mut connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    }
}
