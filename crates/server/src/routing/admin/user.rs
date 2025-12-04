use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::client::device::Device;
use crate::core::federation::authorization::{EventAuthReqArgs, EventAuthResBody};
use crate::core::federation::event::{
    EventReqArgs, EventResBody, MissingEventsReqBody, MissingEventsResBody,
};
use crate::core::identifiers::*;
use crate::core::room::{TimestampToEventReqArgs, TimestampToEventResBody};
use crate::data::room::DbEvent;
use crate::room::{state, timeline};
use crate::{
    AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, config, data, empty_ok,
    json_ok,
};

pub fn router() -> Router {
    Router::with_path("v2").push(
        Router::with_path("users/{user_id}/devices/{device_id}")
            .get(get_device)
            .put(put_device)
            .delete(delete_device),
    )
}

#[handler]
pub fn get_device(
    user_id: PathParam<OwnedUserId>,
    device_id: PathParam<OwnedDeviceId>,
) -> JsonResult<Device> {
    let Ok(device) = data::user::device::get_device(&user_id, &device_id) else {
        return Err(MatrixError::not_found("device is not found.").into());
    };
    json_ok(device.into_matrix_device())
}

#[handler]
pub fn put_device(
    user_id: PathParam<OwnedUserId>,
    device_id: PathParam<OwnedDeviceId>,
) -> JsonResult<Device> {
    let Ok(device) = data::user::device::get_device(&user_id, &device_id) else {
        return Err(MatrixError::not_found("device is not found.").into());
    };
    json_ok(device.into_matrix_device())
}

#[handler]
pub fn delete_device(
    user_id: PathParam<OwnedUserId>,
    device_id: PathParam<OwnedDeviceId>,
) -> EmptyResult {
    data::user::device::remove_device(&user_id, &device_id)?;
    empty_ok()
}
