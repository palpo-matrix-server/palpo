//! (De)serializable types for the [Matrix Server-Server API][federation-api].
//! These types are used by server code.
//!
//! [federation-api]: https://spec.matrix.org/latest/server-server-api/

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

use std::fmt;

mod pubkey;
mod threepid;
mod validate;

use salvo::prelude::*;

use crate::core::client::key::UploadSignaturesResBody;
use crate::exts::*;
use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("identity")
        .oapi_tag("identity")
        .push(
            Router::with_path("v2")
                .get(status)
                .push(validate::router())
                .push(pubkey::router())
                .push(threepid::router())
                .push(Router::with_path("account").post(account))
                .push(Router::with_path("lookup").post(lookup))
                .push(Router::with_path("store-invite").post(store_invite))
                .push(Router::with_path("hash_details").post(hash_details))
                .push(Router::with_path("sign-ed25519").post(sign_ed25519))
                .push(Router::with_path("terms").get(terms).post(accept_terms)),
        )
        .push(Router::with_path("versions").get(versions))
}

#[endpoint]
async fn status(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}
#[endpoint]
async fn versions(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}
#[endpoint]
async fn account(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}

#[endpoint]
async fn lookup(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let _authed = depot.authed_info()?;
    empty_ok()
}

#[endpoint]
async fn store_invite(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    empty_ok()
}

#[endpoint]
async fn hash_details(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let _authed = depot.authed_info()?;
    empty_ok()
}

#[endpoint]
async fn sign_ed25519(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    //TODO: LATER
    let _authed = depot.authed_info()?;
    empty_ok()
}

#[endpoint]
async fn accept_terms(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let _authed = depot.authed_info()?;
    empty_ok()
}

#[endpoint]
async fn terms(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: fixme
    let _authed = depot.authed_info()?;
    empty_ok()
}
