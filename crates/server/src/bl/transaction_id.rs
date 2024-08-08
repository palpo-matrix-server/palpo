use diesel::prelude::*;
use palpo_core::UnixMillis;

use crate::core::identifiers::*;
use crate::core::{DeviceId, TransactionId, UserId};
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_txn_ids, primary_key(event_id))]
pub struct DbEventTxnId {
    pub event_id: OwnedEventId,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub device_id: Option<OwnedDeviceId>,
    pub txn_id: OwnedTransactionId,
    pub created_at: UnixMillis,
}

pub fn add_txn_id(
    event_id: &EventId,
    room_id: &RoomId,
    user_id: &UserId,
    device_id: Option<&DeviceId>,
    txn_id: &TransactionId,
) -> AppResult<()> {
    diesel::insert_into(event_txn_ids::table)
        .values(&DbEventTxnId {
            event_id: event_id.to_owned(),
            room_id: room_id.to_owned(),
            user_id: user_id.to_owned(),
            device_id: device_id.map(|d| d.to_owned()),
            txn_id: txn_id.to_owned(),
            created_at: UnixMillis::now(),
        })
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn existing_txn_id(
    user_id: &UserId,
    device_id: Option<&DeviceId>,
    txn_id: &TransactionId,
) -> AppResult<Option<OwnedEventId>> {
    event_txn_ids::table
        .filter(event_txn_ids::user_id.eq(user_id))
        .filter(event_txn_ids::device_id.eq(device_id))
        .filter(event_txn_ids::txn_id.eq(txn_id))
        .select(event_txn_ids::event_id)
        .first::<OwnedEventId>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}
