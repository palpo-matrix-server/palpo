use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("hierarchy/<room_id>").put(tree)
}

#[endpoint]
async fn tree(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
