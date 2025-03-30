use diesel::prelude::*;
use palpo_core::UnixMillis;

use crate::core::identifiers::*;
use crate::core::{DeviceId, TransactionId, UserId};
use crate::{AppResult, db};
use crate::{diesel_exists, schema::*};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_txn_ids, primary_key(event_id))]
pub struct DbEventTxnId {
    pub id: i64,
    pub txn_id: OwnedTransactionId,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub device_id: Option<OwnedDeviceId>,
    pub event_id: Option<OwnedEventId>,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_txn_ids, primary_key(event_id))]
pub struct NewDbEventTxnId {
    pub txn_id: OwnedTransactionId,
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub device_id: Option<OwnedDeviceId>,
    pub event_id: Option<OwnedEventId>,
    pub created_at: UnixMillis,
}

pub fn add_txn_id(
    txn_id: &TransactionId,
    user_id: &UserId,
    room_id: Option<&RoomId>,
    device_id: Option<&DeviceId>,
    event_id: Option<&EventId>,
) -> AppResult<()> {
    println!("=============add_txn_id {event_id:?}  {txn_id:?}");
    diesel::insert_into(event_txn_ids::table)
        .values(&NewDbEventTxnId {
            txn_id: txn_id.to_owned(),
            user_id: user_id.to_owned(),
            room_id: room_id.map(|r| r.to_owned()),
            device_id: device_id.map(|d| d.to_owned()),
            event_id: event_id.map(|e| e.to_owned()),
            created_at: UnixMillis::now(),
        })
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn txn_id_exists(txn_id: &TransactionId, user_id: &UserId, device_id: Option<&DeviceId>) -> AppResult<bool> {
    if let Some(device_id) = device_id {
        let query = event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.eq(device_id))
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id);
        diesel_exists!(query, &mut *db::connect()?).map_err(Into::into)
    } else {
        let query = event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.is_null())
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id);
        diesel_exists!(query, &mut *db::connect()?).map_err(Into::into)
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
            .first::<Option<OwnedEventId>>(&mut *db::connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    } else {
        event_txn_ids::table
            .filter(event_txn_ids::user_id.eq(user_id))
            .filter(event_txn_ids::device_id.is_null())
            .filter(event_txn_ids::txn_id.eq(txn_id))
            .select(event_txn_ids::event_id)
            .first::<Option<OwnedEventId>>(&mut *db::connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    }
}
