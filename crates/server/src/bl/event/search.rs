use std::collections::BTreeMap;

use diesel::prelude::*;

use crate::core::client::search::{Criteria, EventContextResult, ResultRoomEvents, SearchResult};
use crate::core::events::TimelineEventType;
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonObject;
use crate::full_text_search::*;
use crate::schema::*;
use crate::{db, AppResult, MatrixError, PduEvent};

pub fn search_pdus(user_id: &UserId, criteria: &Criteria, next_batch: Option<&str>) -> AppResult<ResultRoomEvents> {
    let filter = &criteria.filter;

    let room_ids = filter
        .rooms
        .clone()
        .unwrap_or_else(|| crate::user::joined_rooms(user_id, 0).unwrap_or_default());

    // Use limit or else 10, with maximum 100
    let limit = filter.limit.unwrap_or(10).min(100) as usize;

    for room_id in &room_ids {
        if !crate::room::is_joined(user_id, room_id)? {
            return Err(MatrixError::forbidden("You don't have permission to view this room.").into());
        }
    }

    let mut base_query = event_searches::table
        .filter(event_searches::room_id.eq_any(&room_ids))
        .filter(to_tsvector(event_searches::vector).matches(websearch_to_tsquery(&criteria.search_term)));
    let mut data_query = base_query.clone().into_boxed();
    let mut count_query = base_query.clone().into_boxed();
    if let Some(next_batch) = next_batch {
        let next_batch: i64 = next_batch.parse()?;
        data_query = data_query.filter(event_searches::origin_server_ts.le(next_batch))
    }
    let items = data_query
        .select((
            ts_rank_cd(
                to_tsvector(event_searches::vector),
                websearch_to_tsquery(&criteria.search_term),
            ),
            // event_searches::room_id,
            event_searches::event_id,
            event_searches::origin_server_ts,
            // event_searches::stream_ordering,
        ))
        .order_by(event_searches::origin_server_ts.desc())
        .limit(limit as i64)
        .load::<(f32, OwnedEventId, i64)>(&mut *db::connect()?)?;
    let count: i64 = count_query.count().first(&mut *db::connect()?)?;
    let next_batch = if items.len() < limit {
        None
    } else if let Some(last) = items.last() {
        Some(last.2.to_string())
    } else {
        None
    };

    let results: Vec<_> = items
        .into_iter()
        .filter_map(|(rank, event_id, _)| {
            crate::room::timeline::get_pdu(&event_id)
                .ok()?
                .filter(|pdu| {
                    crate::room::state::user_can_see_event(user_id, &pdu.room_id, &pdu.event_id).unwrap_or(false)
                })
                .map(|pdu| (rank, pdu.to_room_event()))
        })
        .map(|(rank, event)| SearchResult {
            context: EventContextResult {
                end: None,
                events_after: Vec::new(),
                events_before: Vec::new(),
                profile_info: BTreeMap::new(),
                start: None,
            },
            rank: Some(rank as f64),
            result: Some(event),
        })
        .collect();

    Ok(ResultRoomEvents {
        count: Some((results.len() as u32).into()),
        groups: BTreeMap::new(),
        next_batch,
        results,
        state: BTreeMap::new(), // TODO
        highlights: criteria
            .search_term
            .split_terminator(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .collect(),
    })
}

pub fn save_pdu(pdu: &PduEvent, pdu_json: &CanonicalJsonObject) -> AppResult<()> {
    let Some(content) = pdu_json.get("content") else {
        return Ok(());
    };
    let Some((key, vector)) = (match pdu.event_ty {
        TimelineEventType::RoomName => pdu_json.get("name").map(|v| (("content.name", v.to_string()))),
        TimelineEventType::RoomTopic => pdu_json.get("topic").map(|v| (("content.topic", v.to_string()))),
        TimelineEventType::RoomMessage => pdu_json.get("message").map(|v| (("content.message", v.to_string()))),
        TimelineEventType::RoomRedaction => {
            // TODO: Redaction
            return Ok(());
        }
        _ => return Ok(()),
    }) else {
        return Ok(());
    };
    diesel::insert_into(event_searches::table)
        .values((
            event_searches::event_id.eq(pdu.event_id.as_str()),
            event_searches::room_id.eq(pdu.room_id.as_str()),
            event_searches::key.eq(key),
            event_searches::vector
                .eq(diesel::dsl::sql::<TsVector>("to_tsvector('english', '?')")
                    .bind::<diesel::sql_types::Text, _>(&vector)),
            event_searches::origin_server_ts.eq(pdu.origin_server_ts),
        ))
        .execute(&mut *db::connect()?)?;

    Ok(())
}
