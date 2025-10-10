use std::collections::{BTreeSet, VecDeque};
use std::str::FromStr;

use salvo::prelude::*;

use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::identifiers::*;
use crate::core::room::RoomType;
use crate::room::space::{
    PaginationToken, SummaryAccessibility, get_parent_children_via, summary_to_chunk,
};
use crate::{AppError, AuthArgs, DepotExt, JsonResult, MatrixError, json_ok};

/// `#GET /_matrix/client/v1/rooms/{room_id}/hierarchy`
/// Paginates over the space tree in a depth-first manner to locate child rooms of a given space.
#[endpoint]
pub(super) async fn get_hierarchy(
    _aa: AuthArgs,
    args: HierarchyReqArgs,
    depot: &mut Depot,
) -> JsonResult<HierarchyResBody> {
    type Entry = (OwnedRoomId, Vec<OwnedServerName>);
    type RoomDeque = VecDeque<Entry>;

    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let limit = args.limit.unwrap_or(50).min(50);
    let max_depth = args.max_depth.unwrap_or(usize::MAX);
    let pagination_token = args
        .from
        .as_ref()
        .and_then(|s| PaginationToken::from_str(s).ok());

    println!("======================pagination_token: {pagination_token:?}");
    // Should prevent unexpeded behaviour in (bad) clients
    if let Some(token) = &pagination_token
        && (token.suggested_only != args.suggested_only || token.max_depth != max_depth)
    {
        return Err(MatrixError::invalid_param(
            "suggested_only and max_depth cannot change on paginated requests",
        )
        .into());
    }

    let room_sns = pagination_token.map(|p| p.room_sns).unwrap_or_default();
    let room_id = &args.room_id;
    let suggested_only = args.suggested_only;
    let mut queue: RoomDeque =
        [(room_id.to_owned(), vec![crate::room::server_name(room_id)?])].into();

    let mut rooms = Vec::with_capacity(limit);
    let mut parents = BTreeSet::new();
    while let Some((current_room, via)) = queue.pop_front() {
        println!("===============mm   ===========");
        let summary = match crate::room::space::get_summary_and_children_client(
            &current_room,
            suggested_only,
            sender_id,
            &via,
        )
        .await
        {
            Ok(summary) => summary,
            Err(e) => {
                println!("===============mm   0");
                error!("failed to get space summary for {}: {}", current_room, e);
                None
            }
        };

        match (summary, &current_room == room_id) {
            (None | Some(SummaryAccessibility::Inaccessible), false) => {
                println!("===============mm   1");
                // Just ignore other unavailable rooms
            }
            (None, true) => {
                println!("===============mm   2");
                return Err(
                    MatrixError::forbidden("the requested room was not found", None).into(),
                );
            }
            (Some(SummaryAccessibility::Inaccessible), true) => {
                println!("===============mm   3");
                return Err(
                    MatrixError::forbidden("the requested room is inaccessible", None).into(),
                );
            }
            (Some(SummaryAccessibility::Accessible(summary)), _) => {
                println!("===============mm   4");
                let populate = parents.len() >= room_sns.len();
                println!(
                    "====          ============== parents.len(): {}   room_sns.len(): {}   populate: {}",
                    parents.len(),
                    room_sns.len(),
                    populate
                );

                let mut children = Vec::new();
                if crate::room::get_room_type(&current_room).ok().flatten() == Some(RoomType::Space)
                {
                    children = get_parent_children_via(&summary, suggested_only)
                        .into_iter()
                        .filter(|(room, _)| !parents.contains(room))
                        .rev()
                        .collect::<Vec<Entry>>();

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
                            .collect::<Vec<Entry>>();
                    }
                    println!(
                        "cccccc current: {current_room}   populate: {populate} summary:{summary:?} children: {children:?}"
                    );
                }

                if populate {
                    rooms.push(summary_to_chunk(summary.clone()));
                } else if queue.is_empty() && children.is_empty() {
                    println!("===============mm   5");
                    break;
                }

                parents.insert(current_room.clone());
                if rooms.len() >= limit {
                    println!("===============mm   6");
                    break;
                }

                if parents.len() > max_depth {
                    println!("===============mm   7");
                    continue;
                }

                println!("===============mm   8");
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

        if !next_room_sns.is_empty() && next_room_sns.iter().ne(&room_sns) {
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
