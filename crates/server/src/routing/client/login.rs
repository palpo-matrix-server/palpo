use salvo::prelude::*;

use crate::core::client::account::IdentityServerInfo;
use crate::{empty_ok, hoops, json_ok, AuthArgs, DepotExt, EmptyObject, EmptyResult, JsonResult};

pub fn public_router() -> Router {
    Router::with_path("login/sso/redirect")
        .get(redirect)
        .push(Router::with_path("idpId").get(provider_url))
}

#[endpoint]
async fn redirect(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn provider_url(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
