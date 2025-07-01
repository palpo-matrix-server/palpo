use std::collections::{HashMap, HashSet};

use state::DbRoomStateField;

use crate::core::identifiers::*;
use crate::core::state::{StateMap, resolve};
use crate::event::PduEvent;
use crate::room::{state, timeline};
use crate::{AppResult, room};

pub(super) async fn state_at_incoming_degree_one(
    incoming_pdu: &PduEvent,
) -> AppResult<Option<HashMap<i64, OwnedEventId>>> {
    let prev_event = &*incoming_pdu.prev_events[0];
    let Ok(prev_frame_id) = state::get_pdu_frame_id(prev_event) else {
        return Ok(None);
    };

    let Ok(mut state) = state::get_full_state_ids(prev_frame_id) else {
        return Ok(None);
    };

    debug!("Using cached state");
    let prev_pdu = timeline::get_pdu(prev_event)?;

    if let Some(state_key) = &prev_pdu.state_key {
        let state_key_id = state::ensure_field_id(&prev_pdu.event_ty.to_string().into(), state_key)?;

        state.insert(state_key_id, prev_event.to_owned());
        // Now it's the state after the pdu
    }

    Ok(Some(state))
}

pub(super) async fn state_at_incoming_resolved(
    incoming_pdu: &PduEvent,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
) -> AppResult<Option<HashMap<i64, OwnedEventId>>> {
    debug!("Calculating state at event using state res");
    let mut extremity_state_hashes = HashMap::new();

    let mut okay = true;
    for prev_event_id in &incoming_pdu.prev_events {
        let prev_event = if let Ok(pdu) = timeline::get_pdu(prev_event_id) {
            pdu
        } else {
            okay = false;
            break;
        };

        let sstate_hash = if let Ok(s) = state::get_pdu_frame_id(prev_event_id) {
            s
        } else {
            okay = false;
            break;
        };

        extremity_state_hashes.insert(sstate_hash, prev_event);
    }
    if !okay {
        return Ok(None);
    }

    let mut fork_states = Vec::with_capacity(extremity_state_hashes.len());
    let mut auth_chain_sets = Vec::with_capacity(extremity_state_hashes.len());

    for (sstate_hash, prev_event) in extremity_state_hashes {
        let mut leaf_state: HashMap<_, _> = state::get_full_state_ids(sstate_hash)?;

        if let Some(state_key) = &prev_event.state_key {
            let state_key_id = state::ensure_field_id(&prev_event.event_ty.to_string().into(), state_key)?;
            leaf_state.insert(state_key_id, prev_event.event_id.clone());
            // Now it's the state after the pdu
        }

        let mut state = StateMap::with_capacity(leaf_state.len());
        let mut starting_events = Vec::with_capacity(leaf_state.len());

        for (k, id) in leaf_state {
            if let Ok(DbRoomStateField {
                event_ty, state_key, ..
            }) = state::get_field(k)
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

    let state_lock = room::lock_state(room_id).await;
    let result = resolve(
        room_version_id,
        &fork_states,
        auth_chain_sets
            .iter()
            .map(|set| set.iter().map(|id| id.to_owned()).collect::<HashSet<_>>())
            .collect::<Vec<_>>(),
        |id| {
            let res = timeline::get_pdu(id);
            if let Err(e) = &res {
                error!("LOOK AT ME Failed to fetch event: {}", e);
            }
            res.ok()
        },
    );
    drop(state_lock);

    match result {
        Ok(new_state) => Ok(Some(
            new_state
                .into_iter()
                .map(|((event_type, state_key), event_id)| {
                    let state_key_id = state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
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
