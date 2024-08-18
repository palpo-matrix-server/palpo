use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

#[endpoint]
pub(super) async fn request_token(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
