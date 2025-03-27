use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::core::identifiers::*;
use crate::core::state::{self, StateMap};
use crate::event::PduEvent;
use crate::room::state::DbRoomStateField;
use crate::{AppError, AppResult};

pub(super) async fn state_at_incoming_degree_one(
    incoming_pdu: &PduEvent,
) -> AppResult<Option<HashMap<i64, Arc<EventId>>>> {
    let prev_event = &*incoming_pdu.prev_events[0];
    let Some(prev_frame_id) = crate::room::state::get_pdu_frame_id(prev_event)? else {
        return Ok(None);
    };

    let Ok(mut state) = crate::room::state::get_full_state_ids(prev_frame_id) else {
        return Ok(None);
    };

    debug!("Using cached state");
    let prev_pdu = crate::room::timeline::get_pdu(prev_event)
        .ok()
        .flatten()
        .ok_or_else(|| AppError::internal("Could not find prev event, but we know the state."))?;

    if let Some(state_key) = &prev_pdu.state_key {
        let state_key_id = crate::room::state::ensure_field_id(&prev_pdu.event_ty.to_string().into(), state_key)?;

        state.insert(state_key_id, Arc::from(prev_event));
        // Now it's the state after the pdu
    }

    Ok(Some(state))
}

pub(super) async fn state_at_incoming_resolved(
    incoming_pdu: &PduEvent,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
) -> AppResult<Option<HashMap<i64, Arc<EventId>>>> {
    debug!("Calculating state at event using state res");
    let mut extremity_sstate_hashes = HashMap::new();

    let mut okay = true;
    for prev_eventid in &incoming_pdu.prev_events {
        let prev_event = if let Ok(Some(pdu)) = crate::room::timeline::get_pdu(prev_eventid) {
            pdu
        } else {
            okay = false;
            break;
        };

        let sstate_hash = if let Ok(Some(s)) = crate::room::state::get_pdu_frame_id(prev_eventid) {
            s
        } else {
            okay = false;
            break;
        };

        extremity_sstate_hashes.insert(sstate_hash, prev_event);
    }
    if !okay {
        return Ok(None);
    }

    let mut fork_states = Vec::with_capacity(extremity_sstate_hashes.len());
    let mut auth_chain_sets = Vec::with_capacity(extremity_sstate_hashes.len());

    for (sstate_hash, prev_event) in extremity_sstate_hashes {
        let mut leaf_state: HashMap<_, _> = crate::room::state::get_full_state_ids(sstate_hash)?;

        if let Some(state_key) = &prev_event.state_key {
            let state_key_id = crate::room::state::ensure_field_id(&prev_event.event_ty.to_string().into(), state_key)?;
            leaf_state.insert(state_key_id, Arc::from(&*prev_event.event_id));
            // Now it's the state after the pdu
        }

        let mut state = StateMap::with_capacity(leaf_state.len());
        let mut starting_events = Vec::with_capacity(leaf_state.len());

        for (k, id) in leaf_state {
            if let Ok(DbRoomStateField {
                event_ty, state_key, ..
            }) = crate::room::state::get_field(k)
            {
                // FIXME: Undo .to_string().into() when StateMap
                //        is updated to use StateEventType
                state.insert((event_ty.to_string().into(), state_key), id.clone());
            } else {
                warn!("Failed to get_state_key_id.");
            }
            starting_events.push(id);
        }

        for starting_event in starting_events {
            auth_chain_sets.push(crate::room::auth_chain::get_auth_chain_ids(
                room_id,
                [&*starting_event].into_iter(),
            )?);
        }

        fork_states.push(state);
    }

    let lock = crate::STATERES_MUTEX.lock();

    let result = state::resolve(
        room_version_id,
        &fork_states,
        auth_chain_sets
            .iter()
            .map(|set| set.iter().map(|id| Arc::from(&**id)).collect::<HashSet<_>>())
            .collect::<Vec<_>>(),
        |id| {
            let res = crate::room::timeline::get_pdu(id);
            if let Err(e) = &res {
                error!("LOOK AT ME Failed to fetch event: {}", e);
            }
            res.ok().flatten()
        },
    );
    drop(lock);

    match result {
        Ok(new_state) => Ok(Some(
            new_state
                .into_iter()
                .map(|((event_type, state_key), event_id)| {
                    let state_key_id = crate::room::state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
                    Ok((state_key_id, event_id))
                })
                .collect::<AppResult<_>>()?,
        )),
        Err(e) => {
            warn!(
                "State resolution on prev events failed, either an event could not be found or deserialization: {}",
                e
            );
            Ok(None)
        }
    }
}
