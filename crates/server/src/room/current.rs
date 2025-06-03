use diesel::prelude::*;

use crate::AppResult;
use crate::core::identifiers::*;
use crate::data::connect;
use crate::data::schema::*;

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = stats_room_currents, primary_key(room_id))]
pub struct RoomCurrent {
    pub room_id: OwnedRoomId,
    pub state_events: i64,
    pub joined_members: i64,
    pub invited_members: i64,
    pub left_members: i64,
    pub banned_members: i64,
    pub knocked_members: i64,
    pub local_users_in_room: i64,
    pub completed_delta_stream_id: i64,
}

#[tracing::instrument]
pub fn get_current(room_id: &RoomId) -> AppResult<Option<RoomCurrent>> {
    stats_room_currents::table
        .filter(stats_room_currents::room_id.eq(room_id))
        .first::<RoomCurrent>(&mut connect()?)
        .optional()
        .map_err(Into::into)
}

#[tracing::instrument]
pub fn get_invite_count(room_id: &RoomId, user_id: &UserId) -> AppResult<Option<u64>> {
    let count = stats_room_currents::table
        .filter(stats_room_currents::room_id.eq(room_id))
        .select(stats_room_currents::invited_members)
        .first::<i64>(&mut connect()?)
        .optional()?;
    Ok(count.map(|c| c as u64))
}

#[tracing::instrument]
pub fn get_left_count(room_id: &RoomId, user_id: &UserId) -> AppResult<Option<u64>> {
    let count = stats_room_currents::table
        .filter(stats_room_currents::room_id.eq(room_id))
        .select(stats_room_currents::left_members)
        .first::<i64>(&mut connect()?)
        .optional()?;
    Ok(count.map(|c| c as u64))
}

#[tracing::instrument]
pub fn get_left_stamp_sn(room_id: &RoomId, user_id: &UserId) -> AppResult<Option<Seqnum>> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("leave").or(room_users::membership.eq("ban")))
        .select(room_users::stamp_sn)
        .first::<i64>(&mut connect()?)
        .optional()
        .map_err(Into::into)
}
