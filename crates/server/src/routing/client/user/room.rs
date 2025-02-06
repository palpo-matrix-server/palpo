use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

#[endpoint]
pub(super) async fn get_mutual_rooms(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
