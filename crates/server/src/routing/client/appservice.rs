use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn authed_router() -> Router {
    Router::with_path("appservice/{appservice_id}/ping").post(ping)
}

#[endpoint]
async fn ping(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
