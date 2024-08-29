use crate::core::third_party::*;
use crate::{empty_ok, json_ok, AuthArgs, EmptyResult, JsonResult};
use palpo_core::directory::RoomTypeFilter::Default;
use salvo::prelude::*;

use crate::core::third_party::*;


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
async fn protocols(_aa: AuthArgs) -> JsonResult<ProtocolsResBody> {
    // TODDO: LATER
    json_ok(ProtocolsResBody::default())
}
#[endpoint]
async fn protocol(_aa: AuthArgs) -> JsonResult<Option<ProtocolResBody>> {
    // TODDO: LATER
    json_ok(None)
}
#[endpoint]
async fn locations(_aa: AuthArgs) -> JsonResult<LocationsResBody> {
    // TODDO: LATER
    json_ok(LocationsResBody::default())
}

#[endpoint]
async fn protocol_locations(_aa: AuthArgs) -> JsonResult<LocationsResBody> {
    // TODDO: LATER
    json_ok(LocationsResBody::default())
}

#[endpoint]
async fn users(_aa: AuthArgs, req: &mut Request) -> JsonResult<UsersResBody> {
    // TODDO: LATER
    json_ok(UsersResBody::default())
}

#[endpoint]
async fn protocol_users(_aa: AuthArgs, req: &mut Request) -> JsonResult<UsersResBody> {
    // TODDO: LATER
    json_ok(UsersResBody::default())
}
