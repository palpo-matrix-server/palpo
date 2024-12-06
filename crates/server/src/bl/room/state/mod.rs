mod diff;
pub use diff::*;
mod field;
pub use field::*;
mod frame;
pub use frame::*;
mod point;
pub use point::*;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use diesel::prelude::*;
use lru_cache::LruCache;
use palpo_core::JsonValue;
use serde::Deserialize;
use tracing::warn;

use crate::core::events::room::avatar::RoomAvatarEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::events::{AnyStrippedStateEvent, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::serde::{to_raw_json_value, RawJson};
use crate::core::state::StateMap;
use crate::core::{EventId, OwnedEventId, RoomId, RoomVersionId, UserId};
use crate::event::{PduBuilder, PduEvent};
use crate::schema::*;
use crate::{db, utils, AppError, AppResult, MatrixError};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_state_deltas, primary_key(frame_id))]
pub struct DbRoomStateDelta {
    pub frame_id: i64,
    pub room_id: OwnedRoomId,
    pub parent_id: Option<i64>,
    pub appended: Vec<u8>,
    pub disposed: Vec<u8>,
}
// #[derive(Insertable, Debug, Clone)]
// #[diesel(table_name = room_state_deltas)]
// pub struct NewDbRoomStateDelta {
//     pub room_id: OwnedRoomId,
//     pub frame_id: i64,
//     pub parent_id: Option<i64>,
//     pub appended: Vec<u8>,
//     pub disposed: Vec<u8>,
// }

pub const SERVER_VISIBILITY_CACHE: LazyLock<Mutex<LruCache<(OwnedServerName, i64), bool>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(100)));
pub const USER_VISIBILITY_CACHE: LazyLock<Mutex<LruCache<(OwnedUserId, i64), bool>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(100)));

