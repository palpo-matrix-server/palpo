use std::sync::{LazyLock, Mutex};

use crate::{room::state::DbRoomStateField, GetUrlOrigin};
use lru_cache::LruCache;
use tracing::{debug, error, warn};

use crate::core::client::space::HierarchyResBody;
use crate::core::client::space::{HierarchyReqArgs, SpaceHierarchyRoomsChunk};
use crate::core::events::room::{
    avatar::RoomAvatarEventContent,
    canonical_alias::RoomCanonicalAliasEventContent,
    create::RoomCreateEventContent,
    guest_access::{GuestAccess, RoomGuestAccessEventContent},
    history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
    join_rules::{self, AllowRule, JoinRule, RoomJoinRulesEventContent},
    topic::RoomTopicEventContent,
};
use crate::core::events::{space::child::SpaceChildEventContent, StateEventType};
use crate::core::federation::space::hierarchy_request;
use crate::core::{federation, space::SpaceRoomJoinRule, OwnedRoomId, RoomId, UserId};
use crate::PduEvent;
use crate::{AppError, AppResult, MatrixError};

pub enum CachedJoinRule {
    //Simplified(SpaceRoomJoinRule),
    Full(JoinRule),
}

pub struct CachedSpaceChunk {
    chunk: SpaceHierarchyRoomsChunk,
    children: Vec<OwnedRoomId>,
    join_rule: CachedJoinRule,
}

pub static ROOM_ID_SPACE_CHUNK_CACHE: LazyLock<Mutex<LruCache<OwnedRoomId, Option<CachedSpaceChunk>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(100)));

