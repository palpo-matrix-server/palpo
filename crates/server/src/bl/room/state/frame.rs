use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use diesel::prelude::*;
use lru_cache::LruCache;

use super::{CompressedStateEvent, StateDiff};
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{db, AppResult};

pub static STATE_INFO_CACHE: LazyLock<
    Mutex<
        LruCache<
            i64,
            Vec<(
                i64,                                // state frame id
                Arc<HashSet<CompressedStateEvent>>, // full state
                Arc<HashSet<CompressedStateEvent>>, // added
                Arc<HashSet<CompressedStateEvent>>, // removed
            )>,
        >,
    >,
> = LazyLock::new(|| Mutex::new(LruCache::new(100_000)));

/// Returns a stack with info on state_hash, full state, added diff and removed diff for the selected state_hash and each parent layer.
pub fn load_frame_info(
    frame_id: i64,
) -> AppResult<
    Vec<(
        i64,                                // state frame id
        Arc<HashSet<CompressedStateEvent>>, // full state
        Arc<HashSet<CompressedStateEvent>>, // added
        Arc<HashSet<CompressedStateEvent>>, // removed
    )>,
> {
    if let Some(r) = STATE_INFO_CACHE.lock().unwrap().get_mut(&frame_id) {
        return Ok(r.clone());
    }

    let StateDiff {
        parent_id,
        append_data,
        remove_data,
    } = super::load_state_diff(frame_id)?;

    if let Some(parent_id) = parent_id {
        let mut response = load_frame_info(parent_id)?;
        let mut state = (*response.last().unwrap().1).clone();
        state.extend(append_data.iter().copied());
        let remove_data = (*remove_data).clone();
        for r in &remove_data {
            state.remove(r);
        }

        response.push((frame_id, Arc::new(state), append_data, Arc::new(remove_data)));
        STATE_INFO_CACHE.lock().unwrap().insert(frame_id, response.clone());

        Ok(response)
    } else {
        let response = vec![(frame_id, append_data.clone(), append_data, remove_data)];
        STATE_INFO_CACHE.lock().unwrap().insert(frame_id, response.clone());
        Ok(response)
    }
}

pub fn get_room_frame_id(room_id: &RoomId) -> AppResult<Option<i64>> {
    rooms::table
        .find(room_id)
        .select(rooms::state_frame_id)
        .first::<Option<i64>>(&mut *db::connect()?)
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

pub fn get_pdu_frame_id(event_id: &EventId) -> AppResult<Option<i64>> {
    room_state_points::table
        .filter(room_state_points::event_id.eq(event_id))
        .select(room_state_points::frame_id)
        .first::<Option<i64>>(&mut *db::connect()?)
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}
/// Returns (state_hash, already_existed)
pub fn ensure_frame(room_id: &RoomId, hash_data: Vec<u8>) -> AppResult<i64> {
    diesel::insert_into(room_state_frames::table)
        .values((
            room_state_frames::room_id.eq(room_id),
            room_state_frames::hash_data.eq(hash_data),
        ))
        .on_conflict_do_nothing()
        .returning(room_state_frames::id)
        .get_result(&mut *db::connect()?)
        .map_err(Into::into)
}
