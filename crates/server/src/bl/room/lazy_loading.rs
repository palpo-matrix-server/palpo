use diesel::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use crate::core::{DeviceId, OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use crate::schema::*;
use crate::{db, diesel_exists, AppError, AppResult};

pub const LAZY_LOAD_WAITING: LazyLock<
    Mutex<HashMap<(OwnedUserId, OwnedDeviceId, OwnedRoomId, i64), HashSet<OwnedUserId>>>,
> = LazyLock::new(Default::default);

#[tracing::instrument]
pub fn lazy_load_was_sent_before(
    user_id: &UserId,
    device_id: &DeviceId,
    room_id: &RoomId,
    confirmed_user_id: &UserId,
) -> AppResult<bool> {
    let query = lazy_load_deliveries::table
        .filter(lazy_load_deliveries::user_id.eq(user_id))
        .filter(lazy_load_deliveries::device_id.eq(device_id))
        .filter(lazy_load_deliveries::room_id.eq(room_id))
        .filter(lazy_load_deliveries::confirmed_user_id.eq(confirmed_user_id));
    diesel_exists!(query, &mut *db::connect()?).map_err(Into::into)
}

#[tracing::instrument]
pub fn lazy_load_mark_sent(
    user_id: &UserId,
    device_id: &DeviceId,
    room_id: &RoomId,
    lazy_load: HashSet<OwnedUserId>,
    next_batch: i64,
) {
    LAZY_LOAD_WAITING.lock().unwrap().insert(
        (user_id.to_owned(), device_id.to_owned(), room_id.to_owned(), next_batch),
        lazy_load,
    );
}

#[tracing::instrument]
pub fn lazy_load_confirm_delivery(
    user_id: &UserId,
    device_id: &DeviceId,
    room_id: &RoomId,
    occur_sn: i64,
) -> AppResult<()> {
    if let Some(confirmed_user_ids) = LAZY_LOAD_WAITING.lock().unwrap().remove(&(
        user_id.to_owned(),
        device_id.to_owned(),
        room_id.to_owned(),
        occur_sn,
    )) {
        for confirmed_user_id in confirmed_user_ids {
            diesel::insert_into(lazy_load_deliveries::table)
                .values((
                    lazy_load_deliveries::user_id.eq(user_id),
                    lazy_load_deliveries::device_id.eq(device_id),
                    lazy_load_deliveries::room_id.eq(room_id),
                    lazy_load_deliveries::confirmed_user_id.eq(confirmed_user_id),
                ))
                .on_conflict_do_nothing()
                .execute(&mut db::connect()?)?;
        }
    }

    Ok(())
}

#[tracing::instrument]
pub fn lazy_load_reset(user_id: &UserId, device_id: &DeviceId, room_id: &RoomId) -> AppResult<()> {
    diesel::delete(
        lazy_load_deliveries::table
            .filter(lazy_load_deliveries::user_id.eq(user_id))
            .filter(lazy_load_deliveries::device_id.eq(device_id))
            .filter(lazy_load_deliveries::room_id.eq(room_id)),
    )
    .execute(&mut db::connect()?)?;
    Ok(())
}
