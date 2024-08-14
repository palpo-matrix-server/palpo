use std::collections::HashSet;
use std::mem::size_of;
use std::ops::Deref;
use std::sync::Arc;

use diesel::prelude::*;
use lru_cache::LruCache;

use super::{room_state_deltas, DbRoomStateDelta};
use crate::room::state::ensure_point;
use crate::schema::*;
use crate::{
    core::{EventId, RoomId},
    room,
};
use crate::{db, utils, AppError, AppResult};

pub struct StateDiff {
    pub parent_id: Option<i64>,
    pub append_data: Arc<HashSet<CompressedStateEvent>>,
    pub remove_data: Arc<HashSet<CompressedStateEvent>>,
}
#[derive(Eq, Hash, PartialEq, Copy, Debug, Clone)]
pub struct CompressedStateEvent([u8; 2 * size_of::<i64>()]);
impl CompressedStateEvent {
    pub fn new(field_id: i64, point_id: i64) -> Self {
        let mut v = field_id.to_be_bytes().to_vec();
        v.extend_from_slice(&point_id.to_be_bytes());
        Self(v.try_into().expect("we checked the size above"))
    }
    pub fn field_id(&self) -> i64 {
        utils::i64_from_bytes(&self.0[0..size_of::<i64>()]).expect("bytes have right length")
    }
    pub fn point_id(&self) -> i64 {
        utils::i64_from_bytes(&self.0[size_of::<i64>()..]).expect("bytes have right length")
    }
    /// Returns state_key_id, event id
    pub fn split(&self) -> AppResult<(i64, Arc<EventId>)> {
        Ok((
            utils::i64_from_bytes(&self[0..size_of::<i64>()]).expect("bytes have right length"),
            crate::room::state::get_point_event_id(
                utils::i64_from_bytes(&self[size_of::<i64>()..]).expect("bytes have right length"),
            )?,
        ))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
impl Deref for CompressedStateEvent {
    type Target = [u8; 2 * size_of::<i64>()];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn compress_event(
    room_id: &RoomId,
    field_id: i64,
    event_id: &EventId,
    event_sn: i64,
) -> AppResult<CompressedStateEvent> {
    let point_id = ensure_point(room_id, event_id, event_sn)?;
    Ok(CompressedStateEvent::new(field_id, point_id))
}

pub fn get_detla(frame_id: i64) -> AppResult<DbRoomStateDelta> {
    room_state_deltas::table
        .find(frame_id)
        .first::<DbRoomStateDelta>(&mut *db::connect()?)
        .map_err(Into::into)
}
pub fn load_state_diff(frame_id: i64) -> AppResult<StateDiff> {
    let DbRoomStateDelta {
        parent_id,
        append_data,
        remove_data,
        ..
    } = room_state_deltas::table
        .find(frame_id)
        .first::<DbRoomStateDelta>(&mut *db::connect()?)?;
    Ok(StateDiff {
        parent_id,
        append_data: Arc::new(
            append_data
                .chunks_exact(size_of::<CompressedStateEvent>())
                .map(|chunk| CompressedStateEvent(chunk.try_into().expect("we checked the size above")))
                .collect(),
        ),
        remove_data: Arc::new(
            remove_data
                .chunks_exact(size_of::<CompressedStateEvent>())
                .map(|chunk| CompressedStateEvent(chunk.try_into().expect("we checked the size above")))
                .collect(),
        ),
    })
}

pub fn save_state_delta(room_id: &RoomId, frame_id: i64, diff: StateDiff) -> AppResult<()> {
    let StateDiff {
        parent_id,
        append_data,
        remove_data,
    } = diff;
    diesel::insert_into(room_state_deltas::table)
        .values(DbRoomStateDelta {
            frame_id,
            room_id: room_id.to_owned(),
            parent_id,
            append_data: append_data
                .iter()
                .flat_map(|event| event.as_bytes())
                .cloned()
                .collect::<Vec<_>>(),
            remove_data: remove_data
                .iter()
                .flat_map(|event| event.as_bytes())
                .cloned()
                .collect::<Vec<_>>(),
        })
        .execute(&mut db::connect()?)?;
    Ok(())
}
/// Creates a new state_hash that often is just a diff to an already existing
/// state_hash and therefore very efficient.
///
/// There are multiple layers of diffs. The bottom layer 0 always contains the full state. Layer
/// 1 contains diffs to states of layer 0, layer 2 diffs to layer 1 and so on. If layer n > 0
/// grows too big, it will be combined with layer n-1 to create a new diff on layer n-1 that's
/// based on layer n-2. If that layer is also too big, it will recursively fix above layers too.
///
/// * `point_id` - Shortstate_hash of this state
/// * `append_data` - Added to base. Each vec is state_key_id+shorteventid
/// * `remove_data` - Removed from base. Each vec is state_key_id+shorteventid
/// * `diff_to_sibling` - Approximately how much the diff grows each time for this layer
/// * `parent_states` - A stack with info on state_hash, full state, added diff and removed diff for each parent layer
#[tracing::instrument(skip(append_data, remove_data, diff_to_sibling, parent_states))]
pub fn calc_and_save_state_delta(
    room_id: &RoomId,
    frame_id: i64,
    append_data: Arc<HashSet<CompressedStateEvent>>,
    remove_data: Arc<HashSet<CompressedStateEvent>>,
    diff_to_sibling: usize,
    mut parent_states: Vec<(
        i64,                                // sstate_hash
        Arc<HashSet<CompressedStateEvent>>, // full state
        Arc<HashSet<CompressedStateEvent>>, // added
        Arc<HashSet<CompressedStateEvent>>, // removed
    )>,
) -> AppResult<()> {
    let diff_sum = append_data.len() + remove_data.len();

    if parent_states.len() > 3 {
        // Number of layers
        // To many layers, we have to go deeper
        let parent = parent_states.pop().unwrap();

        let mut parent_new = (*parent.2).clone();
        let mut parent_removed = (*parent.3).clone();

        for removed in remove_data.iter() {
            if !parent_new.remove(removed) {
                // It was not added in the parent and we removed it
                parent_removed.insert(removed.clone());
            }
            // Else it was added in the parent and we removed it again. We can forget this change
        }

        for new in append_data.iter() {
            if !parent_removed.remove(new) {
                // It was not touched in the parent and we added it
                parent_new.insert(new.clone());
            }
            // Else it was removed in the parent and we added it again. We can forget this change
        }

        return calc_and_save_state_delta(
            room_id,
            frame_id,
            Arc::new(parent_new),
            Arc::new(parent_removed),
            diff_sum,
            parent_states,
        );
    }

    if parent_states.is_empty() {
        // There is no parent layer, create a new state
        return save_state_delta(
            room_id,
            frame_id,
            StateDiff {
                parent_id: None,
                append_data,
                remove_data,
            },
        );
    }

    // Else we have two options.
    // 1. We add the current diff on top of the parent layer.
    // 2. We replace a layer above

    let parent = parent_states.pop().unwrap();
    let parent_diff = parent.2.len() + parent.3.len();

    if diff_sum * diff_sum >= 2 * diff_to_sibling * parent_diff {
        // Diff too big, we replace above layer(s)
        let mut parent_new = (*parent.2).clone();
        let mut parent_removed = (*parent.3).clone();

        for removed in remove_data.iter() {
            if !parent_new.remove(removed) {
                // It was not added in the parent and we removed it
                parent_removed.insert(removed.clone());
            }
            // Else it was added in the parent and we removed it again. We can forget this change
        }

        for new in append_data.iter() {
            if !parent_removed.remove(new) {
                // It was not touched in the parent and we added it
                parent_new.insert(new.clone());
            }
            // Else it was removed in the parent and we added it again. We can forget this change
        }

        calc_and_save_state_delta(
            room_id,
            frame_id,
            Arc::new(parent_new),
            Arc::new(parent_removed),
            diff_sum,
            parent_states,
        )
    } else {
        // Diff small enough, we add diff as layer on top of parent
        save_state_delta(
            room_id,
            frame_id,
            StateDiff {
                parent_id: Some(parent.0),
                append_data,
                remove_data,
            },
        )
    }
}
