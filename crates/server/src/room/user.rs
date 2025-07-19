use std::collections::{BTreeMap, HashMap, HashSet};

use diesel::prelude::*;
use palpo_data::room::DbEvent;

use crate::core::Seqnum;
use crate::core::events::{AnySyncStateEvent, AnyStrippedStateEvent};
use crate::core::events::room::member::MembershipState;
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::data::room::DbEventPushSummary;
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::{AppError, AppResult, IsRemoteOrLocal, MatrixError, config, room};

#[derive(Debug, Clone)]
pub struct UserNotifySummary {
    pub notification_count: u64,
    pub unread_count: u64,
    pub highlight_count: u64,

    pub threads: BTreeMap<OwnedEventId, ThreadPushSummary>,
}

#[derive(Debug, Clone)]
pub struct ThreadPushSummary {
    pub notification_count: u64,
    pub unread_count: u64,
    pub highlight_count: u64,
}

impl UserNotifySummary {
    pub fn all_notification_count(&self) -> u64 {
        self.notification_count + self.threads.iter().map(|(_, t)| t.notification_count).sum::<u64>()
    }
    pub fn all_unread_count(&self) -> u64 {
        self.notification_count + self.threads.iter().map(|(_, t)| t.unread_count).sum::<u64>()
    }
    pub fn all_highlight_count(&self) -> u64 {
        self.highlight_count + self.threads.iter().map(|(_, t)| t.highlight_count).sum::<u64>()
    }
}

impl From<Vec<DbEventPushSummary>> for UserNotifySummary {
    fn from(summaries: Vec<DbEventPushSummary>) -> Self {
        let mut notification_count = 0;
        let mut unread_count = 0;
        let mut highlight_count = 0;
        let mut threads = BTreeMap::new();

        for summary in summaries {
            if let Some(thread_id) = summary.thread_id {
                threads.insert(
                    thread_id,
                    ThreadPushSummary {
                        notification_count: summary.notification_count as u64,
                        unread_count: summary.unread_count as u64,
                        highlight_count: summary.highlight_count as u64,
                    },
                );
            } else {
                notification_count += summary.notification_count as u64;
                unread_count += summary.unread_count as u64;
                highlight_count += summary.highlight_count as u64;
            }
        }

        UserNotifySummary {
            notification_count,
            unread_count,
            highlight_count,
            threads,
        }
    }
}

pub fn notify_summary(user_id: &UserId, room_id: &RoomId) -> AppResult<UserNotifySummary> {
    let summaries = event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .load::<DbEventPushSummary>(&mut connect()?)?;
    Ok(summaries.into())
}

pub fn highlight_count(user_id: &UserId, room_id: &RoomId) -> AppResult<u64> {
    event_push_summaries::table
        .filter(event_push_summaries::user_id.eq(user_id))
        .filter(event_push_summaries::room_id.eq(room_id))
        .select(event_push_summaries::highlight_count)
        .first::<i64>(&mut connect()?)
        .optional()
        .map(|v| v.unwrap_or_default() as u64)
        .map_err(Into::into)
}

pub fn last_read_notification(user_id: &UserId, room_id: &RoomId) -> AppResult<Seqnum> {
    event_receipts::table
        .filter(event_receipts::user_id.eq(user_id))
        .filter(event_receipts::room_id.eq(room_id))
        .order_by(event_receipts::event_sn.desc())
        .select(event_receipts::event_sn)
        .first::<Seqnum>(&mut connect()?)
        .optional()
        .map(|v| v.unwrap_or_default())
        .map_err(Into::into)
}

pub fn shared_rooms(user_ids: Vec<OwnedUserId>) -> AppResult<Vec<OwnedRoomId>> {
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
pub fn is_banned(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let query = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq(MembershipState::Ban.to_string()));
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
pub fn is_joined(user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
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

#[tracing::instrument]
pub fn left_sn(room_id: &RoomId, user_id: &UserId) -> AppResult<Seqnum> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq("leave").or(room_users::membership.eq("ban")))
        .select(room_users::event_sn)
        .first::<Seqnum>(&mut connect()?)
        .map_err(Into::into)
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
pub fn membership(user_id: &UserId, room_id: &RoomId) -> AppResult<MembershipState> {
    let membership = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .order_by(room_users::id.desc())
        .select(room_users::membership)
        .first::<String>(&mut connect()?)
        .optional()?;
    if let Some(membership) = membership {
        Ok(membership.into())
    } else {
        Err(MatrixError::not_found(format!("User {} is not a member of room {}", user_id, room_id)).into())
    }
}
/// Returns an iterator over all rooms a user left.
#[tracing::instrument]
pub fn left_rooms(
    user_id: &UserId,
    since_sn: Option<Seqnum>,
) -> AppResult<HashMap<OwnedRoomId, Vec<RawJson<AnySyncStateEvent>>>> {
    let query = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq_any(vec![
            MembershipState::Leave.to_string(),
            MembershipState::Ban.to_string(),
        ]))
        .into_boxed();
    let query = if let Some(since_sn) = since_sn {
        query.filter(room_users::event_sn.ge(since_sn))
    } else {
        query.filter(room_users::forgotten.eq(false))
    };
    let room_event_ids = query
        .select((room_users::room_id, room_users::event_id))
        .load::<(OwnedRoomId, OwnedEventId)>(&mut connect()?)
        .map(|rows| {
            let mut map: HashMap<OwnedRoomId, Vec<OwnedEventId>> = HashMap::new();
            for (room_id, event_id) in rows {
                map.entry(room_id).or_default().push(event_id);
            }
            map
        })?;
    let mut room_events = HashMap::new();
    for (room_id, event_ids) in room_event_ids {
        let events = event_data::table
            .filter(event_data::event_id.eq_any(&event_ids))
            .select(event_data::json_data)
            .load::<JsonValue>(&mut connect()?)?
            .into_iter()
            .filter_map(|value| RawJson::<AnySyncStateEvent>::from_value(&value).ok())
            .collect::<Vec<_>>();
        room_events.insert(room_id, events);
    }
    Ok(room_events)
}
