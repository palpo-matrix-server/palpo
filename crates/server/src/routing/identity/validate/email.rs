use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

#[endpoint]
pub(super) async fn create_session(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}

#[endpoint]
pub(super) async fn validate(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
#[endpoint]
pub(super) async fn validate_by_end_user(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
