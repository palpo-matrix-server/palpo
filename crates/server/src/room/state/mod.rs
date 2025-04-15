mod diff;
pub use diff::*;
mod field;
pub use field::*;
mod frame;
pub use frame::*;
mod graph;
pub use graph::*;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use diesel::prelude::*;
use lru_cache::LruCache;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::core::Seqnum;
use crate::core::events::room::avatar::RoomAvatarEventContent;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::{GuestAccess, RoomGuestAccessEventContent};
use crate::core::events::room::history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent};
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent, RoomMembership};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::events::{AnyStrippedStateEvent, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::room::RoomType;
use crate::core::serde::{JsonValue, RawJson, to_raw_json_value};
use crate::core::state::StateMap;
use crate::core::{EventId, OwnedEventId, RoomId, RoomVersionId, UserId};
use crate::data::connect;
use crate::data::schema::*;
use crate::event::{PduBuilder, PduEvent};
use crate::event::{update_frame_id, update_frame_id_by_sn};
use crate::{AppError, AppResult, MatrixError, data, utils};

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
    appended: Arc<CompressedState>,
    _disposed_data: Arc<CompressedState>,
) -> AppResult<()> {
    let event_ids = appended
        .iter()
        .filter_map(|new| new.split().ok().map(|(_, id)| id))
        .collect::<Vec<_>>();
    for event_id in &event_ids {
        let pdu = match crate::room::timeline::get_pdu(&event_id) {
            Ok(pdu) => pdu,
            _ => continue,
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

                crate::membership::update_membership(
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
        .execute(&mut connect()?)?;
    Ok(())
}

/// Generates a new StateHash and associates it with the incoming event.
///
/// This adds all current state events (not including the incoming event)
/// to `stateid_pduid` and adds the incoming event to `eventid_statehash`.
#[tracing::instrument(skip(state_ids_compressed), level = "debug")]
pub fn set_event_state(
    event_id: &EventId,
    event_sn: i64,
    room_id: &RoomId,
    state_ids_compressed: Arc<CompressedState>,
) -> AppResult<i64> {
    let prev_frame_id = get_room_frame_id(room_id, None)?;
    let hash_data = utils::hash_keys(state_ids_compressed.iter().map(|s| &s[..]));
    if let Ok(frame_id) = get_frame_id(room_id, &hash_data) {
        update_frame_id(event_id, frame_id)?;
        Ok(frame_id)
    } else {
        let frame_id = ensure_frame(room_id, hash_data)?;
        let states_parents = load_frame_info(prev_frame_id)?;

        let (appended, disposed) = if let Some(parent_state_info) = states_parents.last() {
            let appended: CompressedState = state_ids_compressed
                .difference(&parent_state_info.full_state)
                .copied()
                .collect();

            let disposed: CompressedState = parent_state_info
                .full_state
                .difference(&state_ids_compressed)
                .copied()
                .collect();

            (Arc::new(appended), Arc::new(disposed))
        } else {
            (state_ids_compressed, Arc::new(CompressedState::new()))
        };

        update_frame_id(event_id, frame_id)?;
        calc_and_save_state_delta(room_id, frame_id, appended, disposed, 1_000_000, states_parents)?;
        Ok(frame_id)
    }
}

/// Generates a new StateHash and associates it with the incoming event.
///
/// This adds all current state events (not including the incoming event)
/// to `stateid_pduid` and adds the incoming event to `eventid_state_hash`.
#[tracing::instrument(skip(new_pdu))]
pub fn append_to_state(new_pdu: &PduEvent) -> AppResult<i64> {
    let prev_frame_id = get_room_frame_id(&new_pdu.room_id, None).ok();

    if let Some(state_key) = &new_pdu.state_key {
        let states_parents = prev_frame_id.map_or_else(|| Ok(Vec::new()), |p| load_frame_info(p))?;

        let field_id = ensure_field(&new_pdu.event_ty.to_string().into(), state_key)?.id;

        let new_compressed_event = CompressedEvent::new(field_id, new_pdu.event_sn);

        let replaces = states_parents
            .last()
            .map(|info| {
                info.full_state
                    .iter()
                    .find(|bytes| bytes.starts_with(&field_id.to_be_bytes()))
            })
            .unwrap_or_default();

        if Some(&new_compressed_event) == replaces {
            return prev_frame_id.ok_or_else(|| MatrixError::invalid_param("Room previous point must exists.").into());
        }

        // TODO: state_hash with deterministic inputs
        let mut appended = CompressedState::new();
        appended.insert(new_compressed_event);

        let mut disposed = CompressedState::new();
        if let Some(replaces) = replaces {
            disposed.insert(*replaces);
        }

        let hash_data = utils::hash_keys([new_compressed_event.as_bytes()].into_iter());
        let frame_id = ensure_frame(&new_pdu.room_id, hash_data)?;
        update_frame_id(&new_pdu.event_id, frame_id)?;
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
        update_frame_id(&new_pdu.event_id, frame_id)?;
        Ok(frame_id)
    }
}

pub fn summary_stripped(event: &PduEvent) -> AppResult<Vec<RawJson<AnyStrippedStateEvent>>> {
    let cells: [(&StateEventType, &str); 8] = [
        (&StateEventType::RoomCreate, ""),
        (&StateEventType::RoomJoinRules, ""),
        (&StateEventType::RoomCanonicalAlias, ""),
        (&StateEventType::RoomName, ""),
        (&StateEventType::RoomAvatar, ""),
        (&StateEventType::RoomMember, event.sender.as_str()), // Add recommended events
        (&StateEventType::RoomEncryption, ""),
        (&StateEventType::RoomTopic, ""),
    ];

    let mut state = Vec::new();
    // Add recommended events
    for (event_type, state_key) in cells {
        if let Ok(e) = get_room_state(&event.room_id, &StateEventType::RoomCreate, "") {
            state.push(e.to_stripped_state_event());
        }
    }

    state.push(event.to_stripped_state_event());
    Ok(state)
}

pub fn get_current_frame_id(room_id: &RoomId) -> AppResult<Option<i64>> {
    rooms::table
        .find(room_id)
        .select(rooms::state_frame_id)
        .first(&mut connect()?)
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

/// Returns the room's version.
pub fn get_room_version(room_id: &RoomId) -> AppResult<RoomVersionId> {
    // let room_version = rooms::table
    //     .find(room_id)
    //     .select(rooms::version)
    //     .first::<String>(&mut connect()?)?;
    // Ok(RoomVersionId::try_from(&*room_version)?)

    if let Some(room_version) = rooms::table
        .find(room_id)
        .select(rooms::version)
        .first::<String>(&mut connect()?)
        .optional()?
    {
        return Ok(RoomVersionId::try_from(&*room_version)?);
    }
    let create_event_content =
        get_room_state_content::<RoomCreateEventContent>(room_id, &StateEventType::RoomCreate, "")?;
    Ok(create_event_content.room_version)
}

pub fn get_forward_extremities(room_id: &RoomId) -> AppResult<Vec<Arc<EventId>>> {
    let event_ids = event_forward_extremities::table
        .filter(event_forward_extremities::room_id.eq(room_id))
        .select(event_forward_extremities::event_id)
        .distinct()
        .load::<OwnedEventId>(&mut connect()?)?
        .into_iter()
        .map(|id| id.into())
        .collect();
    Ok(event_ids)
}

pub fn set_forward_extremities<'a, I>(room_id: &'a RoomId, event_ids: I) -> AppResult<()>
where
    I: Iterator<Item = &'a EventId> + Send + 'a,
{
    let event_ids = event_ids.collect::<Vec<_>>();
    diesel::delete(
        event_forward_extremities::table
            .filter(event_forward_extremities::room_id.eq(room_id))
            .filter(event_forward_extremities::event_id.ne_all(&event_ids)),
    )
    .execute(&mut connect()?)?;
    for event_id in event_ids {
        diesel::insert_into(event_forward_extremities::table)
            .values((
                event_forward_extremities::room_id.eq(room_id),
                event_forward_extremities::event_id.eq(event_id),
            ))
            .on_conflict_do_nothing()
            .execute(&mut connect()?)?;
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
    let frame_id = if let Ok(current_frame_id) = get_room_frame_id(room_id, None) {
        current_frame_id
    } else {
        return Ok(HashMap::new());
    };

    let auth_types = crate::core::state::auth_types_for_event(kind, sender, state_key, content)?;
    let mut sauth_events = auth_types
        .into_iter()
        .filter_map(|(event_type, state_key)| {
            get_field_id(&event_type.to_string().into(), &state_key)
                .ok()
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
            if let Ok(pdu) = crate::room::timeline::get_pdu(&event_id) {
                state_map.insert(key, pdu);
            } else {
                tracing::warn!("pdu is not found: {}", event_id);
            }
        }
    }
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
        map.insert(splited.0, Arc::from(&*splited.1));
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
        if let Ok(pdu) = crate::room::timeline::get_pdu(&event_id) {
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
pub fn get_state_event_id(frame_id: i64, event_type: &StateEventType, state_key: &str) -> AppResult<OwnedEventId> {
    let state_key_id = get_field_id(event_type, state_key)?;
    let full_state = load_frame_info(frame_id)?
        .pop()
        .expect("there is always one layer")
        .full_state;
    full_state
        .iter()
        .find(|bytes| bytes.starts_with(&state_key_id.to_be_bytes()))
        .and_then(|compressed| compressed.split().ok().map(|(_, id)| id))
        .ok_or(MatrixError::not_found("state event not found").into())
}

/// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
pub fn get_state(frame_id: i64, event_type: &StateEventType, state_key: &str) -> AppResult<PduEvent> {
    let event_id = get_state_event_id(frame_id, event_type, state_key)?;
    crate::room::timeline::get_pdu(&event_id)
}
pub fn get_state_content<T>(frame_id: i64, event_type: &StateEventType, state_key: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    let state = get_state(frame_id, event_type, state_key)?;
    Ok(serde_json::from_str(state.content.get())?)
}

pub fn get_room_state(room_id: &RoomId, event_type: &StateEventType, state_key: &str) -> AppResult<PduEvent> {
    let frame_id = get_room_frame_id(room_id, None)?;
    get_state(frame_id, event_type, state_key)
}
pub fn get_room_state_content<T>(room_id: &RoomId, event_type: &StateEventType, state_key: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    let frame_id = get_room_frame_id(room_id, None)?;
    get_state_content(frame_id, event_type, state_key)
}
// /// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
// pub fn get_state(
//     room_id: &RoomId,
//     event_type: &StateEventType,
//     state_key: &str,
//     until_sn: Option<i64>,
// ) -> AppResult<Option<PduEvent>> {
//     let Some(frame_id) = get_room_frame_id(room_id, until_sn)? else {
//         return Ok(None);
//     };
//     let event_id = get_state_event_id(frame_id, event_type, state_key)?;
//     if let Some(event_id) = event_id {
//         crate::room::timeline::get_pdu(&event_id)
//     } else {
//         Ok(None)
//     }
// }

/// Get membership for given user in state
fn user_membership(frame_id: i64, user_id: &UserId) -> AppResult<MembershipState> {
    get_state_content::<RoomMemberEventContent>(frame_id, &StateEventType::RoomMember, user_id.as_str())
        .map(|c: RoomMemberEventContent| c.membership)
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
    // }

    // /// Checks if a given user can redact a given event
    // ///
    // /// If federation is true, it allows redaction events from any user of the
    // /// same server as the original event sender
    // pub async fn user_can_redact(
    //     redacts: &EventId,
    //     sender: &UserId,
    //     room_id: &RoomId,
    //     federation: bool,
    // ) -> AppResult<bool> {
    //     let redacting_event = crate::room::timeline.get_pdu(redacts)?;

    //     if redacting_event
    //         .as_ref()
    //         .is_ok_and(|pdu| pdu.kind == TimelineEventType::RoomCreate)
    //     {
    //         return Err(MatrixError::forbidden("Redacting m.room.create is not safe, forbidding.").into());
    //     }

    //     if redacting_event
    //         .as_ref()
    //         .is_ok_and(|pdu| pdu.kind == TimelineEventType::RoomServerAcl)
    //     {
    //         return Err(MatrixError::forbidden(
    //             "Redacting m.room.server_acl will result in the room being inaccessible for \
    // 			 everyone (empty allow key), forbidding.",
    //         )
    //         .into());
    //     }

    //     if let Ok(pl_event_content) = self
    //         .room_state_get_content::<RoomPowerLevelsEventContent>(room_id, &StateEventType::RoomPowerLevels, "")
    //         .await
    //     {
    //         let pl_event: RoomPowerLevels = pl_event_content.into();
    //         Ok(pl_event.user_can_redact_event_of_other(sender)
    //             || pl_event.user_can_redact_own_event(sender)
    //                 && if let Ok(redacting_event) = redacting_event {
    //                     if federation {
    //                         redacting_event.sender.server_name() == sender.server_name()
    //                     } else {
    //                         redacting_event.sender == sender
    //                     }
    //                 } else {
    //                     false
    //                 })
    //     } else {
    //         // Falling back on m.room.create to judge power level
    //         if let Ok(room_create) = self.room_state_get(room_id, &StateEventType::RoomCreate, "").await {
    //             Ok(room_create.sender == sender
    //                 || redacting_event
    //                     .as_ref()
    //                     .is_ok_and(|redacting_event| redacting_event.sender == sender))
    //         } else {
    //             Err(Error::bad_database(
    //                 "No m.room.power_levels or m.room.create events in database for room",
    //             ))
    //         }
    //     }
}

/// Whether a server is allowed to see an event through federation, based on
/// the room's history_visibility at that event's state.
#[tracing::instrument(skip(origin, room_id, event_id))]
pub fn server_can_see_event(origin: &ServerName, room_id: &RoomId, event_id: &EventId) -> AppResult<bool> {
    let frame_id = match get_pdu_frame_id(event_id) {
        Ok(frame_id) => frame_id,
        Err(_) => return Ok(true),
    };

    if let Some(visibility) = SERVER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .get_mut(&(origin.to_owned(), frame_id))
    {
        return Ok(*visibility);
    }

    let history_visibility = get_state_content(frame_id, &StateEventType::RoomHistoryVisibility, "")
        .map(|c: RoomHistoryVisibilityEventContent| c.history_visibility)?;

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

#[tracing::instrument(skip(origin, user_id))]
pub fn server_can_see_user(origin: &ServerName, user_id: &UserId) -> AppResult<bool> {
    Ok(super::server_rooms(origin)?
        .iter()
        .any(|room_id| super::user::is_joined(user_id, room_id).unwrap_or(false)))
}
#[tracing::instrument(skip(sender_id, user_id))]
pub fn user_can_see_user(sender_id: &UserId, user_id: &UserId) -> AppResult<bool> {
    super::user::get_shared_rooms(vec![sender_id.to_owned(), user_id.to_owned()]).map(|rooms| !rooms.is_empty())
}

/// Whether a user is allowed to see an event, based on
/// the room's history_visibility at that event's state.
#[tracing::instrument(skip(user_id, room_id, event_id))]
pub fn user_can_see_event(user_id: &UserId, room_id: &RoomId, event_id: &EventId) -> AppResult<bool> {
    let Ok(frame_id) = get_pdu_frame_id(event_id) else {
        return Ok(true);
    };

    if let Some(visibility) = USER_VISIBILITY_CACHE
        .lock()
        .unwrap()
        .get_mut(&(user_id.to_owned(), frame_id))
    {
        return Ok(*visibility);
    }

    let history_visibility =
        get_state_content::<RoomHistoryVisibilityEventContent>(frame_id, &StateEventType::RoomHistoryVisibility, "")
            .map_or(HistoryVisibility::Shared, |c: RoomHistoryVisibilityEventContent| {
                c.history_visibility
            });

    let visibility = match history_visibility {
        HistoryVisibility::WorldReadable => true,
        HistoryVisibility::Shared => crate::room::is_joined(&user_id, &room_id)?,
        HistoryVisibility::Invited => {
            // Allow if any member on requesting server was AT LEAST invited, else deny
            user_was_invited(frame_id, &user_id)
        }
        HistoryVisibility::Joined => {
            // Allow if any member on requested server was joined, else deny
            user_was_joined(frame_id, &user_id) || user_was_joined(frame_id - 1, &user_id)
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
    if crate::room::is_joined(&user_id, &room_id)? {
        return Ok(UserCanSeeEvent::Always);
    }

    let history_visibility = get_history_visibility(&room_id)?;
    if history_visibility == HistoryVisibility::WorldReadable {
        return Ok(UserCanSeeEvent::Always);
    }

    let leave_sn = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::membership.eq("leave"))
        .select(room_users::event_sn)
        .first::<i64>(&mut connect()?)
        .optional()?;
    if let Some(leave_sn) = leave_sn {
        Ok(UserCanSeeEvent::Until(leave_sn))
    } else {
        Ok(UserCanSeeEvent::Never)
    }
}

/// Returns the new state_hash, and the state diff from the previous room state
pub fn save_state(room_id: &RoomId, new_compressed_events: Arc<CompressedState>) -> AppResult<DeltaInfo> {
    let prev_frame_id = get_room_frame_id(room_id, None).ok();

    let hash_data = utils::hash_keys(new_compressed_events.iter().map(|bytes| &bytes[..]));

    let (new_frame_id, frame_existed) = if let Ok(frame_id) = get_frame_id(room_id, &hash_data) {
        (frame_id, true)
    } else {
        let frame_id = ensure_frame(room_id, hash_data)?;
        (frame_id, false)
    };

    if Some(new_frame_id) == prev_frame_id {
        return Ok(DeltaInfo {
            frame_id: new_frame_id,
            appended: Arc::new(CompressedState::new()),
            disposed: Arc::new(CompressedState::new()),
        });
    }
    for new_compressed_event in new_compressed_events.iter() {
        update_frame_id_by_sn(new_compressed_event.event_sn(), new_frame_id)?;
    }

    let states_parents = prev_frame_id.map_or_else(|| Ok(Vec::new()), |p| load_frame_info(p))?;

    let (appended, disposed) = if let Some(parent_state_info) = states_parents.last() {
        let appended: CompressedState = new_compressed_events
            .difference(&parent_state_info.full_state)
            .copied()
            .collect();

        let disposed: CompressedState = parent_state_info
            .full_state
            .difference(&new_compressed_events)
            .copied()
            .collect();

        (Arc::new(appended), Arc::new(disposed))
    } else {
        (new_compressed_events, Arc::new(CompressedState::new()))
    };

    if !frame_existed {
        calc_and_save_state_delta(
            room_id,
            new_frame_id,
            appended.clone(),
            disposed.clone(),
            2, // every state change is 2 event changes on average
            states_parents,
        )?;
    };

    Ok(DeltaInfo {
        frame_id: new_frame_id,
        appended,
        disposed,
    })
}

// /// Returns a single PDU from `room_id` with key (`event_type`, `state_key`).
// pub fn get_state(
//     room_id: &RoomId,
//     event_type: &StateEventType,
//     state_key: &str,
//     until_sn: Option<i64>,
// ) -> AppResult<Option<PduEvent>> {
//     let Some(frame_id) = get_room_frame_id(room_id, until_sn)? else {
//         return Ok(None);
//     };
//     let event_id = get_state_event_id(frame_id, event_type, state_key)?;
//     if let Some(event_id) = event_id {
//         crate::room::timeline::get_pdu(&event_id)
//     } else {
//         Ok(None)
//     }
// }

pub fn get_name(room_id: &RoomId) -> AppResult<String> {
    get_room_state_content::<RoomNameEventContent>(&room_id, &StateEventType::RoomName, "").map(|c| c.name)
}

pub fn get_avatar_url(room_id: &RoomId) -> AppResult<Option<OwnedMxcUri>> {
    get_room_state_content::<RoomAvatarEventContent>(room_id, &StateEventType::RoomAvatar, "").map(|c| c.url)
}

pub fn get_member(room_id: &RoomId, user_id: &UserId) -> AppResult<RoomMemberEventContent> {
    get_room_state_content::<RoomMemberEventContent>(&room_id, &StateEventType::RoomMember, user_id.as_str())
}
pub fn get_room_topic(room_id: &RoomId) -> AppResult<String> {
    get_room_state_content::<RoomNameEventContent>(&room_id, &StateEventType::RoomTopic, "").map(|c| c.name)
}
pub fn get_canonical_alias(room_id: &RoomId) -> AppResult<Option<OwnedRoomAliasId>> {
    get_room_state_content::<RoomCanonicalAliasEventContent>(&room_id, &StateEventType::RoomCanonicalAlias, "")
        .map(|c| c.alias)
}
// pub fn get_join_rule(room_id: &RoomId) -> AppResult<(SpaceRoomJoinRule, Vec<OwnedRoomId>)> {
//     get_room_state_content::<RoomJoinRulesEventContent>(&room_id, &StateEventType::RoomJoinRules, "")
//         .map(|c| (c.join_rule.clone().into(), allowed_room_ids(c.join_rule)))
// }
pub fn get_join_rule(room_id: &RoomId) -> AppResult<JoinRule> {
    get_room_state_content::<RoomJoinRulesEventContent>(&room_id, &StateEventType::RoomJoinRules, "")
        .map(|c| c.join_rule)
}

pub fn get_room_type(room_id: &RoomId) -> AppResult<Option<RoomType>> {
    get_room_state_content::<RoomCreateEventContent>(room_id, &StateEventType::RoomCreate, "").map(|c| c.room_type)
}

pub fn get_history_visibility(room_id: &RoomId) -> AppResult<HistoryVisibility> {
    get_room_state_content::<RoomHistoryVisibilityEventContent>(&room_id, &StateEventType::RoomHistoryVisibility, "")
        .map(|c| c.history_visibility)
}

pub fn is_world_readable(room_id: &RoomId) -> AppResult<bool> {
    get_history_visibility(room_id).map(|visibility| visibility == HistoryVisibility::WorldReadable)
}

#[tracing::instrument]
pub fn get_user_state(user_id: &UserId, room_id: &RoomId) -> AppResult<Option<Vec<RawJson<AnyStrippedStateEvent>>>> {
    if let Some(state) = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::state_data)
        .first::<Option<JsonValue>>(&mut connect()?)
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
        event_type: TimelineEventType::RoomMember,
        content,
        state_key: Some(target_user.into()),
        ..Default::default()
    };
    Ok(crate::room::timeline::create_hash_and_sign_event(new_event, sender, room_id).is_ok())
}
pub fn guest_can_join(room_id: &RoomId) -> AppResult<bool> {
    get_room_state_content::<RoomGuestAccessEventContent>(&room_id, &StateEventType::RoomGuestAccess, "")
        .map(|c| c.guest_access == GuestAccess::CanJoin)
}

/// Returns an iterator of all our local users in the room, even if they're
/// deactivated/guests
#[tracing::instrument(level = "debug")]
pub fn local_users_in_room<'a>(room_id: &'a RoomId) -> AppResult<Vec<OwnedUserId>> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::user_id)
        .load::<OwnedUserId>(&mut connect()?)
        .map_err(Into::into)
}

/// Returns an iterator of all our local users in the room, even if they're
/// deactivated/guests
#[tracing::instrument(level = "debug")]
pub fn get_members<'a>(room_id: &'a RoomId) -> AppResult<Vec<OwnedUserId>> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::user_id)
        .load::<OwnedUserId>(&mut connect()?)
        .map_err(Into::into)
}

