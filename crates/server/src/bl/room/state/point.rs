use std::sync::Arc;

use diesel::prelude::*;
use palpo_core::OwnedEventId;

use crate::core::{EventId, RoomId};
use crate::schema::*;
use crate::{AppResult, db};

/// Returns (state_hash, already_existed)
pub fn ensure_point(room_id: &RoomId, event_id: &EventId, event_sn: i64) -> AppResult<i64> {
    let id = diesel::insert_into(room_state_points::table)
        .values((
            room_state_points::room_id.eq(room_id),
            room_state_points::event_id.eq(event_id),
            room_state_points::event_sn.eq(event_sn),
        ))
        .on_conflict_do_nothing()
        .returning(room_state_points::id)
        .get_result(&mut *db::connect()?)
        .optional()?;
    if let Some(id) = id {
        println!("Point already existed: {:?}", id);
        Ok(id)
    } else {
        room_state_points::table
            .filter(room_state_points::room_id.eq(room_id))
            .filter(room_state_points::event_id.eq(event_id))
            .select(room_state_points::id)
            .first(&mut *db::connect()?)
            .map_err(Into::into)
    }
}

pub fn update_point_frame_id(point_id: i64, frame_id: i64) -> AppResult<()> {
    println!(
        "Updating point frame_id: {} -> {}  {:#?}",
        point_id,
        frame_id,
        room_state_points::table
            .select((
                room_state_points::id,
                room_state_points::room_id,
                room_state_points::event_id,
                room_state_points::event_sn,
                room_state_points::frame_id
            ))
            .load::<(i64, String, String, i64, Option<i64>)>(&mut db::connect()?)?
    );
    diesel::update(room_state_points::table.filter(room_state_points::id.eq(point_id)))
        .set(room_state_points::frame_id.eq(frame_id))
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn get_point_event_id(point_id: i64) -> AppResult<Arc<EventId>> {
    room_state_points::table
        .find(point_id)
        .select(room_state_points::event_id)
        .first::<OwnedEventId>(&mut *db::connect()?)
        .map(|v| v.into())
        .map_err(Into::into)
}
