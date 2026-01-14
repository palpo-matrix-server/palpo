use salvo::prelude::*;

use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::{AuthArgs, DepotExt, JsonResult, json_ok, room};

/// `#GET /_matrix/client/v1/rooms/{room_id}/hierarchy`
/// Paginates over the space tree in a depth-first manner to locate child rooms of a given space.
#[endpoint]
pub(super) async fn get_hierarchy(
    _aa: AuthArgs,
    args: HierarchyReqArgs,
    depot: &mut Depot,
) -> JsonResult<HierarchyResBody> {
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();

    let res_body = room::space::get_room_hierarchy(sender_id, &args).await?;
    json_ok(res_body)
}
