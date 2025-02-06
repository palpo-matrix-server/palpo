use salvo::prelude::*;

use crate::{empty_ok, AuthArgs, EmptyResult};

pub fn router() -> Router {
    Router::with_path("hierarchy/{room_id}").put(tree)
}

#[endpoint]
async fn tree(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
