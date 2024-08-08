use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::core::client::account::IdentityServerInfo;
use crate::{empty_ok, json_ok, AuthArgs, AuthedInfo, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("openid/userinfo").get(userinfo)
}

#[endpoint]
async fn userinfo(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