pub async fn get_hierarchy(
    user_id: &UserId,
    room_id: &RoomId,
    limit: usize,
    skip: u64,
    max_depth: u64,
    suggested_only: bool,
) -> AppResult<HierarchyResBody> {
    let mut left_to_skip = skip;

    let mut rooms_in_path = Vec::new();
    let mut stack = vec![vec![room_id.to_owned()]];
    let mut results = Vec::new();
    let conf = crate::config();

    while let Some(current_room) = {
        while stack.last().map_or(false, |s| s.is_empty()) {
            stack.pop();
        }
        if !stack.is_empty() {
            stack.last_mut().and_then(|s| s.pop())
        } else {
            None
        }
    } {
        rooms_in_path.push(current_room.clone());
        if results.len() >= limit {
            break;
        }

        if let Some(cached) = ROOM_ID_SPACE_CHUNK_CACHE
            .lock()
            .unwrap()
            .get_mut(&current_room.to_owned())
            .as_ref()
        {
            if let Some(cached) = cached {
                let allowed = match &cached.join_rule {
                    //CachedJoinRule::Simplified(s) => {
                    //self.handle_simplified_join_rule(s, authed.user_id(), &current_room)?
                    //}
                    CachedJoinRule::Full(f) => handle_join_rule(f, user_id, &current_room)?,
                };
                if allowed {
                    if left_to_skip > 0 {
                        left_to_skip -= 1;
                    } else {
                        results.push(cached.chunk.clone());
                    }
                    if (rooms_in_path.len() as u64) < max_depth {
                        stack.push(cached.children.clone());
                    }
                }
            }
            continue;
        }

        if let Some(current_state_hash) = crate::room::state::get_room_frame_id(&current_room, None)? {
            let state = crate::room::state::get_full_state_ids(current_state_hash)?;

            let mut children_ids = Vec::new();
            let mut children_pdus = Vec::new();
            for (key, event_id) in state {
                let DbRoomStateField {
                    event_ty, state_key, ..
                } = crate::room::state::get_field(key)?;
                if event_ty != StateEventType::SpaceChild {
                    continue;
                }

                let pdu = crate::room::timeline::get_pdu(&event_id)?
                    .ok_or_else(|| AppError::internal("Event in space state not found"))?;

                if serde_json::from_str::<SpaceChildEventContent>(pdu.content.get())
                    .ok()
                    .map(|c| c.via)
                    .map_or(true, |v| v.is_empty())
                {
                    continue;
                }

                if let Ok(room_id) = OwnedRoomId::try_from(state_key) {
                    children_ids.push(room_id);
                    children_pdus.push(pdu);
                }
            }

            // TODO: Sort children
            children_ids.reverse();

            let chunk = get_room_chunk(user_id, &current_room, children_pdus);
            if let Ok(chunk) = chunk {
                if left_to_skip > 0 {
                    left_to_skip -= 1;
                } else {
                    results.push(chunk.clone());
                }
                let join_rule = crate::room::state::get_state(&current_room, &StateEventType::RoomJoinRules, "", None)?
                    .map(|s| {
                        serde_json::from_str(s.content.get())
                            .map(|c: RoomJoinRulesEventContent| c.join_rule)
                            .map_err(|e| {
                                error!("Invalid room join rule event in database: {}", e);
                                AppError::public("Invalid room join rule event in database.")
                            })
                    })
                    .transpose()?
                    .unwrap_or(JoinRule::Invite);

                ROOM_ID_SPACE_CHUNK_CACHE.lock().unwrap().insert(
                    current_room.clone(),
                    Some(CachedSpaceChunk {
                        chunk,
                        children: children_ids.clone(),
                        join_rule: CachedJoinRule::Full(join_rule),
                    }),
                );
            }

            if (rooms_in_path.len() as u64) < max_depth {
                stack.push(children_ids);
            }
        } else {
            let server = current_room.server_name().map_err(AppError::public)?;
            if server == &conf.server_name {
                continue;
            }
            if !results.is_empty() {
                // Early return so the client can see some data already
                break;
            }
            debug!("Asking {server} for /hierarchy");
            let request = hierarchy_request(
                &server.origin().await,
                federation::space::HierarchyReqArgs {
                    room_id: current_room.clone(),
                    suggested_only,
                },
            )?
            .into_inner();
            if let Ok(response) = crate::sending::send_federation_request(&server, request)
                .await?
                .json::<federation::space::HierarchyResBody>()
                .await
            {
                warn!("Got response from {server} for /hierarchy\n{response:?}");
                let chunk = SpaceHierarchyRoomsChunk {
                    canonical_alias: response.room.canonical_alias,
                    name: response.room.name,
                    num_joined_members: response.room.num_joined_members,
                    room_id: response.room.room_id,
                    topic: response.room.topic,
                    world_readable: response.room.world_readable,
                    guest_can_join: response.room.guest_can_join,
                    avatar_url: response.room.avatar_url,
                    join_rule: response.room.join_rule.clone(),
                    room_type: response.room.room_type,
                    children_state: response.room.children_state,
                };
                let children = response.children.iter().map(|c| c.room_id.clone()).collect::<Vec<_>>();

                let join_rule = match response.room.join_rule {
                    SpaceRoomJoinRule::Invite => JoinRule::Invite,
                    SpaceRoomJoinRule::Knock => JoinRule::Knock,
                    SpaceRoomJoinRule::Private => JoinRule::Private,
                    SpaceRoomJoinRule::Restricted => JoinRule::Restricted(join_rules::Restricted {
                        allow: response
                            .room
                            .allowed_room_ids
                            .into_iter()
                            .map(|room| AllowRule::room_membership(room))
                            .collect(),
                    }),
                    SpaceRoomJoinRule::KnockRestricted => JoinRule::KnockRestricted(join_rules::Restricted {
                        allow: response
                            .room
                            .allowed_room_ids
                            .into_iter()
                            .map(|room| AllowRule::room_membership(room))
                            .collect(),
                    }),
                    SpaceRoomJoinRule::Public => JoinRule::Public,
                    _ => return Err(AppError::public("Unknown join rule")),
                };
                if handle_join_rule(&join_rule, user_id, &current_room)? {
                    if left_to_skip > 0 {
                        left_to_skip -= 1;
                    } else {
                        results.push(chunk.clone());
                    }
                    if (rooms_in_path.len() as u64) < max_depth {
                        stack.push(children.clone());
                    }
                }

                ROOM_ID_SPACE_CHUNK_CACHE.lock().unwrap().insert(
                    current_room.clone(),
                    Some(CachedSpaceChunk {
                        chunk,
                        children,
                        join_rule: CachedJoinRule::Full(join_rule),
                    }),
                );

                /* TODO:
                for child in response.children {
                    ROOM_ID_SPACE_CHUNK_CACHE.insert(
                        current_room.clone(),
                        CachedSpaceChunk {
                            chunk: child.chunk,
                            children,
                            join_rule,
                        },
                    );
                }*/
            } else {
                ROOM_ID_SPACE_CHUNK_CACHE
                    .lock()
                    .unwrap()
                    .insert(current_room.clone(), None);
            }
        }
    }

    Ok(HierarchyResBody {
        next_batch: if results.is_empty() {
            None
        } else {
            Some((skip + results.len() as u64).to_string())
        },
        rooms: results,
    })
}

