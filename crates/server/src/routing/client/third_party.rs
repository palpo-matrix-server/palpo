use salvo::prelude::*;

use crate::core::client::third_party::ProtocolsResBody;
use crate::core::client::uiaa::AuthData;
use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("thirdparty")
        .push(Router::with_path("protocols").get(protocols))
        .push(Router::with_path("protocol").get(protocol))
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

// #GET /_matrix/client/r0/thirdparty/protocols
/// TODO: Fetches all metadata about protocols supported by the homeserver.
#[endpoint]
async fn protocols(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
#[endpoint]
async fn protocol(_aa: AuthArgs) -> JsonResult<ProtocolsResBody> {
    // TODDO: todo
    json_ok(ProtocolsResBody::default())
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
