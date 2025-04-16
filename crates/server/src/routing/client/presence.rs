use std::time::Duration;

use salvo::oapi::extract::JsonBody;
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::OwnedUserId;
use crate::core::client::presence::{PresenceResBody, SetPresenceReqBody};
use crate::data::user::NewDbPresence;
use crate::{AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, empty_ok, hoops, json_ok};

pub fn authed_router() -> Router {
    Router::with_path("presence/{user_id}/status")
        .get(get_status)
        .push(Router::with_hoop(hoops::limit_rate).put(set_status))
}

/// #GET /_matrix/client/r0/presence/{user_id}/status
/// Gets the presence state of the given user.
///
/// - Only works if you share a room with the user
#[endpoint]
fn get_status(user_id: PathParam<OwnedUserId>, depot: &mut Depot) -> JsonResult<PresenceResBody> {
    if !crate::config().allow_local_presence {
        return Err(MatrixError::forbidden("Presence is disabled on this server").into());
    }

    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let user_id = user_id.into_inner();

    if !crate::room::state::user_can_see_user(sender_id, &user_id)? {
        return Err(MatrixError::unauthorized("You cannot get the presence state of this user").into());
    }

    let content = crate::data::user::last_presence(&user_id)?.content;

    json_ok(PresenceResBody {
        // TODO: Should just use the presenceeventcontent type here?
        status_msg: content.status_msg,
        currently_active: content.currently_active,
        last_active_ago: content.last_active_ago.map(|millis| Duration::from_millis(millis)),
        presence: content.presence,
    })
}

/// #PUT /_matrix/client/r0/presence/{user_id}/status
/// Sets the presence state of the sender user.
#[endpoint]
async fn set_status(
    _aa: AuthArgs,
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<SetPresenceReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    if !crate::allow_local_presence() {
        return Err(MatrixError::forbidden("Presence is disabled on this server").into());
    }

    let authed = depot.authed_info()?;
    let user_id = user_id.into_inner();
    if authed.user_id() != &user_id {
        return Err(MatrixError::forbidden("You cannot set the presence state of another user").into());
    }
    crate::data::user::set_presence(
        NewDbPresence {
            user_id: authed.user_id().to_owned(),
            stream_id: None,
            state: Some(body.presence.to_string()),
            status_msg: body.status_msg.clone(),
            last_active_at: None,
            last_federation_update_at: None,
            last_user_sync_at: None,
            currently_active: None, //TODO,
            occur_sn: None,
        },
        true,
    )?;

    empty_ok()
}
