use salvo::prelude::*;

use crate::core::third_party::*;
use crate::{AuthArgs, JsonResult, json_ok};

pub fn authed_router() -> Router {
    Router::with_path("thirdparty")
        .push(Router::with_path("protocols").get(protocols))
        .push(Router::with_path("protocol").get(protocol))
        .push(
            Router::with_path("location")
                .get(locations)
                .push(Router::with_path("{protocol}").get(protocol_locations)),
        )
        .push(
            Router::with_path("user")
                .get(users)
                .push(Router::with_path("{protocol}").get(protocol_users)),
        )
}

/// #GET /_matrix/client/r0/thirdparty/protocols
/// TODO: Fetches all metadata about protocols supported by the homeserver.
#[endpoint]
async fn protocols(_aa: AuthArgs) -> JsonResult<ProtocolsResBody> {
    // TODO: LATER
    json_ok(ProtocolsResBody::default())
}
#[endpoint]
async fn protocol(_aa: AuthArgs) -> JsonResult<Option<ProtocolResBody>> {
    // TODO: LATER
    json_ok(None)
}
#[endpoint]
async fn locations(_aa: AuthArgs) -> JsonResult<LocationsResBody> {
    // TODO: LATER
    json_ok(LocationsResBody::default())
}

#[endpoint]
async fn protocol_locations(_aa: AuthArgs) -> JsonResult<LocationsResBody> {
    // TODO: LATER
    json_ok(LocationsResBody::default())
}

#[endpoint]
async fn users(_aa: AuthArgs) -> JsonResult<UsersResBody> {
    // TODO: LATER
    json_ok(UsersResBody::default())
}

#[endpoint]
async fn protocol_users(_aa: AuthArgs) -> JsonResult<UsersResBody> {
    // TODO: LATER
    json_ok(UsersResBody::default())
}
