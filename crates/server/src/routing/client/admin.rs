use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::core::client::account::IdentityServerInfo;
use crate::{empty_ok, exts::*, json_ok, AuthArgs, AuthedInfo, EmptyResult, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("admin/whois/<user_id>").get(whois)
}

#[endpoint]
async fn whois(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
