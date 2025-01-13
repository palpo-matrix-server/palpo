use salvo::prelude::*;

use crate::AuthArgs;
use crate::{empty_ok, EmptyResult};

pub fn router() -> Router {
    Router::with_path("transactions/{txn_id}").put(send_event)
}

#[endpoint]
async fn send_event(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
