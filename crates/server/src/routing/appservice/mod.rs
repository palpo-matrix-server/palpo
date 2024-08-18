//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/
mod third_party;
mod transaction;

use salvo::prelude::*;

use crate::{empty_ok, EmptyResult};

pub fn router() -> Router {
    Router::with_path("app").oapi_tag("appservice").push(
        Router::with_path("v1")
            .push(Router::with_path("ping").post(ping))
            .push(Router::with_path("rooms/<room_alias>").get(query_rooms))
            .push(Router::with_path("users/<user_id>").get(query_users))
            .push(third_party::router())
            .push(transaction::router()),
    )
}

#[endpoint]
async fn ping() -> EmptyResult {
    // TODO: ping
    // let _authed = depot.authed_info()?;
    empty_ok()
}
#[endpoint]
async fn query_rooms() -> EmptyResult {
    // TODO: query_rooms
    // let _authed = depot.authed_info()?;
    empty_ok()
}
#[endpoint]
async fn query_users() -> EmptyResult {
    // TODO: query_users
    // let _authed = depot.authed_info()?;
    empty_ok()
}
