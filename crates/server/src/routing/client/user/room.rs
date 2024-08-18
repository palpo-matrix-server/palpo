use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::json;

use crate::core::client::account::data::{RoomDataResBody, SetDataInRoomReqBody};
use crate::core::events::AnyRoomAccountDataEvent;
use crate::core::http::RoomEventTypeReqArgs;
use crate::core::serde::RawJson;
use crate::{empty_ok, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

// #GET /_matrix/client/r0/user/{user_id}/rooms/{room_id}/account_data/{type}
/// Gets some room account data for the sender user.
#[endpoint]
pub(super) async fn get_data(
    _aa: AuthArgs,
    args: RoomEventTypeReqArgs,
    depot: &mut Depot,
) -> JsonResult<RoomDataResBody> {
    let authed = depot.authed_info()?;

    let account_data = crate::user::get_data::<AnyRoomAccountDataEvent>(
        authed.user_id(),
        Some(&args.room_id),
        &args.event_type.to_string(),
    )?
    .ok_or(MatrixError::not_found("Data not found."))?;

    json_ok(RoomDataResBody {
        account_data: RawJson::new(&account_data.content())?,
    })
}

// #PUT /_matrix/client/r0/user/{user_id}/rooms/{room_id}/account_data/{event_type}
/// Sets some room account data for the sender user.
#[endpoint]
pub(super) async fn set_data(
    _aa: AuthArgs,
    args: RoomEventTypeReqArgs,
    body: JsonBody<SetDataInRoomReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let content: serde_json::Value =
        serde_json::from_str(body.data.inner().get()).map_err(|_| MatrixError::bad_json("Data is invalid."))?;

    let event_type = args.event_type.to_string();

    crate::user::set_data(
        authed.user_id(),
        Some(args.room_id),
        &event_type,
        json!({
            "type": event_type,
            "content": content,
        }),
    )?;
    empty_ok()
}
#[endpoint]
pub(super) async fn mutual(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
