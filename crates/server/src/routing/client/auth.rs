use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, exts::*, json_ok, AuthArgs, AuthedInfo, EmptyResult, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("auth/<auth_type>/fallback/web").get(uiaa_fallback)
}

#[endpoint]
async fn uiaa_fallback(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
