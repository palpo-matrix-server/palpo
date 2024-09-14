use palpo_core::JsonValue;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::Deserialize;

use crate::core::client::account::data::{GlobalAccountDataResBody, RoomAccountDataResBody};
use crate::core::events::AnyGlobalAccountDataEventContent;
use crate::core::serde::RawJson;
use crate::core::user::{UserEventTypeReqArgs, UserRoomEventTypeReqArgs};
use crate::{empty_ok, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

#[derive(Deserialize)]
struct ExtractGlobalEventContent {
    content: RawJson<AnyGlobalAccountDataEventContent>,
}

// #GET /_matrix/client/r0/user/{user_id}/account_data/{event_type}
/// Gets some account data for the sender user.
#[endpoint]
pub(super) async fn get_global_data(
    _aa: AuthArgs,
    args: UserEventTypeReqArgs,
    depot: &mut Depot,
) -> JsonResult<GlobalAccountDataResBody> {
    let authed = depot.authed_info()?;

    let content = crate::user::get_data::<JsonValue>(authed.user_id(), None, &args.event_type.to_string())?
        .ok_or(MatrixError::not_found("User data not found."))?;

    json_ok(GlobalAccountDataResBody(RawJson::from_value(&content)?))
}

// #PUT /_matrix/client/r0/user/{user_id}/account_data/{event_type}
/// Sets some account data for the sender user.
#[endpoint]
pub(super) async fn set_global_data(
    _aa: AuthArgs,
    args: UserEventTypeReqArgs,
    body: JsonBody<JsonValue>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let event_type = args.event_type.to_string();

    crate::user::set_data(authed.user_id(), None, &event_type, body.into_inner())?;
    empty_ok()
}

// #GET /_matrix/client/r0/user/{user_id}/rooms/{roomId}/account_data/{event_type}
/// Gets some account data for the sender user.
#[endpoint]
pub(super) async fn get_room_data(
    _aa: AuthArgs,
    args: UserRoomEventTypeReqArgs,
    depot: &mut Depot,
) -> JsonResult<RoomAccountDataResBody> {
    let authed = depot.authed_info()?;

    let content =
        crate::user::get_data::<JsonValue>(authed.user_id(), Some(&*args.room_id), &args.event_type.to_string())?
            .ok_or(MatrixError::not_found("User data not found."))?;

    json_ok(RoomAccountDataResBody(RawJson::from_value(&content)?))
}

// #PUT /_matrix/client/r0/user/{user_id}/account_data/{event_type}
/// Sets some room account data for the sender user.
#[endpoint]
pub(super) async fn set_room_data(
    _aa: AuthArgs,
    args: UserRoomEventTypeReqArgs,
    body: JsonBody<JsonValue>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let event_type = args.event_type.to_string();

    crate::user::set_data(authed.user_id(), Some(args.room_id), &event_type, body.into_inner())?;
    empty_ok()
}
