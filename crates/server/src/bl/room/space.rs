use std::collections::{BTreeSet, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};

use clap::error::Error;
use lru_cache::LruCache;

use crate::PduEvent;
use crate::core::client::space::SpaceHierarchyRoomsChunk;
use crate::core::events::room::{
    canonical_alias::RoomCanonicalAliasEventContent,
    create::RoomCreateEventContent,
    guest_access::{GuestAccess, RoomGuestAccessEventContent},
    history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
    join_rules::{self, AllowRule, JoinRule, RoomJoinRulesEventContent},
    topic::RoomTopicEventContent,
};
use crate::core::events::space::child::HierarchySpaceChildEvent;
use crate::core::events::{StateEventType, space::child::SpaceChildEventContent};
use crate::core::federation::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::federation::space::{SpaceHierarchyChildSummary, SpaceHierarchyParentSummary, hierarchy_request};
use crate::core::identifiers::*;
use crate::core::serde::RawJson;
use crate::core::{OwnedRoomId, RoomId, UserId, federation, space::SpaceRoomJoinRule};
use crate::{AppError, AppResult, GetUrlOrigin, MatrixError, room::state::DbRoomStateField};

pub static ROOM_ID_SPACE_CHUNK_CACHE: LazyLock<Mutex<LruCache<OwnedRoomId, Option<CachedSpaceHierarchySummary>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(100)));

pub struct CachedSpaceHierarchySummary {
    summary: SpaceHierarchyParentSummary,
}

pub enum SummaryAccessibility {
    Accessible(SpaceHierarchyParentSummary),
    Inaccessible,
}

