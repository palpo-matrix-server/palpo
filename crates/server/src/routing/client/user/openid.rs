use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

#[endpoint]
pub(super) async fn request_token(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
