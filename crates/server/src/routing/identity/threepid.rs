use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("3pid")
        .push(Router::with_path("bind").post(bind))
        .push(Router::with_path("unbind").post(unbind))
        .push(Router::with_path("getValidated3pid").get(validated))
}

#[endpoint]
async fn bind(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
async fn unbind(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn validated(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
