use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;
use crate::core::presence::PresenceUpdate;

use crate::{empty_ok, json_ok, EmptyResult, JsonResult};
use crate::{exts::*, AuthArgs, AuthedInfo};

pub fn router() -> Router {
    Router::with_path("transactions/<txn_id>").put(send_event)
}

#[endpoint]
async fn send_event(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
