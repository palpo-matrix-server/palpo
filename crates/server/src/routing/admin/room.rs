use std::str::FromStr;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::client::device::Device;
use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::federation::authorization::{EventAuthReqArgs, EventAuthResBody};
use crate::core::federation::event::{
    EventReqArgs, EventResBody, MissingEventsReqBody, MissingEventsResBody,
};
use crate::core::identifiers::*;
use crate::core::room::{TimestampToEventReqArgs, TimestampToEventResBody};
use crate::data::room::DbEvent;
use crate::room::space::PaginationToken;
use crate::room::{state, timeline};
use crate::{
    AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, config, data, empty_ok,
    json_ok,
};

pub fn router() -> Router {
    Router::new().push(Router::with_path("v1").push(
        Router::with_path("rooms").get(list_rooms).push(
            Router::with_path("{room_id}").push(Router::with_path("hierarchy").get(get_hierarchy)),
        ),
    ))
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

#[handler]
pub fn list_rooms(
    user_id: PathParam<OwnedUserId>,
    device_id: PathParam<OwnedDeviceId>,
) -> JsonResult<Device> {
    let Ok(device) = data::user::device::get_device(&user_id, &device_id) else {
        return Err(MatrixError::not_found("device is not found.").into());
    };
    json_ok(device.into_matrix_device())
}
