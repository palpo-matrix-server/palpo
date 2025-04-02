mod event;
pub(super) mod membership;
mod message;
mod receipt;
mod relation;
mod state;
mod tag;
mod thread;
pub(crate) use membership::knock_room;

use std::cmp::max;
use std::collections::BTreeMap;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::json;
use serde_json::value::to_raw_value;

use crate::core::UnixMillis;
use crate::core::client::directory::{PublicRoomsFilteredReqBody, PublicRoomsReqArgs};
use crate::core::client::room::CreateRoomResBody;
use crate::core::client::room::{
    AliasesResBody, CreateRoomReqBody, RoomPreset, SetReadMarkerReqBody, UpgradeRoomReqBody, UpgradeRoomResBody,
};
use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::directory::{PublicRoomFilter, PublicRoomsResBody, RoomNetwork};
use crate::core::events::receipt::{Receipt, ReceiptEvent, ReceiptEventContent, ReceiptThread, ReceiptType};
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::GuestAccess;
use crate::core::events::room::guest_access::RoomGuestAccessEventContent;
use crate::core::events::room::history_visibility::HistoryVisibility;
use crate::core::events::room::history_visibility::RoomHistoryVisibilityEventContent;
use crate::core::events::room::join_rules::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::events::room::tombstone::RoomTombstoneEventContent;
use crate::core::events::room::topic::RoomTopicEventContent;
use crate::core::events::{RoomAccountDataEventType, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::room::Visibility;
use crate::core::serde::{CanonicalJsonObject, JsonValue};
use crate::event::PduBuilder;
use crate::{AppError, AppResult, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, empty_ok, hoops, json_ok};

/// #GET /_matrix/client/v1/rooms/{room_id}/hierarchy``
/// Paginates over the space tree in a depth-first manner to locate child rooms of a given space.
#[endpoint]
async fn get_hierarchy(_aa: AuthArgs, args: HierarchyReqArgs, depot: &mut Depot) -> JsonResult<HierarchyResBody> {
    let authed = depot.authed_info()?;
    let skip = args.from.as_ref().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let limit = args.limit.unwrap_or(10).min(100) as usize;
    let max_depth = args.max_depth.map_or(3, u64::from).min(10) + 1; // +1 to skip the space room itself

    let mut left_to_skip = skip;

    let mut queue: VecDeque<(OwnedRoomId, Vec<OwnedServerName>)> =
        [room_id.to_owned(), vec![room_id.server_name()?.to_owned()]].into();

    let mut rooms = Vec::with_capacity(limit);
    let mut parents = BTreeSet::new();
    let conf = crate::config();

    while let Some((current_room, via)) = queue.pop_front() {
        let summary =
            crate::room::space::get_summary_and_children_client(&current_room, suggested_only, sender_user, &via)
                .await?;

        match (summary, current_room == room_id) {
            (None | Some(SummaryAccessibility::Inaccessible), false) => {
                // Just ignore other unavailable rooms
            }
            (None, true) => {
                return Err(MatrixError::forbidden("The requested room was not found").into());
            }
            (Some(SummaryAccessibility::Inaccessible), true) => {
                return Err(MatrixError::forbidden("The requested room is inaccessible").into());
            }
            (Some(SummaryAccessibility::Accessible(summary)), _) => {
                let populate = parents.len() >= room_sns.clone().count();

                let mut children: Vec<(OwnedRoomId, Vec<OwnedServerName>)> =
                    get_parent_children_via(&summary, suggested_only)
                        .filter(|(room, _)| !parents.contains(room))
                        .rev()
                        .map(|(key, val)| (key, val.collect()))
                        .collect();

                if !populate {
                    children = children
                        .iter()
                        .rev()
                        .skip_while(|(room, _)| {
                            let room_sn = crate::room::get_room_sn(room)?;
                            room_sn
                                .map_ok(|short| Some(&short) != room_sns.clone().nth(parents.len()))
                                .unwrap_or_else(|_| false)
                        })
                        .map(Clone::clone)
                        .rev()
                        .collect::<Vec<(OwnedRoomId, Vec<OwnedServerName>)>>();
                }

                if populate {
                    rooms.push(summary_to_chunk(summary.clone()));
                } else if queue.is_empty() && children.is_empty() {
                    return Err(MatrixError::invalid_param("Room IDs in token were not found.").into());
                }

                parents.insert(current_room.clone());
                if rooms.len() >= limit {
                    break;
                }

                if children.is_empty() {
                    break;
                }

                if parents.len() >= max_depth {
                    continue;
                }

                queue.extend(children);
            }
        }
    }

    let next_batch = if let Some((room, _)) = queue.pop_front() {
        parents.insert(room);

        let next_room_sns: Vec<_> = parents
            .iter()
            .filter_map(|room_id| crate::room::get_room_sn(room_id).ok())
            .collect()
            .await;

        (next_room_sns.iter().ne(room_sns) && !next_room_sns.is_empty())
            .then_some(PaginationToken {
                room_sns: next_room_sns,
                limit: max_depth.try_into().ok()?,
                max_depth: max_depth.try_into().ok()?,
                suggested_only,
            })
            .as_ref()
            .map(PaginationToken::to_string)
    } else {
        None
    };

    json_ok(HierarchyResBody { next_batch, rooms })
}
