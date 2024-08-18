use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

pub fn router() -> Router {
    Router::with_path("thirdparty")
        .push(Router::with_path("protocol/<protocol>").get(protocol))
        .push(
            Router::with_path("location")
                .get(locations)
                .push(Router::with_path("<protocol>").get(protocol_locations)),
        )
        .push(
            Router::with_path("user")
                .get(users)
                .push(Router::with_path("<protocol>").get(protocol_users)),
        )
}

#[endpoint]
async fn protocols(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
async fn protocol(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
async fn locations(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn protocol_locations(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn users(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

#[endpoint]
async fn protocol_users(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
