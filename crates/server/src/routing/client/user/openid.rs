use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::core::client::account::IdentityServerInfo;
use crate::{empty_ok, hoops, json_ok, AuthArgs, AuthedInfo, DepotExt, EmptyResult, JsonResult};

#[endpoint]
pub(super) async fn request_token(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
