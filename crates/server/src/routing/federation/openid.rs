use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

pub fn router() -> Router {
    Router::with_path("openid/userinfo").get(userinfo)
}

#[endpoint]
async fn userinfo(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
