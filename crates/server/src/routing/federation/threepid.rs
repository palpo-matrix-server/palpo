use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("3pid/onbind").put(on_bind)
}

#[endpoint]
async fn on_bind(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
