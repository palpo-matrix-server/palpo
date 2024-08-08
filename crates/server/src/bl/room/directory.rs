use diesel::prelude::*;

use crate::core::RoomId;
use crate::schema::*;
use crate::{db, AppResult};

#[tracing::instrument]
pub fn set_public(room_id: &RoomId, value: bool) -> AppResult<()> {
    diesel::update(rooms::table.find(room_id))
        .set(rooms::is_public.eq(value))
        .execute(&mut db::connect()?)?;
    Ok(())
}

#[tracing::instrument]
pub fn is_public(room_id: &RoomId) -> AppResult<bool> {
    rooms::table
        .find(room_id)
        .select(rooms::is_public)
        .first::<bool>(&mut *db::connect()?)
        .map_err(Into::into)
}
