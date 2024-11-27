use diesel::prelude::*;

use crate::core::events::StateEventType;
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_state_fields)]
pub struct DbRoomStateField {
    pub id: i64,
    pub event_ty: StateEventType,
    pub state_key: String,
}

pub fn get_field(field_id: i64) -> AppResult<DbRoomStateField> {
    room_state_fields::table
        .find(field_id)
        .first::<DbRoomStateField>(&mut *db::connect()?)
        .map_err(Into::into)
}
pub fn get_field_id(event_ty: &StateEventType, state_key: &str) -> AppResult<Option<i64>> {
    room_state_fields::table
        .filter(room_state_fields::event_ty.eq(event_ty))
        .filter(room_state_fields::state_key.eq(state_key))
        .select(room_state_fields::id)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}
pub fn ensure_field_id(event_ty: &StateEventType, state_key: &str) -> AppResult<i64> {
    let id = diesel::insert_into(room_state_fields::table)
        .values((
            room_state_fields::event_ty.eq(event_ty),
            room_state_fields::state_key.eq(state_key),
        ))
        .on_conflict_do_nothing()
        .returning(room_state_fields::id)
        .get_result::<i64>(&mut *db::connect()?)
        .optional()?;
    if let Some(id) = id {
        Ok(id)
    } else {
        room_state_fields::table
            .filter(room_state_fields::event_ty.eq(event_ty))
            .filter(room_state_fields::state_key.eq(state_key))
            .select(room_state_fields::id)
            .first::<i64>(&mut *db::connect()?)
            .map_err(Into::into)
    }
}
pub fn ensure_field(event_ty: &StateEventType, state_key: &str) -> AppResult<DbRoomStateField> {
    let id = diesel::insert_into(room_state_fields::table)
        .values((
            room_state_fields::event_ty.eq(event_ty),
            room_state_fields::state_key.eq(state_key),
        ))
        .on_conflict_do_nothing()
        .returning(room_state_fields::id)
        .get_result::<i64>(&mut *db::connect()?)
        .optional()?;
    if let Some(id) = id {
        room_state_fields::table
            .find(id)
            .first::<DbRoomStateField>(&mut *db::connect()?)
            .map_err(Into::into)
    } else {
        room_state_fields::table
            .filter(room_state_fields::event_ty.eq(event_ty))
            .filter(room_state_fields::state_key.eq(state_key))
            .first::<DbRoomStateField>(&mut *db::connect()?)
            .map_err(Into::into)
    }
}
