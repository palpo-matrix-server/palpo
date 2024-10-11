use std::collections::HashSet;

use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;
use crate::{db, AppResult, JsonValue};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_users)]
pub struct DbRoomUser {
    pub id: i64,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub sender_id: OwnedUserId,
    pub membership: String,
    pub forgotten: bool,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub state_data: Option<JsonValue>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = room_users)]
pub struct NewDbRoomUser {
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub sender_id: OwnedUserId,
    pub membership: String,
    pub forgotten: bool,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub state_data: Option<JsonValue>,
    pub created_at: UnixMillis,
}

pub fn reset_notification_counts(user_id: &UserId, room_id: &RoomId) -> AppResult<()> {
    diesel::update(
        event_push_summaries::table
            .filter(event_push_summaries::user_id.eq(user_id))
            .filter(event_push_summaries::room_id.eq(room_id)),
    )
    .set((
        event_push_summaries::notification_count.eq(0),
        event_push_summaries::unread_count.eq(0),
    ))
    .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn notification_count(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
    event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .select(event_push_summaries::notification_count)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map(|v| v.unwrap_or_default() as u64)
        .map_err(Into::into)
}

pub fn highlight_count(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
    event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .select(event_push_summaries::notification_count)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map(|v| v.unwrap_or_default() as u64)
        .map_err(Into::into)
}

pub fn last_notification_read(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    event_receipts::table
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::room_id.eq(room_id))
        .order_by(event_receipts::event_sn.desc())
        .select(event_receipts::event_sn)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map(|v| v.unwrap_or_default())
        .map_err(Into::into)
}

pub fn get_event_frame_id(room_id: &RoomId, event_sn: i64) -> AppResult<Option<i64>> {
    room_state_points::table
        .filter(room_state_points::room_id.eq(room_id))
        .filter(room_state_points::event_sn.eq(event_sn))
        .select(room_state_points::frame_id)
        .first::<Option<i64>>(&mut *db::connect()?)
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

pub fn get_shared_rooms(user_ids: Vec<OwnedUserId>) -> AppResult<Vec<OwnedRoomId>> {
    let mut user_rooms: Vec<(OwnedUserId, Vec<OwnedRoomId>)> = Vec::new();
    for user_id in user_ids {
        let room_ids = room_users::table
            .filter(room_users::user_id.eq(&user_id))
            .select(room_users::room_id)
            .load::<OwnedRoomId>(&mut *db::connect()?)?;
        user_rooms.push((user_id, room_ids));
    }

    let mut shared_rooms = user_rooms.pop().map(|i| i.1).unwrap_or_default();
    while let Some((user_id, room_ids)) = user_rooms.pop() {
        let set1: HashSet<_> = shared_rooms.into_iter().collect();
        let set2: HashSet<_> = room_ids.into_iter().collect();
        shared_rooms = set1.intersection(&set2).cloned().collect();
    }
    Ok(shared_rooms)
}

pub fn keys_changed_users(room_id: &RoomId, from_sn: i64, to_sn: Option<i64>) -> AppResult<Vec<OwnedUserId>> {
    if let Some(to_sn) = to_sn {
        e2e_key_changes::table
            .filter(e2e_key_changes::room_id.eq(room_id))
            .filter(e2e_key_changes::occur_sn.ge(from_sn))
            .filter(e2e_key_changes::occur_sn.le(to_sn))
            .select(e2e_key_changes::user_id)
            .load::<OwnedUserId>(&mut *db::connect()?)
            .map_err(Into::into)
    } else {
        e2e_key_changes::table
            .filter(e2e_key_changes::room_id.eq(room_id.as_str()))
            .filter(e2e_key_changes::occur_sn.ge(from_sn))
            .select(e2e_key_changes::user_id)
            .load::<OwnedUserId>(&mut *db::connect()?)
            .map_err(Into::into)
    }
}

pub fn joined_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::event_sn)
        .first::<i64>(&mut *db::connect()?)
        .map_err(Into::into)
}
pub fn joined_count(room_id: &RoomId) -> AppResult<i64> {
    let count = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::user_id)
        .count()
        .get_result(&mut *db::connect()?)?;
    Ok(count)
}
