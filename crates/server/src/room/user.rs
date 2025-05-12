use std::collections::HashSet;

use diesel::prelude::*;

use crate::AppResult;
use crate::core::Seqnum;
use crate::core::events::AnyStrippedStateEvent;
use crate::core::events::room::member::MembershipState;
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};

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
    .execute(&mut connect()?)?;
    Ok(())
}

pub fn notification_count(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
    event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .select(event_push_summaries::notification_count)
        .first::<i64>(&mut connect()?)
        .optional()
        .map(|v| v.unwrap_or_default() as u64)
        .map_err(Into::into)
}

pub fn highlight_count(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
    event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .select(event_push_summaries::notification_count)
        .first::<i64>(&mut connect()?)
        .optional()
        .map(|v| v.unwrap_or_default() as u64)
        .map_err(Into::into)
}

pub fn last_notification_read(user_id: &UserId, room_id: &RoomId) -> AppResult<Seqnum> {
    event_receipts::table
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::room_id.eq(room_id))
        .order_by(event_receipts::occur_sn.desc())
        .select(event_receipts::occur_sn)
        .first::<Seqnum>(&mut connect()?)
        .optional()
        .map(|v| v.unwrap_or_default())
        .map_err(Into::into)
}

pub fn get_shared_rooms(user_ids: Vec<OwnedUserId>) -> AppResult<Vec<OwnedRoomId>> {
    let mut user_rooms: Vec<(OwnedUserId, Vec<OwnedRoomId>)> = Vec::new();
    for user_id in user_ids {
        let room_ids = room_users::table
            .filter(room_users::user_id.eq(&user_id))
            .select(room_users::room_id)
            .load::<OwnedRoomId>(&mut connect()?)?;
        user_rooms.push((user_id, room_ids));
    }

    let mut shared_rooms = user_rooms.pop().map(|i| i.1).unwrap_or_default();
    if shared_rooms.is_empty() {
        return Ok(shared_rooms);
    }
    while let Some((user_id, room_ids)) = user_rooms.pop() {
        let set1: HashSet<_> = shared_rooms.into_iter().collect();
        let set2: HashSet<_> = room_ids.into_iter().collect();
        shared_rooms = set1.intersection(&set2).cloned().collect();
        if shared_rooms.is_empty() {
            return Ok(shared_rooms);
        }
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
            .load::<OwnedUserId>(&mut connect()?)
            .map_err(Into::into)
    } else {
        e2e_key_changes::table
            .filter(e2e_key_changes::room_id.eq(room_id.as_str()))
            .filter(e2e_key_changes::occur_sn.ge(from_sn))
            .select(e2e_key_changes::user_id)
            .load::<OwnedUserId>(&mut connect()?)
            .map_err(Into::into)
    }
}

pub fn join_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::event_sn)
        .first::<i64>(&mut connect()?)
        .map_err(Into::into)
}
pub fn join_count(room_id: &RoomId) -> AppResult<i64> {
    let count = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::user_id)
        .count()
        .get_result(&mut connect()?)?;
    Ok(count)
}

pub fn knock_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("knock"))
        .select(room_users::event_sn)
        .first::<i64>(&mut connect()?)
        .map_err(Into::into)
}
pub fn knock_count(room_id: &RoomId) -> AppResult<i64> {
    let count = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("knock"))
        .select(room_users::user_id)
        .count()
        .get_result(&mut connect()?)?;
    Ok(count)
}
pub fn leave_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("leave"))
        .select(room_users::event_sn)
        .first::<i64>(&mut connect()?)
        .map_err(Into::into)
}

#[tracing::instrument]
pub fn is_invited(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let query = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq(MembershipState::Invite.to_string()));
    diesel_exists!(query, &mut connect()?).map_err(Into::into)
}

#[tracing::instrument]
pub fn is_left(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let left = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .order_by(room_users::id.desc())
        .select(room_users::membership)
        .first::<String>(&mut connect()?)
        .map(|m| m == MembershipState::Leave.to_string())
        .optional()?
        .unwrap_or(true);
    Ok(left)
}

#[tracing::instrument]
pub fn is_knocked<'a>(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let query = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq(MembershipState::Knock.to_string()));
    diesel_exists!(query, &mut connect()?).map_err(Into::into)
}

#[tracing::instrument]
pub fn once_joined(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let query = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq(MembershipState::Join.to_string()));

    diesel_exists!(query, &mut connect()?).map_err(Into::into)
}

#[tracing::instrument]
pub fn is_joined(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    println!(
        "=======###====user id: {user_id:?}  room_id: {room_id:?}  membership: {:?}",
        room_users::table
            .filter(room_users::user_id.eq(user_id))
            .filter(room_users::room_id.eq(room_id))
            .order_by(room_users::id.desc())
            .select(room_users::membership)
            .first::<String>(&mut connect()?)
    );
    let joined = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .order_by(room_users::id.desc())
        .select(room_users::membership)
        .first::<String>(&mut connect()?)
        .map(|m| m == MembershipState::Join.to_string())
        .optional()?
        .unwrap_or(false);
    Ok(joined)
}

#[tracing::instrument(level = "trace")]
pub fn invite_state(user_id: &UserId, room_id: &RoomId) -> AppResult<Vec<RawJson<AnyStrippedStateEvent>>> {
    if let Some(state) = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq(MembershipState::Invite.to_string()))
        .select(room_users::state_data)
        .first::<Option<JsonValue>>(&mut connect()?)
        .optional()?
        .flatten()
    {
        Ok(serde_json::from_value(state)?)
    } else {
        Ok(vec![])
    }
}

#[tracing::instrument(level = "trace")]
pub fn membership(user_id: &UserId, room_id: &RoomId) -> Option<MembershipState> {
    if is_joined(user_id, room_id).unwrap_or(false) {
        return Some(MembershipState::Join);
    }
    if is_left(user_id, room_id).unwrap_or(false) {
        return Some(MembershipState::Leave);
    }
    if is_knocked(user_id, room_id).unwrap_or(false) {
        return Some(MembershipState::Knock);
    }
    if is_invited(user_id, room_id).unwrap_or(false) {
        return Some(MembershipState::Invite);
    }
    if once_joined(user_id, room_id).unwrap_or(false) {
        return Some(MembershipState::Ban);
    }
    None
}