fn get_room_chunk(user_id: &UserId, room_id: &RoomId, children: Vec<PduEvent>) -> AppResult<SpaceHierarchyRoomsChunk> {
    Ok(SpaceHierarchyRoomsChunk {
        canonical_alias: crate::room::state::get_state(&room_id, &StateEventType::RoomCanonicalAlias, "", None)?
            .map_or(Ok(None), |s| {
                serde_json::from_str(s.content.get())
                    .map(|c: RoomCanonicalAliasEventContent| c.alias)
                    .map_err(|_| AppError::internal("Invalid canonical alias event in database."))
            })?,
        name: crate::room::state::get_name(&room_id, None)?,
        num_joined_members: crate::room::joined_member_count(&room_id)?
            .try_into()
            .expect("user count should not be that big"),
        room_id: room_id.to_owned(),
        topic: crate::room::state::get_state(&room_id, &StateEventType::RoomTopic, "", None)?.map_or(
            Ok(None),
            |s| {
                serde_json::from_str(s.content.get())
                    .map(|c: RoomTopicEventContent| Some(c.topic))
                    .map_err(|_| {
                        error!("Invalid room topic event in database for room {}", room_id);
                        AppError::internal("Invalid room topic event in database.")
                    })
            },
        )?,
        world_readable: crate::room::state::get_state(&room_id, &StateEventType::RoomHistoryVisibility, "", None)?
            .map_or(Ok(false), |s| {
                serde_json::from_str(s.content.get())
                    .map(|c: RoomHistoryVisibilityEventContent| {
                        c.history_visibility == HistoryVisibility::WorldReadable
                    })
                    .map_err(|_| AppError::internal("Invalid room history visibility event in database."))
            })?,
        guest_can_join: crate::room::state::get_state(&room_id, &StateEventType::RoomGuestAccess, "", None)?.map_or(
            Ok(false),
            |s| {
                serde_json::from_str(s.content.get())
                    .map(|c: RoomGuestAccessEventContent| c.guest_access == GuestAccess::CanJoin)
                    .map_err(|_| AppError::internal("Invalid room guest access event in database."))
            },
        )?,
        avatar_url: crate::room::state::get_state(&room_id, &StateEventType::RoomAvatar, "", None)?
            .map(|s| {
                serde_json::from_str(s.content.get())
                    .map(|c: RoomAvatarEventContent| c.url)
                    .map_err(|_| AppError::internal("Invalid room avatar event in database."))
            })
            .transpose()?
            // url is now an Option<String> so we must flatten
            .flatten(),
        join_rule: {
            let join_rule = crate::room::state::get_state(&room_id, &StateEventType::RoomJoinRules, "", None)?
                .map(|s| {
                    serde_json::from_str(s.content.get())
                        .map(|c: RoomJoinRulesEventContent| c.join_rule)
                        .map_err(|e| {
                            error!("Invalid room join rule event in database: {}", e);
                            AppError::public("Invalid room join rule event in database.")
                        })
                })
                .transpose()?
                .unwrap_or(JoinRule::Invite);

            if !handle_join_rule(&join_rule, user_id, room_id)? {
                debug!("User is not allowed to see room {room_id}");
                // This error will be caught later
                return Err(MatrixError::forbidden("User is not allowed to see the room").into());
            }

            translate_joinrule(&join_rule)?
        },
        room_type: crate::room::state::get_state(&room_id, &StateEventType::RoomCreate, "", None)?
            .map(|s| {
                serde_json::from_str::<RoomCreateEventContent>(s.content.get()).map_err(|e| {
                    error!("Invalid room create event in database: {}", e);
                    AppError::public("Invalid room create event in database.")
                })
            })
            .transpose()?
            .and_then(|e| e.room_type),
        children_state: children
            .into_iter()
            .map(|pdu| pdu.to_stripped_spacechild_state_event())
            .collect(),
    })
}

fn translate_joinrule(join_rule: &JoinRule) -> AppResult<SpaceRoomJoinRule> {
    match join_rule {
        JoinRule::Invite => Ok(SpaceRoomJoinRule::Invite),
        JoinRule::Knock => Ok(SpaceRoomJoinRule::Knock),
        JoinRule::Private => Ok(SpaceRoomJoinRule::Private),
        JoinRule::Restricted(_) => Ok(SpaceRoomJoinRule::Restricted),
        JoinRule::KnockRestricted(_) => Ok(SpaceRoomJoinRule::KnockRestricted),
        JoinRule::Public => Ok(SpaceRoomJoinRule::Public),
        _ => Err(AppError::public("Unknown join rule")),
    }
}

fn handle_simplified_join_rule(join_rule: &SpaceRoomJoinRule, user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    let allowed = match join_rule {
        SpaceRoomJoinRule::Public => true,
        SpaceRoomJoinRule::Knock => true,
        SpaceRoomJoinRule::Invite => crate::room::is_joined(user_id, &room_id)?,
        _ => false,
    };

    Ok(allowed)
}

fn handle_join_rule(join_rule: &JoinRule, user_id: &UserId, room_id: &RoomId) -> AppResult<bool> {
    if handle_simplified_join_rule(&translate_joinrule(join_rule)?, user_id, room_id)? {
        return Ok(true);
    }

    match join_rule {
        JoinRule::Restricted(r) => {
            for rule in &r.allow {
                match rule {
                    join_rules::AllowRule::RoomMembership(rm) => {
                        if let Ok(true) = crate::room::is_joined(user_id, &rm.room_id) {
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }

            Ok(false)
        }
        JoinRule::KnockRestricted(_) => {
            // TODO: Check rules
            Ok(false)
        }
        _ => Ok(false),
    }
}
