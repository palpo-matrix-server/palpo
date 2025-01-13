//! `POST /_matrix/client/*/register`
//!
//! Register an account on this homeserver.
//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3register
use std::time::Duration;

use salvo::oapi::extract::JsonBody;
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::presence::{PresenceResBody, SetPresenceReqBody};
use crate::core::OwnedUserId;
use crate::user::NewDbPresence;
use crate::{empty_ok, hoops, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

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
    let authed = depot.authed_info()?;
    let user_id = user_id.into_inner();

    let mut presence = crate::user::get_last_presence(&user_id)?;
    // for room_id in crate::room::user::get_shared_rooms(vec![authed.user.id.clone(), user_id.clone()])? {
    //     if let Some(last_presence) = crate::user::get_last_presence_in_room(&user_id, &room_id)? {
    //         presence = Some(last_presence);
    //         break;
    //     }
    // }
    // if presence.is_none() {
    //     presence = crate::user::get_last_presence(&user_id)?;
    // }

    if let Some(presence) = presence {
        json_ok(PresenceResBody {
            // TODO: Should just use the presenceeventcontent type here?
            status_msg: presence.status_msg,
            currently_active: presence.currently_active,
            last_active_ago: presence.last_active_at.map(|millis| Duration::from_millis(millis.0)),
            presence: presence.state.map(Into::into).unwrap_or_default(),
        })
    } else {
        Err(MatrixError::not_found("Presence state for this user was not found").into())
    }
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
    // for room_id in crate::user::joined_rooms(authed.user_id(), 0)? {
    //     crate::user::set_presence(NewDbPresence {
    //         user_id: authed.user_id().to_owned(),
    //         room_id: Some(room_id),
    //         stream_id: None,
    //         state: Some(body.presence.to_string()),
    //         status_msg: body.status_msg.clone(),
    //         last_active_at: None,
    //         last_federation_update_at: None,
    //         last_user_sync_at: None,
    //         currently_active: None, //TODO
    //     })?;
    // }
    crate::user::set_presence(
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