/// Gets up to five servers that are likely to be in the room in the
/// distant future.
///
/// See <https://spec.matrix.org/latest/appendices/#routing>
#[tracing::instrument(level = "trace")]
pub fn servers_route_via(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    let Ok(pdu) = crate::room::state::get_room_state(room_id, &StateEventType::RoomPowerLevels, "") else {
        return Ok(Vec::new());
    };

    let most_powerful_user_server = pdu
        .get_content::<RoomPowerLevelsEventContent>()?
        .users
        .iter()
        .max_by_key(|(_, power)| *power)
        .and_then(|x| (*x.1 >= 50).then_some(x))
        .map(|(user, _power)| user.server_name().to_owned());

    let mut servers: Vec<OwnedServerName> = crate::room::room_servers(room_id)?.into_iter().take(5).collect();

    if let Some(server) = most_powerful_user_server {
        servers.insert(0, server);
        servers.truncate(5);
    }

    Ok(servers)
}

// TODO: Implement, current just copy servers_route_via
#[tracing::instrument(level = "trace")]
pub fn servers_invite_via(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    let Ok(pdu) = crate::room::state::get_room_state(room_id, &StateEventType::RoomPowerLevels, "") else {
        return Ok(Vec::new());
    };

    let most_powerful_user_server = pdu
        .get_content::<RoomPowerLevelsEventContent>()?
        .users
        .iter()
        .max_by_key(|(_, power)| *power)
        .and_then(|x| (*x.1 >= 50).then_some(x))
        .map(|(user, _power)| user.server_name().to_owned());

    let mut servers: Vec<OwnedServerName> = crate::room::room_servers(room_id)?.into_iter().take(5).collect();

    if let Some(server) = most_powerful_user_server {
        servers.insert(0, server);
        servers.truncate(5);
    }

    Ok(servers)
}

/// Returns an empty vec if not a restricted room
pub fn allowed_room_ids(join_rule: JoinRule) -> Vec<OwnedRoomId> {
    let mut room_ids = Vec::with_capacity(1);
    if let JoinRule::Restricted(r) | JoinRule::KnockRestricted(r) = join_rule {
        for rule in r.allow {
            if let AllowRule::RoomMembership(RoomMembership { room_id: membership }) = rule {
                room_ids.push(membership.clone());
            }
        }
    }
    room_ids
}
