use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use diesel::prelude::*;
use lru_cache::LruCache;

use super::{CompressedState, StateDiff};
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{db, AppResult};

pub static STATE_INFO_CACHE: LazyLock<
    Mutex<
        LruCache<
            i64,
            Vec<FrameInfo>,
        >,
    >,
> = LazyLock::new(|| Mutex::new(LruCache::new(100_000)));

#[derive(Clone, Default)]
pub struct FrameInfo {
    pub frame_id: i64,
    pub full_state: Arc<HashSet<CompressedState>>,
    pub appended: Arc<HashSet<CompressedState>>,
    pub disposed: Arc<HashSet<CompressedState>>,
}

/// Returns a stack with info on state_hash, full state, added diff and removed diff for the selected state_hash and each parent layer.
pub fn load_frame_info(frame_id: i64) -> AppResult<Vec<FrameInfo>> {
    if let Some(r) = STATE_INFO_CACHE.lock().unwrap().get_mut(&frame_id) {
        return Ok(r.clone());
    }

    println!("llllllllllload_frame_info frame_id: {frame_id}");

    let StateDiff {
        parent_id,
        appended,
        disposed,
    } = super::load_state_diff(frame_id)?;

    if let Some(parent_id) = parent_id {
		println!("load_frame_info frame_id: {frame_id}, parent is {parent_id}");
        let mut info = load_frame_info(parent_id)?;
        let mut full_state = (*info.last().expect("at least one frame").full_state).clone();
        full_state.extend(appended.iter().copied());
        let disposed = (*disposed).clone();
        for r in &disposed {
            println!("RRRRRREvmove state: {r:?}   {:?}", r.split().unwrap());
            full_state.remove(r);
        }

        info.push(FrameInfo {
            frame_id,
            full_state: Arc::new(full_state),
            appended,
            disposed: Arc::new(disposed),
        });
        STATE_INFO_CACHE.lock().unwrap().insert(frame_id, info.clone());

        Ok(info)
    } else {
        println!("load_frame_info frame_id: {frame_id} parent is none");
        let info = vec![FrameInfo {
            frame_id: frame_id,
            full_state: appended.clone(),
            appended,
            disposed,
        }];
        STATE_INFO_CACHE.lock().unwrap().insert(frame_id, info.clone());
        Ok(info)
    }
}

pub fn get_room_frame_id(room_id: &RoomId, until_sn: Option<i64>) -> AppResult<Option<i64>> {
    if let Some(until_sn) = until_sn {
        room_state_points::table
            .filter(room_state_points::room_id.eq(room_id))
            .filter(room_state_points::event_sn.le(until_sn))
            .select(room_state_points::frame_id)
            .order(room_state_points::event_sn.desc())
            .first::<Option<i64>>(&mut *db::connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    } else {
        rooms::table
            .find(room_id)
            .select(rooms::state_frame_id)
            .first::<Option<i64>>(&mut *db::connect()?)
            .optional()
            .map(|v| v.flatten())
            .map_err(Into::into)
    }
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
