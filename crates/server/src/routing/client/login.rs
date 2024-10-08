use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

pub fn public_router() -> Router {
    Router::with_path("login/sso/redirect")
        .get(redirect)
        .push(Router::with_path("idpId").get(provider_url))
}

#[endpoint]
async fn redirect(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn provider_url(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
