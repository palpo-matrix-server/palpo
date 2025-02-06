use crate::core::third_party::*;
use crate::{json_ok, AuthArgs, JsonResult};
use salvo::prelude::*;

pub fn router() -> Router {
    Router::with_path("thirdparty")
        .push(Router::with_path("protocol/{protocol}").get(protocol))
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
async fn users(_aa: AuthArgs, req: &mut Request) -> JsonResult<UsersResBody> {
    // TODO: LATER
    json_ok(UsersResBody::default())
}

#[endpoint]
async fn protocol_users(_aa: AuthArgs, req: &mut Request) -> JsonResult<UsersResBody> {
    // TODO: LATER
    json_ok(UsersResBody::default())
}
