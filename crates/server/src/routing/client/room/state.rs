use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::client::room::ReportContentReqBody;
use crate::core::client::state::{
    SendStateEventReqBody, SendStateEventResBody, StateEventsForEmptyKeyReqArgs, StateEventsForKeyReqArgs,
    StateEventsForKeyResBody, StateEventsResBody,
};
use crate::core::client::typing::{CreateTypingEventReqBody, Typing};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::identifiers::*;
use crate::core::room::{RoomEventReqArgs, RoomEventTypeReqArgs, RoomTypingReqArgs};
use crate::room::state;
use crate::utils::HtmlEscape;
use crate::{AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, empty_ok, json_ok};

/// #GET /_matrix/client/r0/rooms/{room_id}/state
/// Get all state events for a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) fn get_state(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    depot: &mut Depot,
) -> JsonResult<StateEventsResBody> {
    let authed = depot.authed_info()?;
    let room_id = room_id.into_inner();

    if !state::user_can_see_state_events(&authed.user_id(), &room_id)? {
        return Err(MatrixError::forbidden(None, "You don't have permission to view this room.").into());
    }

    let frame_id = state::get_room_frame_id(&room_id, None)?;

    let room_state = state::get_full_state(frame_id)?
        .values()
        .map(|pdu| pdu.to_state_event())
        .collect();
    json_ok(StateEventsResBody::new(room_state))
}
/// #POST /_matrix/client/r0/rooms/{room_id}/report/{event_id}
/// Reports an inappropriate event to homeserver admins
#[endpoint]
pub fn report(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<ReportContentReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let pdu = match crate::room::timeline::get_pdu(&args.event_id) {
        Ok(pdu) => pdu,
        _ => return Err(MatrixError::invalid_param("Invalid Event ID").into()),
    };

    if let Some(true) = body.score.map(|s| s > 0 || s < -100) {
        return Err(MatrixError::invalid_param("Invalid score, must be within 0 to -100").into());
    };

    if let Some(true) = body.reason.clone().map(|s| s.chars().count() > 250) {
        return Err(MatrixError::invalid_param("Reason too long, should be 250 characters or fewer").into());
    };

    crate::admin::send_message(RoomMessageEventContent::text_html(
        format!(
            "Report received from: {}\n\n\
                Event ID: {:?}\n\
                Room ID: {:?}\n\
                Sent By: {:?}\n\n\
                Report Score: {:?}\n\
                Report Reason: {:?}",
            authed.user_id(),
            pdu.event_id,
            pdu.room_id,
            pdu.sender,
            body.score,
            body.reason
        ),
        format!(
            "<details><summary>Report received from: <a href=\"https://matrix.to/#/{0:?}\">{0:?}\
                </a></summary><ul><li>Event Info<ul><li>Event ID: <code>{1:?}</code>\
                <a href=\"https://matrix.to/#/{2:?}/{1:?}\">🔗</a></li><li>Room ID: <code>{2:?}</code>\
                </li><li>Sent By: <a href=\"https://matrix.to/#/{3:?}\">{3:?}</a></li></ul></li><li>\
                Report Info<ul><li>Report Score: {4:?}</li><li>Report Reason: {5}</li></ul></li>\
                </ul></details>",
            authed.user_id(),
            pdu.event_id,
            pdu.room_id,
            pdu.sender,
            body.score,
            HtmlEscape(body.reason.as_deref().unwrap_or(""))
        ),
    ));
    empty_ok()
}
/// #GET /_matrix/client/r0/rooms/{room_id}/state/{event_type}/{state_key}
/// Get single state event of a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) fn state_for_key(
    _aa: AuthArgs,
    args: StateEventsForKeyReqArgs,
    depot: &mut Depot,
) -> JsonResult<StateEventsForKeyResBody> {
    let authed = depot.authed_info()?;
    if !state::user_can_see_state_events(&authed.user_id(), &args.room_id)? {
        return Err(MatrixError::forbidden(None, "You don't have permission to view this room.").into());
    }

    let event = state::get_room_state(&args.room_id, &args.event_type, &args.state_key, None)?;

    let event_format = args.format.as_ref().is_some_and(|f| f.to_lowercase().eq("event"));
    json_ok(StateEventsForKeyResBody {
        content: Some(event.get_content()?),
        event: if event_format {
            Some(event.to_state_event_value())
        } else {
            None
        },
    })
}

