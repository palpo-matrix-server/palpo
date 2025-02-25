use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};

pub fn router() -> Router {
    Router::with_path("3pid/onbind").put(on_bind)
}

#[endpoint]
async fn on_bind(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
