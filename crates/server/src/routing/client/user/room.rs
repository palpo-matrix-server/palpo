use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

#[endpoint]
pub(super) async fn get_mutual_rooms(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
