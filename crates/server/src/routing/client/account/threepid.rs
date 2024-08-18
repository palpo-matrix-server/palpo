//! `POST /_matrix/client/*/account/3pid/add`
//!
//! Add contact information to a user's account

//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3account3pidadd

use salvo::prelude::*;

use crate::core::client::account::threepid::ThreepidsResBody;
use crate::{empty_ok, json_ok, AuthArgs, EmptyResult, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("3pid")
        .get(get)
        // 1.0 => "/_matrix/client/r0/account/3pid/add",
        // 1.1 => "/_matrix/client/v3/account/3pid/add",
        // 1.0 => "/_matrix/client/r0/account/3pid/bind",
        // 1.1 => "/_matrix/client/v3/account/3pid/bind",
        .push(Router::with_path("add").post(add))
        .push(Router::with_path("bind").post(bind))
        .push(Router::with_path("delete").post(delete))
}

// #GET _matrix/client/v3/account/3pid
/// Get a list of third party identifiers associated with this account.
///
/// - Currently always returns empty list
#[endpoint]
async fn get(_aa: AuthArgs) -> JsonResult<ThreepidsResBody> {
    // TODO: fixme
    json_ok(ThreepidsResBody::new(Vec::new()))
}

#[endpoint]
async fn add(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}

#[endpoint]
async fn bind(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}

#[endpoint]
async fn unbind(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}

#[endpoint]
async fn delete(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}
