use palpo_core::JsonValue;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::core::client::account::data::GlobalDataResBody;
use crate::core::events::{AnyGlobalAccountDataEvent, AnyGlobalAccountDataEventContent};
use crate::core::http::UserEventTypeReqArgs;
use crate::core::serde::RawJson;
use crate::{empty_ok, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

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
