use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

pub fn authed_router() -> Router {
    Router::with_path("appservice/<appservice_id>/ping").post(ping)
}

#[endpoint]
async fn ping(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
