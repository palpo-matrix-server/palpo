//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use std::fmt;

use salvo::prelude::*;

use crate::exts::*;
use crate::{db, empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("push")
        .oapi_tag("push gateway")
        .push(Router::with_path("v1/notify").post(notify))
}

#[endpoint]
async fn notify(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: notify
    let authed = depot.authed_info()?;
    empty_ok()
}
