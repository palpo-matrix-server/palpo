use palpo_core::JsonValue;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::value::to_raw_value as to_raw_json_value;

use crate::core::client::account::data::{GlobalDataResBody, SetGlobalDataReqBody};
use crate::core::client::account::IdentityServerInfo;
use crate::core::events::{AnyGlobalAccountDataEvent, AnyGlobalAccountDataEventContent};
use crate::core::http::UserEventTypeReqArgs;
use crate::core::serde::{RawJson, RawJsonValue};
use crate::{
    db, empty_ok, hoops, json_ok, AppError, AppResult, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult,
    MatrixError,
};

#[derive(Deserialize)]
struct ExtractGlobalEventContent {
    content: RawJson<AnyGlobalAccountDataEventContent>,
}

// #GET /_matrix/client/r0/user/{user_id}/account_data/{event_type}
/// Gets some account data for the sender user.
#[endpoint]
pub(super) async fn get_data(
    _aa: AuthArgs,
    args: UserEventTypeReqArgs,
    depot: &mut Depot,
) -> JsonResult<GlobalDataResBody> {
    let authed = depot.authed_info()?;

    let account_data =
        crate::user::get_data::<AnyGlobalAccountDataEvent>(authed.user_id(), None, &args.event_type.to_string())?
            .ok_or(MatrixError::not_found("User data not found."))?;

    json_ok(GlobalDataResBody {
        account_data: RawJson::new(&account_data.content())?,
    })
}

// #PUT /_matrix/client/r0/user/{user_id}/account_data/{event_type}
/// Sets some account data for the sender user.
#[endpoint]
pub(super) async fn set_data(
    _aa: AuthArgs,
    args: UserEventTypeReqArgs,
    body: JsonBody<JsonValue>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let event_type = args.event_type.to_string();

    crate::user::set_data(
        authed.user_id(),
        None,
        &event_type,
        json!({
            "type": event_type,
            "content": body.into_inner(),
        }),
    )?;
    empty_ok()
}
