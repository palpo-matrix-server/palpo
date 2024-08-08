use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

#[endpoint]
pub(super) async fn create_session(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
pub(super) async fn validate(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
pub(super) async fn validate_by_phone_number(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