/// #GET /_matrix/client/r0/rooms/{room_id}/state/{event_type}
/// Get single state event of a room.
///
/// - If not joined: Only works if current room history visibility is world readable
#[endpoint]
pub(super) async fn state_for_empty_key(
    _aa: AuthArgs,
    args: StateEventsForEmptyKeyReqArgs,
    depot: &mut Depot,
) -> JsonResult<StateEventsForKeyResBody> {
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let event = if !state::user_can_see_state_events(sender_id, &args.room_id)? {
        if let Ok(leave_sn) = crate::room::user::leave_sn(sender_id, &args.room_id) {
            state::get_room_state(&args.room_id, &args.event_type, "", Some(leave_sn))?
        } else {
            return Err(MatrixError::forbidden(None, "You don't have permission to view this room.").into());
        }
    } else {
        state::get_room_state(&args.room_id, &args.event_type, "", None)?
    };

    let event_format = args.format.as_ref().is_some_and(|f| f.to_lowercase().eq("event"));
    json_ok(StateEventsForKeyResBody {
        content: Some(event.get_content()?),
        event: if event_format {
            Some(event.to_state_event_value())
        } else {
            None
        },
    })
}

/// #PUT /_matrix/client/r0/rooms/{room_id}/state/{event_type}/{state_key}
/// Sends a state event into the room.
///
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
/// - If event is new canonical_alias: Rejects if alias is incorrect
#[endpoint]
pub(super) async fn send_state_for_key(
    _aa: AuthArgs,
    args: StateEventsForKeyReqArgs,
    body: JsonBody<SendStateEventReqBody>,
    depot: &mut Depot,
) -> JsonResult<SendStateEventResBody> {
    let authed = depot.authed_info()?;
    let body = body.into_inner();

    let event_id = crate::state::send_state_event_for_key(
        authed.user_id(),
        &args.room_id,
        &args.event_type,
        body.0,
        args.state_key.to_owned(),
    )
    .await?;

    let event_id = (*event_id).to_owned();
    json_ok(SendStateEventResBody { event_id })
}

/// #PUT /_matrix/client/r0/rooms/{room_id}/state/{event_type}
/// Sends a state event into the room.
///
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
/// - If event is new canonical_alias: Rejects if alias is incorrect
#[endpoint]
pub(super) async fn send_state_for_empty_key(
    _aa: AuthArgs,
    args: RoomEventTypeReqArgs,
    body: JsonBody<SendStateEventReqBody>,
    depot: &mut Depot,
) -> JsonResult<SendStateEventResBody> {
    let authed = depot.authed_info()?;
    let body = body.into_inner();
    let event_id = crate::state::send_state_event_for_key(
        authed.user_id(),
        &args.room_id,
        &args.event_type.to_string().into(),
        body.0,
        "".into(),
    )
    .await?;

    let event_id = (*event_id).to_owned();
    json_ok(SendStateEventResBody { event_id })
}

/// #PUT /_matrix/client/r0/rooms/{room_id}/typing/{user_id}
/// Sets the typing state of the sender user.
#[endpoint]
pub async fn send_typing(
    _aa: AuthArgs,
    args: RoomTypingReqArgs,
    body: JsonBody<CreateTypingEventReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    if !crate::room::is_joined(authed.user_id(), &args.room_id)? {
        return Err(MatrixError::forbidden(None, "You are not in this room.").into());
    }

    if let Typing::Yes(duration) = body.state {
        crate::room::typing::add_typing(
            authed.user_id(),
            &args.room_id,
            duration.as_millis() as u64 + UnixMillis::now().get(),
        )
        .await?;
    } else {
        crate::room::typing::remove_typing(authed.user_id(), &args.room_id).await?;
    }
    empty_ok()
}
#[endpoint]
pub(super) async fn timestamp(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    //TODO:??
    let _authed = depot.authed_info()?;
    empty_ok()
}
