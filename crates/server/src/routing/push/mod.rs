//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/

use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, DepotExt, EmptyResult};

pub fn router() -> Router {
    Router::with_path("push")
        .oapi_tag("push gateway")
        .push(Router::with_path("v1/notify").post(notify))
}

#[endpoint]
async fn notify(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: notify
    let _authed = depot.authed_info()?;
    empty_ok()
}