/// Identifier used to check if rooms are accessible. None is used if you want
/// to return the room, no matter if accessible or not
pub enum Identifier<'a> {
    UserId(&'a UserId),
    ServerName(&'a ServerName),
}

/// Gets the summary of a space using solely local information
pub async fn get_summary_and_children_local(
    current_room: &RoomId,
    identifier: &Identifier<'_>,
) -> AppResult<Option<SummaryAccessibility>> {
    match ROOM_ID_SPACE_CHUNK_CACHE.lock().unwrap().get_mut(current_room).as_ref() {
        None => (), // cache miss
        Some(None) => return Ok(None),
        Some(Some(cached)) => {
            return Ok(Some(
                if is_accessible_child(
                    current_room,
                    &cached.summary.join_rule,
                    identifier,
                    &cached.summary.allowed_room_ids,
                ) {
                    SummaryAccessibility::Accessible(cached.summary.clone())
                } else {
                    SummaryAccessibility::Inaccessible
                },
            ));
        }
    }

    let children_pdus: Vec<_> = get_stripped_space_child_events(current_room)?;

    let Ok(summary) = get_room_summary(current_room, children_pdus, identifier).await else {
        return Ok(None);
    };

    ROOM_ID_SPACE_CHUNK_CACHE.lock().unwrap().insert(
        current_room.to_owned(),
        Some(CachedSpaceHierarchySummary {
            summary: summary.clone(),
        }),
    );

    Ok(Some(SummaryAccessibility::Accessible(summary)))
}

/// Gets the summary of a space using solely federation
#[tracing::instrument(level = "debug")]
async fn get_summary_and_children_federation(
    current_room: &RoomId,
    suggested_only: bool,
    user_id: &UserId,
    via: &[OwnedServerName],
) -> AppResult<Option<SummaryAccessibility>> {
    let mut res_body = None;
    for server in via {
        let request = hierarchy_request(
            &server.origin().await,
            HierarchyReqArgs {
                room_id: current_room.to_owned(),
                suggested_only,
            },
        )?
        .into_inner();

        if let Ok(respone) = crate::sending::send_federation_request(server, request).await {
            if let Ok(body) = respone.json::<HierarchyResBody>().await {
                let summary = body.room;
                ROOM_ID_SPACE_CHUNK_CACHE.lock().unwrap().insert(
                    current_room.to_owned(),
                    Some(CachedSpaceHierarchySummary {
                        summary: summary.clone(),
                    }),
                );
                res_body = Some(body);
            }
        }
        if res_body.is_some() {
            break;
        }
    }
    let Some(res_body) = res_body else {
        return Ok(None);
    };

    res_body
        .children
        .into_iter()
        .then(|child| ROOM_ID_SPACE_CHUNK_CACHE.lock().map(|lock| (child, lock)))
        .filter_map(|(child, mut cache)| (!cache.contains_key(current_room)).then_some((child, cache)))
        .for_each(|(child, cache)| cache_insert(cache, current_room, child));

    let identifier = Identifier::UserId(user_id);
    let is_accessible_child =
        is_accessible_child(current_room, &summary.join_rule, &identifier, &summary.allowed_room_ids);

    if is_accessible_child {
        return Ok(Some(SummaryAccessibility::Accessible(summary)));
    }

    Ok(Some(SummaryAccessibility::Inaccessible))
}

/// Simply returns the stripped m.space.child events of a room
fn get_stripped_space_child_events(room_id: &RoomId) -> AppResult<Vec<RawJson<HierarchySpaceChildEvent>>> {
    crate::room::get_room_sn(room_id)
        .map_ok(|current_shortstatehash| {
            crate::room::state::state_keys_with_ids(current_shortstatehash, &StateEventType::SpaceChild)
        })
        .filter_map(move |(state_key, event_id): (_, OwnedEventId)| {
            crate::room::timeline::get_pdu(&event_id)
                .map_ok(move |pdu| (state_key, pdu))
                .ok()
        })
        .filter_map(move |(state_key, pdu)| {
            if let Ok(content) = pdu.get_content::<SpaceChildEventContent>() {
                if content.via.is_empty() {
                    return None;
                }
            }

            if RoomId::parse(&state_key).is_ok() {
                return Some(pdu.to_stripped_spacechild_state_event());
            }

            None
        })
}

/// Gets the summary of a space using either local or remote (federation)
/// sources
pub async fn get_summary_and_children_client(
    current_room: &OwnedRoomId,
    suggested_only: bool,
    user_id: &UserId,
    via: &[OwnedServerName],
) -> AppResult<Option<SummaryAccessibility>> {
    let identifier = Identifier::UserId(user_id);

    if let Ok(Some(response)) = get_summary_and_children_local(current_room, &identifier) {
        return Ok(Some(response));
    }

    get_summary_and_children_federation(current_room, suggested_only, user_id, via).await
}

async fn get_room_summary(
    room_id: &RoomId,
    children_state: Vec<RawJson<HierarchySpaceChildEvent>>,
    identifier: &Identifier<'_>,
) -> AppResult<SpaceHierarchyParentSummary> {
    let join_rule = crate::room::state::room_state_get_content(room_id, &StateEventType::RoomJoinRules, "")
        .map_or(JoinRule::Invite, |c: RoomJoinRulesEventContent| c.join_rule);

    let allowed_room_ids = crate::room::state::allowed_room_ids(join_rule.clone());

    let join_rule = join_rule.clone().into();
    let is_accessible_child = is_accessible_child(room_id, &join_rule, identifier, &allowed_room_ids);

    if !is_accessible_child {
        return Err(MatrixError::forbidden("User is not allowed to see the room").into());
    }

    let name = crate::room::state::get_name(room_id).ok().flatten();
    let topic = crate::room::get_room_topic(room_id).ok();
    let room_type = crate::room::state::get_room_type(room_id).ok();
    let world_readable = crate::room::state::is_world_readable(room_id);
    let guest_can_join = crate::room::state::guest_can_join(room_id)?;
    let num_joined_members = crate::room::room_joined_count(room_id).unwrap_or(0);
    let canonical_alias = crate::room::get_canonical_alias(room_id).ok();
    let avatar_url =
        crate::room::state::AppearsOnTableget_avatar(room_id).map(|res| res.into_option().unwrap_or_default().url);

    Ok(SpaceHierarchyParentSummary {
        canonical_alias,
        name,
        topic,
        world_readable,
        guest_can_join,
        avatar_url,
        room_type,
        children_state,
        allowed_room_ids,
        join_rule,
        room_id: room_id.to_owned(),
        num_joined_members: num_joined_members
            .try_into()
            .expect("user count should not be that big"),
    })
}

/// With the given identifier, checks if a room is accessable
fn is_accessible_child(
    current_room: &RoomId,
    join_rule: &SpaceRoomJoinRule,
    identifier: &Identifier<'_>,
    allowed_room_ids: &[OwnedRoomId],
) -> bool {
    if let Identifier::ServerName(server_name) = identifier {
        // Checks if ACLs allow for the server to participate
        if crate::event::handler::acl_check(server_name, current_room).is_err() {
            return false;
        }
    }

    if let Identifier::UserId(user_id) = identifier {
        if crate::room::is_joined(user_id, current_room)? {
            return true;
        }

        if crate::room::is_invited(user_id, current_room)? {
            return true;
        }
    }

    match join_rule {
        SpaceRoomJoinRule::Public | SpaceRoomJoinRule::Knock | SpaceRoomJoinRule::KnockRestricted => true,
        SpaceRoomJoinRule::Restricted => allowed_room_ids.iter().any(|room| match identifier {
            Identifier::UserId(user) => crate::room::is_joined(user, room),
            Identifier::ServerName(server) => crate::room::state::server_in_room(server, room),
        }),

        // Invite only, Private, or Custom join rule
        _ => false,
    }
}

/// Returns the children of a SpaceHierarchyParentSummary, making use of the
/// children_state field
pub fn get_parent_children_via(
    parent: &SpaceHierarchyParentSummary,
    suggested_only: bool,
) -> Vec<(OwnedRoomId, Vec<OwnedServerName>)> {
    parent
        .children_state
        .iter()
        .map(RawJson::deserialize)
        .filter_map(Result::ok)
        .filter_map(move |ce| {
            (!suggested_only || ce.content.suggested).then_some((ce.state_key, ce.content.via.into_iter()))
        })
}

async fn cache_insert(mut cache: MutexGuard<'_, Cache>, current_room: &RoomId, child: SpaceHierarchyChildSummary) {
    let SpaceHierarchyChildSummary {
        canonical_alias,
        name,
        num_joined_members,
        room_id,
        topic,
        world_readable,
        guest_can_join,
        avatar_url,
        join_rule,
        room_type,
        allowed_room_ids,
    } = child;

    let summary = SpaceHierarchyParentSummary {
        canonical_alias,
        name,
        num_joined_members,
        topic,
        world_readable,
        guest_can_join,
        avatar_url,
        join_rule,
        room_type,
        allowed_room_ids,
        room_id: room_id.clone(),
        children_state: get_stripped_space_child_events(&room_id).collect().await,
    };

    cache.insert(current_room.to_owned(), Some(CachedSpaceHierarchySummary { summary }));
}

// Here because cannot implement `From` across ruma-federation-api and
// ruma-client-api types
impl From<CachedSpaceHierarchySummary> for SpaceHierarchyRoomsChunk {
    fn from(value: CachedSpaceHierarchySummary) -> Self {
        let SpaceHierarchyParentSummary {
            canonical_alias,
            name,
            num_joined_members,
            room_id,
            topic,
            world_readable,
            guest_can_join,
            avatar_url,
            join_rule,
            room_type,
            children_state,
            ..
        } = value.summary;

        Self {
            canonical_alias,
            name,
            num_joined_members,
            room_id,
            topic,
            world_readable,
            guest_can_join,
            avatar_url,
            join_rule,
            room_type,
            children_state,
        }
    }
}

/// Here because cannot implement `From` across ruma-federation-api and
/// ruma-client-api types
#[must_use]
pub fn summary_to_chunk(summary: SpaceHierarchyParentSummary) -> SpaceHierarchyRoomsChunk {
    let SpaceHierarchyParentSummary {
        canonical_alias,
        name,
        num_joined_members,
        room_id,
        topic,
        world_readable,
        guest_can_join,
        avatar_url,
        join_rule,
        room_type,
        children_state,
        ..
    } = summary;

    SpaceHierarchyRoomsChunk {
        canonical_alias,
        name,
        num_joined_members,
        room_id,
        topic,
        world_readable,
        guest_can_join,
        avatar_url,
        join_rule,
        room_type,
        children_state,
    }
}
