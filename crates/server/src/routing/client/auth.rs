use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn authed_router() -> Router {
    Router::with_path("auth/{auth_type}/fallback/web").get(uiaa_fallback)
}

#[endpoint]
async fn uiaa_fallback(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
