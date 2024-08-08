//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

mod third_party;
mod transaction;

use salvo::prelude::*;

use crate::exts::*;
use crate::{empty_ok, json_ok, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("appservice").oapi_tag("appservice").push(
        Router::with_path("v2")
            .push(Router::with_path("ping").post(ping))
            .push(Router::with_path("rooms/<room_alias>").get(query_rooms))
            .push(Router::with_path("users/<user_id>").get(query_users))
            .push(third_party::router())
            .push(transaction::router()),
    )
}

#[endpoint]
async fn ping(depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let authed = depot.authed_info()?;
    empty_ok()
}
#[endpoint]
async fn query_rooms(depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let authed = depot.authed_info()?;
    empty_ok()
}
#[endpoint]
async fn query_users(depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let authed = depot.authed_info()?;
    empty_ok()
}
