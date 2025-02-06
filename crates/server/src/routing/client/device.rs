//! `POST /_matrix/client/*/register`
//!
//! Register an account on this homeserver.
//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3register

use diesel::prelude::*;
use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::client::device::{
    DeleteDeviceReqBody, DeleteDevicesReqBody, DeviceResBody, DevicesResBody, UpdatedDeviceReqBody,
};
use crate::core::client::uiaa::{AuthFlow, AuthType, UiaaInfo};
use crate::core::error::ErrorKind;
use crate::core::OwnedDeviceId;
use crate::schema::*;
use crate::user::DbUserDevice;
use crate::{db, empty_ok, json_ok, utils, AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, SESSION_ID_LENGTH};

pub fn authed_router() -> Router {
    Router::with_path("devices")
        .get(list_devices)
        // .push(
        //     Router::with_hoop(hoops::limit_rate)
        //         .push(Router::new().post(register).push(Router::with_path("available").get(available)))
        //         .push(Router::with_path("m.login.registration_token/validity").get(validate_token)),
        // )
        .push(Router::with_path("delete_devices").post(delete_devices))
        .push(
            Router::with_path("{device_id}")
                .get(get_device)
                .delete(delete_device)
                .put(update_device),
        )
}

/// #GET /_matrix/client/r0/devices/{device_id}
/// Get metadata on a single device of the sender user.
#[endpoint]
async fn get_device(
    _aa: AuthArgs,
    device_id: PathParam<OwnedDeviceId>,
    depot: &mut Depot,
) -> JsonResult<DeviceResBody> {
    let authed = depot.authed_info()?;

    let device_id = device_id.into_inner();
    json_ok(DeviceResBody(
        crate::user::get_device(authed.user_id(), &device_id)?.into_matrix_device(),
    ))
}

/// #GET /_matrix/client/r0/devices
/// Get metadata on all devices of the sender user.
#[endpoint]
async fn list_devices(_aa: AuthArgs, depot: &mut Depot) -> JsonResult<DevicesResBody> {
    let authed = depot.authed_info()?;

    let devices = user_devices::table
        .filter(user_devices::user_id.eq(authed.user_id()))
        .load::<DbUserDevice>(&mut *db::connect()?)?;
    json_ok(DevicesResBody {
        devices: devices.into_iter().map(DbUserDevice::into_matrix_device).collect(),
    })
}

/// #PUT /_matrix/client/r0/devices/{device_id}
/// Updates the metadata on a given device of the sender user.
#[endpoint]
fn update_device(
    _aa: AuthArgs,
    device_id: PathParam<OwnedDeviceId>,
    body: JsonBody<UpdatedDeviceReqBody>,
) -> EmptyResult {
    let device_id = device_id.into_inner();
    let device = user_devices::table
        .filter(user_devices::device_id.eq(&device_id))
        .first::<DbUserDevice>(&mut *db::connect()?)?;

    diesel::update(&device)
        .set(user_devices::display_name.eq(&body.display_name))
        .execute(&mut *db::connect()?)?;

    empty_ok()
}

/// #DELETE /_matrix/client/r0/devices/{deviceId}
/// Deletes the given device.
///
/// - Requires UIAA to verify user password
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip, last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
#[endpoint]
async fn delete_device(
    _aa: AuthArgs,
    device_id: PathParam<OwnedDeviceId>,
    body: JsonBody<Option<DeleteDeviceReqBody>>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;
    let auth = body.into_inner().map(|body| body.auth).flatten();
    let device_id = device_id.into_inner();

    // UIAA
    let mut uiaa_info = UiaaInfo {
        flows: vec![AuthFlow {
            stages: vec![AuthType::Password],
        }],
        completed: Vec::new(),
        params: Default::default(),
        session: None,
        auth_error: None,
    };
    let Some(auth) = auth else {
        uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
        return Err(uiaa_info.into());
    };

    if let Err(e) = crate::uiaa::try_auth(authed.user_id(), authed.device_id(), &auth, &uiaa_info) {
        if let AppError::Matrix(e) = e {
            if e.kind == ErrorKind::Forbidden {
                return Err(e.into());
            }
        }
        uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
        return Err(uiaa_info.into());
    }
    crate::user::remove_device(authed.user_id(), &device_id)?;
    empty_ok()
}

/// #PUT /_matrix/client/r0/devices/{deviceId}
/// Deletes the given device.
///
/// - Requires UIAA to verify user password
///
/// For each device:
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip, last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
#[endpoint]
async fn delete_devices(_aa: AuthArgs, body: JsonBody<DeleteDevicesReqBody>, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;
    let DeleteDevicesReqBody { devices, auth } = body.into_inner();

    // UIAA
    let uiaa_info = UiaaInfo {
        flows: vec![AuthFlow {
            stages: vec![AuthType::Password],
        }],
        completed: Vec::new(),
        params: Default::default(),
        session: None,
        auth_error: None,
    };
    let Some(auth) = auth else {
        return Err(uiaa_info.into());
    };

    crate::uiaa::try_auth(authed.user_id(), authed.device_id(), &auth, &uiaa_info)?;
    diesel::delete(
        user_devices::table
            .filter(user_devices::user_id.eq(authed.device_id()))
            .filter(user_devices::device_id.eq_any(&devices)),
    )
    .execute(&mut *db::connect()?)?;

    empty_ok()
}

#[endpoint]
pub(super) async fn dehydrated(_aa: AuthArgs) -> EmptyResult {
    //TODO: Later
    empty_ok()
}

#[endpoint]
pub(super) async fn delete_dehydrated(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;
    crate::user::delete_dehydrated_devices(authed.user_id())?;
    empty_ok()
}

#[endpoint]
pub(super) async fn upsert_dehydrated(_aa: AuthArgs) -> EmptyResult {
    //TODO: Later
    empty_ok()
}