/// Set the room to the given state_hash and update caches.
pub fn force_state(
    room_id: &RoomId,
    frame_id: i64,
    appended: Arc<HashSet<CompressedState>>,
    _disposed_data: Arc<HashSet<CompressedState>>,
) -> AppResult<()> {
    let event_ids = appended
        .iter()
        .filter_map(|new| new.split().ok().map(|(_, id)| id))
        .collect::<Vec<_>>();
    for event_id in event_ids {
        let pdu = match crate::room::timeline::get_pdu_json(&event_id)? {
            Some(pdu) => pdu,
            None => continue,
        };

        let pdu: PduEvent = match serde_json::from_str(
            &serde_json::to_string(&pdu).expect("CanonicalJsonObj can be serialized to JSON"),
        ) {
            Ok(pdu) => pdu,
            Err(_) => continue,
        };

        match pdu.event_ty {
            TimelineEventType::RoomMember => {
                #[derive(Deserialize)]
                struct ExtractMembership {
                    membership: MembershipState,
                }

                let membership = match serde_json::from_str::<ExtractMembership>(pdu.content.get()) {
                    Ok(e) => e.membership,
                    Err(_) => continue,
                };

                let state_key = match pdu.state_key {
                    Some(k) => k,
                    None => continue,
                };

                let user_id = match UserId::parse(state_key) {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                crate::room::update_membership(
                    &pdu.event_id,
                    pdu.event_sn,
                    room_id,
                    &user_id,
                    membership,
                    &pdu.sender,
                    None,
                )?;
            }
            TimelineEventType::SpaceChild => {
                crate::room::space::ROOM_ID_SPACE_CHUNK_CACHE
                    .lock()
                    .unwrap()
                    .remove(&pdu.room_id);
            }
            _ => continue,
        }
    }

    crate::room::update_room_servers(room_id)?;
    crate::room::update_room_currents(room_id)?;

    set_room_state(room_id, frame_id)?;

    Ok(())
}

#[tracing::instrument]
pub fn set_room_state(room_id: &RoomId, frame_id: i64) -> AppResult<()> {
    diesel::update(rooms::table.find(room_id))
        .set(rooms::state_frame_id.eq(frame_id))
        .execute(&mut db::connect()?)?;
    Ok(())
}

/// Generates a new StateHash and associates it with the incoming event.
///
/// This adds all current state events (not including the incoming event)
/// to `stateid_pduid` and adds the incoming event to `eventid_state_hash`.
#[tracing::instrument(skip(new_pdu))]
pub fn append_to_state(new_pdu: &PduEvent) -> AppResult<i64> {
    println!("xxxxxxxxxappend_to_state new_pdu： {:#?}", new_pdu);
    let prev_frame_id = get_room_frame_id(&new_pdu.room_id, None)?;

    let point_id = ensure_point(&new_pdu.room_id, &new_pdu.event_id, new_pdu.event_sn)?;
    if let Some(state_key) = &new_pdu.state_key {
        let states_parents = prev_frame_id.map_or_else(|| Ok(Vec::new()), |p| load_frame_info(p))?;

        let field_id = ensure_field(&new_pdu.event_ty.to_string().into(), state_key)?.id;

        let new_compressed_event = CompressedState::new(field_id, point_id);

        let replaces = states_parents
            .last()
            .map(|info| {
                info.full_state
                    .iter()
                    .find(|bytes| bytes.starts_with(&field_id.to_be_bytes()))
            })
            .unwrap_or_default();
        println!("=======xxxx state_key: {state_key} replaces: {replaces:#?}",);

        if Some(&new_compressed_event) == replaces {
            return prev_frame_id.ok_or_else(|| MatrixError::invalid_param("Room previous point must exists.").into());
        }

        // TODO: state_hash with deterministic inputs
        let mut appended = HashSet::new();
        appended.insert(new_compressed_event);

        let mut disposed = HashSet::new();
        if let Some(replaces) = replaces {
            disposed.insert(*replaces);
        }

        let hash_data = utils::hash_keys(&vec![new_compressed_event.as_bytes()]);
        let frame_id = ensure_frame(&new_pdu.room_id, hash_data)?;
        update_point_frame_id(new_compressed_event.point_id(), frame_id)?;
        calc_and_save_state_delta(
            &new_pdu.room_id,
            frame_id,
            Arc::new(appended),
            Arc::new(disposed),
            2,
            states_parents,
        )?;
        Ok(frame_id)
    } else {
        let frame_id = prev_frame_id.ok_or_else(|| MatrixError::invalid_param("Room previous point must exists."))?;
        update_point_frame_id(point_id, frame_id)?;
        Ok(frame_id)
    }
}

pub fn calculate_invite_state(invite_event: &PduEvent) -> AppResult<Vec<RawJson<AnyStrippedStateEvent>>> {
    let mut state = Vec::new();
    // Add recommended events
    if let Some(e) = get_state(&invite_event.room_id, &StateEventType::RoomCreate, "", None)? {
        state.push(e.to_stripped_state_event());
    }
    if let Some(e) = get_state(&invite_event.room_id, &StateEventType::RoomJoinRules, "", None)? {
        state.push(e.to_stripped_state_event());
    }
    if let Some(e) = get_state(&invite_event.room_id, &StateEventType::RoomCanonicalAlias, "", None)? {
        state.push(e.to_stripped_state_event());
    }
    if let Some(e) = get_state(&invite_event.room_id, &StateEventType::RoomAvatar, "", None)? {
        state.push(e.to_stripped_state_event());
    }
    if let Some(e) = get_state(&invite_event.room_id, &StateEventType::RoomName, "", None)? {
        state.push(e.to_stripped_state_event());
    }
    if let Some(e) = get_state(
        &invite_event.room_id,
        &StateEventType::RoomMember,
        invite_event.sender.as_str(),
        None,
    )? {
        state.push(e.to_stripped_state_event());
    }

    state.push(invite_event.to_stripped_state_event());
    Ok(state)
}

pub fn get_current_frame_id(room_id: &RoomId) -> AppResult<Option<i64>> {
    rooms::table
        .find(room_id)
        .select(rooms::state_frame_id)
        .first(&mut *db::connect()?)
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

/// Returns the room's version.
pub fn get_room_version(room_id: &RoomId) -> AppResult<RoomVersionId> {
    let create_event = get_state(room_id, &StateEventType::RoomCreate, "", None)?;

    let create_event_content: RoomCreateEventContent = create_event
        .as_ref()
        .map(|create_event| {
            serde_json::from_str(create_event.content.get()).map_err(|e| {
                warn!("Invalid create event: {}", e);
                AppError::internal("Invalid create event in database.")
            })
        })
        .transpose()?
        .ok_or_else(|| MatrixError::invalid_param("No create event found"))?;

    Ok(create_event_content.room_version)
}

pub fn get_forward_extremities(room_id: &RoomId) -> AppResult<HashSet<Arc<EventId>>> {
    let event_ids = event_forward_extremities::table
        .filter(event_forward_extremities::room_id.eq(room_id))
        .select(event_forward_extremities::event_id)
        .load::<OwnedEventId>(&mut *db::connect()?)?
        .into_iter()
        .map(|id| id.into())
        .collect();
    Ok(event_ids)
}

pub fn set_forward_extremities(room_id: &RoomId, event_ids: Vec<OwnedEventId>) -> AppResult<()> {
    for event_id in event_ids {
        diesel::insert_into(event_forward_extremities::table)
            .values((
                event_forward_extremities::room_id.eq(room_id),
                event_forward_extremities::event_id.eq(event_id),
            ))
            .on_conflict_do_nothing()
            .execute(&mut db::connect()?)?;
    }
    Ok(())
}

/// This fetches auth events from the current state.
#[tracing::instrument]
pub fn get_auth_events(
    room_id: &RoomId,
    kind: &TimelineEventType,
    sender: &UserId,
    state_key: Option<&str>,
    content: &serde_json::value::RawValue,
) -> AppResult<StateMap<PduEvent>> {
    let frame_id = if let Some(current_frame_id) = get_room_frame_id(room_id, None)? {
        current_frame_id
    } else {
        return Ok(HashMap::new());
    };

    let auth_events = crate::core::state::auth_types_for_event(kind, sender, state_key, content)?;

    let mut sauth_events = auth_events
        .into_iter()
        .filter_map(|(event_type, state_key)| {
            get_field_id(&event_type.to_string().into(), &state_key)
                .ok()
                .flatten()
                .map(|field_id| (field_id, (event_type, state_key)))
        })
        .collect::<HashMap<_, _>>();

    let full_state = load_frame_info(frame_id)?
        .pop()
        .expect("there is always one layer")
        .full_state;
    let mut state_map = StateMap::new();
    for state in full_state.iter() {
        let (state_key_id, event_id) = state.split()?;
        if let Some(key) = sauth_events.remove(&state_key_id) {
            if let Some(pdu) = crate::room::timeline::get_pdu(&event_id)? {
                state_map.insert(key, pdu);
            }
        }
    }
    println!("???????????state_map: {:?}", state_map);
    Ok(state_map)
}

/// Builds a StateMap by iterating over all keys that start
/// with state_hash, this gives the full state for the given state_hash.
pub fn get_full_state_ids(frame_id: i64) -> AppResult<HashMap<i64, Arc<EventId>>> {
    let full_state = load_frame_info(frame_id)?
        .pop()
        .expect("there is always one layer")
        .full_state;
    let mut map = HashMap::new();
    for compressed in full_state.iter() {
        let splited = compressed.split()?;
        map.insert(splited.0, splited.1);
    }
    Ok(map)
}

pub fn get_full_state(frame_id: i64) -> AppResult<HashMap<(StateEventType, String), PduEvent>> {
    let full_state = load_frame_info(frame_id)?
        .pop()
        .expect("there is always one layer")
        .full_state;

    let mut result = HashMap::new();
    for compressed in full_state.iter() {
        let (_, event_id) = compressed.split()?;
        if let Some(pdu) = crate::room::timeline::get_pdu(&event_id)? {
            result.insert(
                (
                    pdu.event_ty.to_string().into(),
                    pdu.state_key
                        .as_ref()
                        .ok_or_else(|| AppError::public("State event has no state key."))?
                        .clone(),
                ),
                pdu,
            );
        }
    }

    Ok(result)
}

/// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
pub fn get_state_event_id(
    frame_id: i64,
    event_type: &StateEventType,
    state_key: &str,
) -> AppResult<Option<Arc<EventId>>> {
    if let Some(state_key_id) = get_field_id(event_type, state_key)? {
        let full_state = load_frame_info(frame_id)?
            .pop()
            .expect("there is always one layer")
            .full_state;
        Ok(full_state
            .iter()
            .find(|bytes| bytes.starts_with(&state_key_id.to_be_bytes()))
            .and_then(|compressed| compressed.split().ok().map(|(_, id)| id)))
    } else {
        Ok(None)
    }
}

/// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
pub fn get_pdu(frame_id: i64, event_type: &StateEventType, state_key: &str) -> AppResult<Option<PduEvent>> {
    get_state_event_id(frame_id, event_type, state_key)?
        .map_or(Ok(None), |event_id| crate::room::timeline::get_pdu(&event_id))
}

/// Get membership for given user in state
fn user_membership(frame_id: i64, user_id: &UserId) -> AppResult<MembershipState> {
    get_pdu(frame_id, &StateEventType::RoomMember, user_id.as_str())?.map_or(Ok(MembershipState::Leave), |s| {
        serde_json::from_str(s.content.get())
            .map(|c: RoomMemberEventContent| c.membership)
            .map_err(|_| AppError::internal("Invalid room membership event in database."))
    })
}

/// The user was a joined member at this state (potentially in the past)
fn user_was_joined(frame_id: i64, user_id: &UserId) -> bool {
    user_membership(frame_id, user_id)
        .map(|s| s == MembershipState::Join)
        .unwrap_or_default() // Return sensible default, i.e. false
}

/// The user was an invited or joined room member at this state (potentially
/// in the past)
fn user_was_invited(frame_id: i64, user_id: &UserId) -> bool {
    user_membership(frame_id, user_id)
        .map(|s| s == MembershipState::Join || s == MembershipState::Invite)
        .unwrap_or_default() // Return sensible default, i.e. false
}

/// Whether a server is allowed to see an event through federation, based on
/// the room's history_visibility at that event's state.
#[tracing::instrument(skip(origin, room_id, event_id))]
pub fn server_can_see_event(origin: &ServerName, room_id: &RoomId, event_id: &EventId) -> AppResult<bool> {
    let frame_id = match get_pdu_frame_id(event_id)? {
        Some(frame_id) => frame_id,
        None => return Ok(true),
    };

    if let Some(visibility) = SERVER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .get_mut(&(origin.to_owned(), frame_id))
    {
        return Ok(*visibility);
    }

    let history_visibility =
        get_pdu(frame_id, &StateEventType::RoomHistoryVisibility, "")?.map_or(Ok(HistoryVisibility::Shared), |s| {
            serde_json::from_str(s.content.get())
                .map(|c: RoomHistoryVisibilityEventContent| c.history_visibility)
                .map_err(|_| AppError::internal("Invalid history visibility event in database."))
        })?;

    let mut current_server_members = crate::room::get_joined_users(room_id, None)?
        .into_iter()
        .filter(|member| member.server_name() == origin);

    let visibility = match history_visibility {
        HistoryVisibility::WorldReadable | HistoryVisibility::Shared => true,
        HistoryVisibility::Invited => {
            // Allow if any member on requesting server was AT LEAST invited, else deny
            current_server_members.any(|member| user_was_invited(frame_id, &member))
        }
        HistoryVisibility::Joined => {
            // Allow if any member on requested server was joined, else deny
            current_server_members.any(|member| user_was_joined(frame_id, &member))
        }
        _ => {
            error!("Unknown history visibility {history_visibility}");
            false
        }
    };

    SERVER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .insert((origin.to_owned(), frame_id), visibility);

    Ok(visibility)
}

/// Whether a user is allowed to see an event, based on
/// the room's history_visibility at that event's state.
#[tracing::instrument(skip(user_id, room_id, event_id))]
pub fn user_can_see_event(user_id: &UserId, room_id: &RoomId, event_id: &EventId) -> AppResult<bool> {
    let frame_id = match get_pdu_frame_id(event_id).unwrap() {
        Some(frame_id) => frame_id,
        None => return Ok(false),
    };

    if let Some(visibility) = USER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .get_mut(&(user_id.to_owned(), frame_id))
    {
        return Ok(*visibility);
    }

    let currently_member = crate::room::is_joined(&user_id, &room_id)?;

    let history_visibility =
        get_pdu(frame_id, &StateEventType::RoomHistoryVisibility, "")?.map_or(Ok(HistoryVisibility::Shared), |s| {
            serde_json::from_str(s.content.get())
                .map(|c: RoomHistoryVisibilityEventContent| c.history_visibility)
                .map_err(|_| AppError::internal("Invalid history visibility event in database."))
        })?;

    let visibility = match history_visibility {
        HistoryVisibility::WorldReadable => true,
        HistoryVisibility::Shared => currently_member,
        HistoryVisibility::Invited => {
            // Allow if any member on requesting server was AT LEAST invited, else deny
            user_was_invited(frame_id, &user_id)
        }
        HistoryVisibility::Joined => {
            // Allow if any member on requested server was joined, else deny
            user_was_joined(frame_id, &user_id)
        }
        _ => {
            error!("Unknown history visibility {history_visibility}");
            false
        }
    };

    USER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .insert((user_id.to_owned(), frame_id), visibility);

    Ok(visibility)
}

#[derive(Eq, PartialEq, Debug, Clone, Copy)]
pub enum UserCanSeeEvent {
    Always,
    Until(i64),
    Never,
}
impl UserCanSeeEvent {
    pub fn as_until_sn(&self) -> i64 {
        match self {
            UserCanSeeEvent::Always => i64::MAX,
            UserCanSeeEvent::Until(sn) => *sn,
            UserCanSeeEvent::Never => 0,
        }
    }
}
/// Whether a user is allowed to see an event, based on
/// the room's history_visibility at that event's state.
#[tracing::instrument(skip(user_id, room_id))]
pub fn user_can_see_state_events(user_id: &UserId, room_id: &RoomId) -> AppResult<UserCanSeeEvent> {
    let currently_member = crate::room::is_joined(&user_id, &room_id)?;

    let history_visibility = get_state(&room_id, &StateEventType::RoomHistoryVisibility, "", None)?.map_or(
        Ok(HistoryVisibility::Shared),
        |s| {
            serde_json::from_str(s.content.get())
                .map(|c: RoomHistoryVisibilityEventContent| c.history_visibility)
                .map_err(|_| AppError::internal("Invalid history visibility event in database."))
        },
    )?;
    if currently_member || history_visibility == HistoryVisibility::WorldReadable {
        return Ok(UserCanSeeEvent::Always);
    }

    let leave_sn = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::event_sn)
        .first::<i64>(&mut *db::connect()?)
        .optional()?;
    if let Some(leave_sn) = leave_sn {
        Ok(UserCanSeeEvent::Until(leave_sn))
    } else {
        Ok(UserCanSeeEvent::Never)
    }
}

/// Returns the new state_hash, and the state diff from the previous room state
pub fn save_state(room_id: &RoomId, new_compressed_events: Arc<HashSet<CompressedState>>) -> AppResult<DeltaInfo> {
    println!(
        "xxxxxxxxxsave_state new_compressed_events： {:#?}",
        new_compressed_events
    );
    let prev_frame_id = get_room_frame_id(room_id, None)?;

    let hash_data = utils::hash_keys(&new_compressed_events.iter().map(|bytes| &bytes[..]).collect::<Vec<_>>());

    let new_frame_id = ensure_frame(room_id, hash_data)?;

    if Some(new_frame_id) == prev_frame_id {
        return Ok(DeltaInfo {
            frame_id: new_frame_id,
            appended: Arc::new(HashSet::new()),
            disposed: Arc::new(HashSet::new()),
        });
    }
    for new_compressed_event in new_compressed_events.iter() {
        update_point_frame_id(new_compressed_event.point_id(), new_frame_id)?;
    }

    let states_parents = prev_frame_id.map_or_else(|| Ok(Vec::new()), |p| load_frame_info(p))?;

    let (appended, disposed) = if let Some(parent_state_info) = states_parents.last() {
        let appended: HashSet<_> = new_compressed_events
            .difference(&parent_state_info.full_state)
            .copied()
            .collect();

        let disposed: HashSet<_> = parent_state_info
            .full_state
            .difference(&new_compressed_events)
            .copied()
            .collect();
        println!("xxxxxxxxxx????xx222 appended: {:#?} disposed: {:#?}", appended, disposed);

        (Arc::new(appended), Arc::new(disposed))
    } else {
        (new_compressed_events, Arc::new(HashSet::new()))
    };

    if Some(new_frame_id) != prev_frame_id {
        calc_and_save_state_delta(
            room_id,
            new_frame_id,
            appended.clone(),
            disposed.clone(),
            1_000_000, // high number because no state will be based on this one
            states_parents,
        )?;
    };
    set_room_state(room_id, new_frame_id)?;

    Ok(DeltaInfo {
        frame_id: new_frame_id,
        appended,
        disposed,
    })
}

/// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
pub fn get_state(
    room_id: &RoomId,
    event_type: &StateEventType,
    state_key: &str,
    until_sn: Option<i64>,
) -> AppResult<Option<PduEvent>> {
    let Some(frame_id) = get_room_frame_id(room_id, until_sn)? else {
        return Ok(None);
    };
    let event_id = get_state_event_id(frame_id, event_type, state_key)?;
    if let Some(event_id) = event_id {
        crate::room::timeline::get_pdu(&event_id)
    } else {
        Ok(None)
    }
}

pub fn get_name(room_id: &RoomId, until_sn: Option<i64>) -> AppResult<Option<String>> {
    get_state(&room_id, &StateEventType::RoomName, "", None)?.map_or(Ok(None), |s| {
        serde_json::from_str(s.content.get())
            .map(|c: RoomNameEventContent| Some(c.name))
            .map_err(|_| AppError::internal("Invalid room name event in database."))
    })
}

pub fn get_avatar(room_id: &RoomId) -> AppResult<Option<RoomAvatarEventContent>> {
    get_state(&room_id, &StateEventType::RoomAvatar, "", None)?.map_or(Ok(None), |s| {
        serde_json::from_str(s.content.get()).map_err(|_| AppError::internal("Invalid room avatar event in database."))
    })
}

pub fn get_member(room_id: &RoomId, user_id: &UserId) -> AppResult<Option<RoomMemberEventContent>> {
    get_state(&room_id, &StateEventType::RoomMember, user_id.as_str(), None)?.map_or(Ok(None), |s| {
        serde_json::from_str(s.content.get()).map_err(|_| AppError::internal("Invalid room member event in database."))
    })
}

#[tracing::instrument]
pub fn get_invite_state(user_id: &UserId, room_id: &RoomId) -> AppResult<Option<Vec<RawJson<AnyStrippedStateEvent>>>> {
    if let Some(state) = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::state_data)
        .first::<Option<JsonValue>>(&mut *db::connect()?)
        .optional()?
        .flatten()
    {
        Ok(Some(serde_json::from_value(state)?))
    } else {
        Ok(None)
    }
}

pub fn user_can_invite(room_id: &RoomId, sender: &UserId, target_user: &UserId) -> AppResult<bool> {
    let content = to_raw_json_value(&RoomMemberEventContent::new(MembershipState::Invite))?;

    let new_event = PduBuilder {
        event_ty: TimelineEventType::RoomMember,
        content,
        unsigned: None,
        state_key: Some(target_user.into()),
        redacts: None,
    };

    Ok(crate::room::timeline::create_hash_and_sign_event(new_event, sender, room_id).is_ok())
}

// #[tracing::instrument]
// pub fn left_state(
//     user_id: &UserId,
//     room_id: &RoomId,
//
// ) -> AppResult<Option<Vec<AnyStrippedStateEvent>>> {
// let mut key = user_id.as_bytes().to_vec();
// key.push(0xff);
// key.extend_from_slice(room_id.as_bytes());

// self.userroomid_leftstate
//     .get(&key)?
//     .map(|state| {
//         let state = serde_json::from_slice(&state)
//             .map_err(|_| AppError::public("Invalid state in userroomid_leftstate."))?;

//         Ok(state)
//     })
//     .transpose()
// }
