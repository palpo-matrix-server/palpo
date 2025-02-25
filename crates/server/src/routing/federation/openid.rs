use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn router() -> Router {
    Router::with_path("openid/userinfo").get(userinfo)
}

#[endpoint]
async fn userinfo(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
