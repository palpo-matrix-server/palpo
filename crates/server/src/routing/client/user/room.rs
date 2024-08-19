use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

#[endpoint]
pub(super) async fn get_mutual_rooms(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
