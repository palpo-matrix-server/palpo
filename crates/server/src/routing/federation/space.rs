use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn router() -> Router {
    Router::with_path("hierarchy/{room_id}").put(tree)
}

#[endpoint]
async fn tree(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
