//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use std::fmt;

mod backfill;
mod event;
pub(super) mod key;
mod membership;
mod openid;
mod query;
mod room;
mod space;
mod threepid;
mod transaction;
mod user;

use salvo::prelude::*;

use crate::core::directory::Server;
use crate::core::federation::directory::ServerVersionResBody;
use crate::{empty_ok, hoops, json_ok, AppError, AppResult, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("federation")
        .hoop(check_federation_enabled)
        .oapi_tag("federation")
        .push(
            Router::with_path("v1")
                .push(backfill::router())
                .push(event::router())
                .push(membership::router_v1())
                .push(openid::router())
                .push(query::router())
                .push(room::router())
                .push(space::router())
                .push(threepid::router())
                .push(transaction::router())
                .push(user::router())
                .push(Router::with_path("version").post(version)),
        )
        .push(
            Router::with_path("v2")
                .push(backfill::router())
                .push(event::router())
                .push(membership::router_v2())
                .push(openid::router())
                .push(query::router())
                .push(room::router())
                .push(space::router())
                .push(threepid::router())
                .push(transaction::router())
                .push(user::router())
                .push(Router::with_path("version").post(version)),
        )
        .push(Router::with_path("versions").get(get_versions))
}

#[endpoint]
async fn check_federation_enabled() -> EmptyResult {
    if !crate::allow_federation() {
        return Err(AppError::public("Federation is disabled."));
    }
    empty_ok()
}

#[endpoint]
async fn get_versions(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    //TODO: https://github.com/matrix-org/matrix-spec-proposals/pull/3723
    empty_ok()
}
// #GET /_matrix/federation/v1/version
/// Get version information on this server.
#[endpoint]
async fn version(depot: &mut Depot) -> JsonResult<ServerVersionResBody> {
    json_ok(ServerVersionResBody {
        server: Some(Server {
            name: Some("Palpus".to_owned()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
    })
}

#[endpoint]
async fn notifications(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    panic!("notifications Not implemented")
}

#[endpoint]
async fn sync(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    panic!("syncNot implemented")
}
