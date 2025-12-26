use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::Serialize;

use crate::admin;
use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::{AuthArgs, DepotExt, JsonResult, json_ok};

pub fn router() -> Router {
    Router::new().push(Router::with_path("v1").push(
        Router::with_path("rooms").get(list_rooms).push(
            Router::with_path("{room_id}").push(Router::with_path("hierarchy").get(get_hierarchy)),
        ),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
struct RoomInfoResponse {
    room_id: String,
    name: String,
    joined_members: u64,
}

#[derive(Debug, Serialize, ToSchema)]
struct RoomsResponse {
    offset: i64,
    total_rooms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_batch: Option<String>,
    rooms: Vec<RoomInfoResponse>,
}

#[handler]
pub fn list_rooms(
    from: QueryParam<i64, false>,
    limit: QueryParam<i64, false>,
) -> JsonResult<RoomsResponse> {
    let offset = from.into_inner().unwrap_or(0).max(0);
    let limit = limit.into_inner().unwrap_or(100).clamp(1, 1000);

    let all_rooms = crate::room::all_room_ids()?;
    let total_rooms = all_rooms.len() as i64;

    let rooms: Vec<RoomInfoResponse> = all_rooms
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|room_id| {
            let info = admin::get_room_info(&room_id);
            RoomInfoResponse {
                room_id: info.id.to_string(),
                name: info.name,
                joined_members: info.joined_members,
            }
        })
        .collect();

    let next_batch = if (offset as usize + rooms.len()) < total_rooms as usize {
        Some((offset + rooms.len() as i64).to_string())
    } else {
        None
    };

    json_ok(RoomsResponse {
        offset,
        total_rooms,
        next_batch,
        rooms,
    })
}

#[handler]
pub async fn get_hierarchy(
    _aa: AuthArgs,
    args: HierarchyReqArgs,
    depot: &mut Depot,
) -> JsonResult<HierarchyResBody> {
    let authed = depot.authed_info()?;

    let res_body = crate::room::space::get_room_hierarchy(authed.user_id(), &args).await?;
    json_ok(res_body)
}
