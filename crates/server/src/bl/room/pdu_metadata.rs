use diesel::prelude::*;
use serde::Deserialize;

use crate::core::client::relation::RelationEventsResBody;
use crate::core::identifiers::*;
use crate::core::{
    events::{relation::RelationType, TimelineEventType},
    Direction, EventId, RoomId, UserId,
};
use crate::event::PduEvent;
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Clone, Debug, Deserialize)]
struct ExtractRelType {
    rel_type: RelationType,
}
#[derive(Clone, Debug, Deserialize)]
struct ExtractRelatesToEventId {
    #[serde(rename = "m.relates_to")]
    relates_to: ExtractRelType,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_relations)]
pub struct DbEventRelation {
    pub id: i64,

    pub room_id: OwnedRoomId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub child_id: OwnedEventId,
    pub child_sn: i64,
    pub child_event_type: String,
    pub rel_type: Option<String>,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = event_relations)]
pub struct NewDbEventRelation {
    pub room_id: OwnedRoomId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub child_id: OwnedEventId,
    pub child_sn: i64,
    pub child_event_type: String,
    pub rel_type: Option<String>,
}

#[tracing::instrument]
pub fn add_relation(
    room_id: &RoomId,
    event_id: &EventId,
    child_id: &EventId,
    child_event_type: String,
    rel_type: Option<RelationType>,
) -> AppResult<()> {
    let event_sn = crate::event::get_event_sn(event_id)?;
    let child_sn = crate::event::get_event_sn(child_id)?;
    diesel::insert_into(event_relations::table)
        .values(&NewDbEventRelation {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
            event_sn,
            child_id: child_id.to_owned(),
            child_sn,
            child_event_type,
            rel_type: rel_type.map(|v| v.to_string()),
        })
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn paginate_relations_with_filter(
    user_id: &UserId,
    room_id: &RoomId,
    target: &EventId,
    filter_event_type: Option<TimelineEventType>,
    filter_rel_type: Option<RelationType>,
    from: Option<&str>,
    to: Option<&str>,
    limit: Option<usize>,
    recurse: bool,
    dir: Direction,
) -> AppResult<RelationEventsResBody> {
    let from: Option<i64> = from.map(|from| from.parse()).transpose()?;
    let to: Option<i64> = to.map(|to| to.parse()).transpose()?;

    // Use limit or else 10, with maximum 100
    let limit = limit
        .and_then(|u| u32::try_from(u).ok())
        .map_or(10_usize, |u| u as usize)
        .min(100);

    // Spec (v1.10) recommends depth of at least 3
    let depth: u8 = if recurse { 3 } else { 1 };

    let next_token;

    match dir {
        crate::core::Direction::Forward => {
            let events_after: Vec<_> = crate::room::pdu_metadata::get_relations(
                user_id,
                room_id,
                target,
                filter_event_type.as_ref(),
                filter_rel_type.as_ref(),
                from,
                to,
                limit,
            )? // TODO: should be relations_after
            .into_iter()
            .filter(|(_, pdu)| {
                filter_event_type.as_ref().map_or(true, |t| &pdu.kind == t)
                    && if let Ok(content) = serde_json::from_str::<ExtractRelatesToEventId>(pdu.content.get()) {
                        filter_rel_type
                            .as_ref()
                            .map_or(true, |r| &content.relates_to.rel_type == r)
                    } else {
                        false
                    }
            })
            .filter(|(_, pdu)| {
                crate::room::state::user_can_see_event(user_id, &room_id, &pdu.event_id).unwrap_or(false)
            }) // Stop at `to`
            .collect();

            next_token = events_after.last().map(|(count, _)| count).copied();

            let events_after: Vec<_> = events_after
                .into_iter()
                .rev() // relations are always most recent first
                .map(|(_, pdu)| pdu.to_message_like_event())
                .collect();

            Ok(RelationEventsResBody {
                chunk: events_after,
                next_batch: next_token.map(|t| t.to_string()),
                prev_batch: from.map(|from| from.to_string()),
            })
        }
        crate::core::Direction::Backward => {
            let events_before: Vec<_> = crate::room::pdu_metadata::get_relations(
                user_id,
                &room_id,
                target,
                filter_event_type.as_ref(),
                filter_rel_type.as_ref(),
                from,
                to,
                limit,
            )?
            .into_iter()
            .filter(|(_, pdu)| {
                filter_event_type.as_ref().map_or(true, |t| &pdu.kind == t)
                    && if let Ok(content) = serde_json::from_str::<ExtractRelatesToEventId>(pdu.content.get()) {
                        filter_rel_type
                            .as_ref()
                            .map_or(true, |r| &content.relates_to.rel_type == r)
                    } else {
                        false
                    }
            })
            .filter(|(_, pdu)| {
                crate::room::state::user_can_see_event(user_id, &room_id, &pdu.event_id).unwrap_or(false)
            })
            .collect();

            next_token = events_before.last().map(|(count, _)| count).copied();

            let events_before: Vec<_> = events_before
                .into_iter()
                .map(|(_, pdu)| pdu.to_message_like_event())
                .collect();

            Ok(RelationEventsResBody {
                chunk: events_before,
                next_batch: next_token.map(|t| t.to_string()),
                prev_batch: from.map(|from| from.to_string()),
            })
        }
    }
}

pub fn get_relations(
    user_id: &UserId,
    room_id: &RoomId,
    event_id: &EventId,
    child_event_type: Option<&TimelineEventType>,
    rel_type: Option<&RelationType>,
    from: Option<i64>,
    to: Option<i64>,
    limit: usize,
) -> AppResult<Vec<(i64, PduEvent)>> {
    let mut query = event_relations::table
        .filter(event_relations::room_id.eq(room_id))
        .filter(event_relations::event_id.eq(event_id))
        .into_boxed();
    if let Some(child_event_type) = child_event_type {
        query = query.filter(event_relations::child_event_type.eq(child_event_type.to_string()));
    }
    if let Some(rel_type) = rel_type {
        query = query.filter(event_relations::rel_type.eq(rel_type.to_string()));
    }
    if let Some(from) = from {
        query = query.filter(event_relations::child_sn.ge(from));
    }
    if let Some(to) = to {
        query = query.filter(event_relations::child_sn.le(to));
    }
    let relations = query
        .order_by(event_relations::child_sn.desc())
        .limit(limit as i64)
        .load::<DbEventRelation>(&mut *db::connect()?)?;
    let mut pdus = Vec::with_capacity(relations.len());
    for relation in relations {
        if let Some(mut pdu) = crate::room::timeline::get_pdu(&relation.event_id)? {
            if pdu.sender != user_id {
                pdu.remove_transaction_id()?;
            }
            pdus.push((relation.event_sn, pdu));
        }
    }
    Ok(pdus)
}

// #[tracing::instrument(skip(room_id, event_ids))]
// pub fn mark_as_referenced(room_id: &RoomId, event_ids: &[Arc<EventId>]) -> AppResult<()> {
// for prev in event_ids {
//     let mut key = room_id.as_bytes().to_vec();
//     key.extend_from_slice(prev.as_bytes());
//     self.referencedevents.insert(&key, &[])?;
// }

//     Ok(())
// }

// pub fn is_event_referenced(room_id: &RoomId, event_id: &EventId) -> AppResult<bool> {
// let mut key = room_id.as_bytes().to_vec();
// key.extend_from_slice(event_id.as_bytes());
// Ok(self.referencedevents.get(&key)?.is_some())
// }

#[tracing::instrument(skip(event_id))]
pub fn mark_event_soft_failed(event_id: &EventId) -> AppResult<()> {
    diesel::update(events::table.filter(events::id.eq(event_id)))
        .set(events::soft_failed.eq(true))
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn is_event_soft_failed(event_id: &EventId) -> AppResult<bool> {
    events::table
        .filter(events::id.eq(event_id))
        .select(events::soft_failed)
        .first(&mut *db::connect()?)
        .map_err(Into::into)
}
