use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn authed_router() -> Router {
    Router::with_path("admin/whois/{user_id}").get(whois)
}

#[endpoint]
async fn whois(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
