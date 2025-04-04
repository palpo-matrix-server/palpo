use std::cmp::max;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::str::FromStr;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::json;
use serde_json::value::to_raw_value;

use crate::core::client::directory::{PublicRoomsFilteredReqBody, PublicRoomsReqArgs};
use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::directory::{PublicRoomFilter, PublicRoomsResBody, RoomNetwork};
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::identifiers::*;
use crate::room::space::{PaginationToken, SummaryAccessibility, get_parent_children_via, summary_to_chunk};
use crate::{AppError, AuthArgs, DepotExt, JsonResult, MatrixError, json_ok};

/// #GET /_matrix/client/v1/rooms/{room_id}/hierarchy``
/// Paginates over the space tree in a depth-first manner to locate child rooms of a given space.
#[endpoint]
pub(super) async fn get_hierarchy(
    _aa: AuthArgs,
    args: HierarchyReqArgs,
    depot: &mut Depot,
) -> JsonResult<HierarchyResBody> {
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let skip = args.from.as_ref().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let limit = args.limit.unwrap_or(10).min(100) as usize;
    let max_depth = args.max_depth.map_or(3, usize::from).min(10) + 1; // +1 to skip the space room itself
    let pagination_token = args.from.as_ref().and_then(|s| PaginationToken::from_str(s).ok());

    let room_sns = pagination_token.map(|p| p.room_sns).unwrap_or_default();

    let mut left_to_skip = skip;
    let room_id = &args.room_id;
    let suggested_only = args.suggested_only;

    let mut queue: VecDeque<(OwnedRoomId, Vec<OwnedServerName>)> = [(
        room_id.to_owned(),
        vec![
            room_id
                .server_name()
                .map_err(|name| AppError::public(format!("bad server name: {name}")))?
                .to_owned(),
        ],
    )]
    .into();

    let mut rooms = Vec::with_capacity(limit);
    let mut parents = BTreeSet::new();
    let conf = crate::config();

    while let Some((current_room, via)) = queue.pop_front() {
        let summary =
            crate::room::space::get_summary_and_children_client(&current_room, suggested_only, sender_id, &via).await?;

        match (summary, &current_room == room_id) {
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
                let populate = parents.len() >= room_sns.len();

                let mut children: Vec<(OwnedRoomId, Vec<OwnedServerName>)> =
                    get_parent_children_via(&summary, suggested_only)
                        .into_iter()
                        .filter(|(room, _)| !parents.contains(room))
                        .rev()
                        .collect();

                if !populate {
                    children = children
                        .iter()
                        .rev()
                        .skip_while(|(room, _)| {
                            crate::room::get_room_sn(room)
                                .map(|room_sn| Some(&room_sn) != room_sns.get(parents.len()))
                                .unwrap_or_else(|_| false)
                        })
                        .map(Clone::clone)
                        .collect::<Vec<_>>()
                        .into_iter()
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
            .collect();

        if !next_room_sns.is_empty() && next_room_sns.iter().ne(&room_sns)  {
            Some(
                PaginationToken {
                    room_sns: next_room_sns,
                    limit,
                    max_depth,
                    suggested_only,
                }
                .to_string(),
            )
        } else {
            None
        }
    } else {
        None
    };

    json_ok(HierarchyResBody { next_batch, rooms })
}
