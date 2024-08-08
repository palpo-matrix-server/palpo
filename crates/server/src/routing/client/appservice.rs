use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, AuthArgs, DepotExt, EmptyObject, EmptyResult, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("appservice/<appservice_id>/ping").post(ping)
}

#[endpoint]
async fn ping(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
