//! `POST /_matrix/client/*/register`
//!
//! Register an account on this homeserver.
//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3register
use std::str::FromStr;

use salvo::oapi::extract::JsonBody;
use salvo::oapi::ToParameters;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::client::account::{IdentityServerInfo, LoginType, RegistrationKind};
use crate::core::client::push::pusher::PushersResBody;
use crate::core::client::push::SetPusherReqBody;
use crate::core::client::register::*;
use crate::core::client::uiaa::{AuthData, AuthFlow, AuthType, UiaaInfo};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::identifiers::*;
use crate::core::push::Pusher;
use crate::core::{push, OwnedDeviceId, OwnedUserId, UserId};
use crate::{
    config, db, empty_ok, hoops, json_ok, utils, AppError, AppResult, AuthArgs, AuthedInfo, DepotExt, EmptyResult,
    JsonResult, DEVICE_ID_LENGTH, RANDOM_USER_ID_LENGTH, SESSION_ID_LENGTH, TOKEN_LENGTH,
};

pub fn authed_router() -> Router {
    Router::with_path("pushers")
        .get(pushers)
        .push(Router::with_hoop(hoops::limit_rate).push(Router::with_path("set").post(set_pusher)))
}

// #GET /_matrix/client/r0/pushers
/// Gets all currently active pushers for the sender user.
#[endpoint]
async fn pushers(depot: &mut Depot) -> JsonResult<PushersResBody> {
    let authed = depot.authed_info()?;

    json_ok(PushersResBody {
        pushers: crate::user::pusher::get_pushers(authed.user_id())?
            .into_iter()
            .map(TryInto::<Pusher>::try_into)
            .collect::<Result<Vec<_>, AppError>>()?,
    })
}

// #POST /_matrix/client/r0/pushers/set
/// Adds a pusher for the sender user.
///
/// - TODO: Handle `append`
#[endpoint]
async fn set_pusher(body: JsonBody<SetPusherReqBody>, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;

    crate::user::pusher::set_pusher(authed.user_id(), body.action.clone())?;

    empty_ok()
}
