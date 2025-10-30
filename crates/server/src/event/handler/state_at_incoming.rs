use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use state::DbRoomStateField;

use crate::core::identifiers::*;
use crate::core::room_version_rules::{RoomVersionRules, StateResolutionV2Rules};
use crate::core::state::StateError;
use crate::core::state::{StateMap, resolve};
use crate::event::PduEvent;
use crate::room::{state, timeline};
use crate::{AppResult, room};

// pub(super) async fn state_at_incoming_degree_one(
//     incoming_pdu: &PduEvent,
// ) -> AppResult<IndexMap<i64, OwnedEventId>> {
//     let room_id = &incoming_pdu.room_id;
//     let prev_event = &*incoming_pdu.prev_events[0];
//     let Ok(prev_frame_id) =
//         state::get_pdu_frame_id(prev_event).or_else(|_| room::get_frame_id(room_id, None))
//     else {
//         return Ok(IndexMap::new());
//     };

//     let Ok(mut state) = state::get_full_state_ids(prev_frame_id) else {
//         return Ok(IndexMap::new());
//     };

//     debug!("using cached state");
//     let prev_pdu = timeline::get_pdu(prev_event)?;

//     if let Some(state_key) = &prev_pdu.state_key {
//         let state_key_id =
//             state::ensure_field_id(&prev_pdu.event_ty.to_string().into(), state_key)?;

//         state.insert(state_key_id, prev_event.to_owned());
//         // Now it's the state after the pdu
//     }

//     Ok(state)
// }

pub(super) async fn state_at_incoming_resolved(
    incoming_pdu: &PduEvent,
    room_id: &RoomId,
    version_rules: &RoomVersionRules,
) -> AppResult<IndexMap<i64, OwnedEventId>> {
    debug!("calculating state at event using state resolve");
    let mut extremity_state_hashes = HashMap::new();

    let Ok(curr_frame_id) = room::get_frame_id(room_id, None) else {
        println!("=======state_at_incoming_resolved  0");
        return Ok(IndexMap::new());
    };
    for prev_event_id in &incoming_pdu.prev_events {
        let Ok(prev_event) = timeline::get_pdu(prev_event_id) else {
            continue;
        };

        if prev_event.is_rejected {
            extremity_state_hashes.insert(curr_frame_id, prev_event);
            continue;
        }

        let frame_id = state::get_pdu_frame_id(prev_event_id).unwrap_or(curr_frame_id);
        extremity_state_hashes.insert(frame_id, prev_event);
    }

    let mut fork_states = Vec::with_capacity(extremity_state_hashes.len());
    let mut auth_chain_sets = Vec::with_capacity(extremity_state_hashes.len());

    for (sstate_hash, prev_event) in extremity_state_hashes {
        let mut leaf_state = state::get_full_state_ids(sstate_hash)?;

        if let Some(state_key) = &prev_event.state_key {
            let state_key_id =
                state::ensure_field_id(&prev_event.event_ty.to_string().into(), state_key)?;
            leaf_state.insert(state_key_id, prev_event.event_id.clone());
            // Now it's the state after the pdu
        }

        let mut state = StateMap::with_capacity(leaf_state.len());
        let mut starting_events = Vec::with_capacity(leaf_state.len());

        for (k, id) in leaf_state {
            if let Ok(DbRoomStateField {
                event_ty,
                state_key,
                ..
            }) = state::get_field(k)
            {
                // FIXME: Undo .to_string().into() when StateMap
                //        is updated to use StateEventType
                state.insert((event_ty.to_string().into(), state_key), id.clone());
            } else {
                warn!("failed to get_state_key_id.");
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
        &version_rules.authorization,
        version_rules
            .state_resolution
            .v2_rules()
            .unwrap_or(StateResolutionV2Rules::V2_0),
        &fork_states,
        auth_chain_sets
            .iter()
            .map(|set| set.iter().map(|id| id.to_owned()).collect::<HashSet<_>>())
            .collect::<Vec<_>>(),
        &async |event_id| {
            timeline::get_pdu(&event_id)
                .map(|s| s.pdu)
                .map_err(|_| StateError::other("missing PDU 5"))
        },
        |map| {
            let mut subgraph = HashSet::new();
            for event_ids in map.values() {
                for event_id in event_ids {
                    if let Ok(pdu) = timeline::get_pdu(event_id) {
                        subgraph.extend(pdu.auth_events.iter().cloned());
                        subgraph.extend(pdu.prev_events.iter().cloned());
                    }
                }
            }
            Some(subgraph)
        },
    )
    .await;
    drop(state_lock);
    println!("=======state_at_incoming_resolved  result: {result:?}");

    match result {
        Ok(new_state) => Ok(new_state
            .into_iter()
            .map(|((event_type, state_key), event_id)| {
                let state_key_id =
                    state::ensure_field_id(&event_type.to_string().into(), &state_key)?;
                Ok((state_key_id, event_id))
            })
            .collect::<AppResult<_>>()?),
        Err(e) => {
            warn!(
                "state resolution on prev events failed, either an event could not be found or deserialization: {}",
                e
            );
            Ok(IndexMap::new())
        }
    }
}
