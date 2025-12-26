use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::Deserialize;

use crate::core::client::device::Device;
use crate::core::identifiers::*;
use crate::{EmptyResult, JsonResult, MatrixError, data, empty_ok, json_ok};

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDeviceReqBody {
    #[serde(default)]
    pub display_name: Option<String>,
}

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
    body: JsonBody<UpdateDeviceReqBody>,
) -> JsonResult<Device> {
    let body = body.into_inner();
    let update = data::user::device::DeviceUpdate {
        display_name: Some(body.display_name),
        user_agent: None,
        last_seen_ip: None,
        last_seen_at: None,
    };
    let Ok(device) = data::user::device::update_device(&user_id, &device_id, update) else {
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
