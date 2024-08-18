use std::collections::HashSet;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::value::to_raw_value;

use crate::core::client::filter::LazyLoadOptions;
use crate::core::client::redact::{RedactEventReqArgs, RedactEventReqBody, RedactEventResBody};
use crate::core::client::room::{ContextReqArgs, ContextResBody, ReportContentReqBody, RoomEventResBody};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::events::room::redaction::RoomRedactionEventContent;
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::http::RoomEventReqArgs;
use crate::room::state::DbRoomStateField;
use crate::utils::HtmlEscape;
use crate::PduBuilder;
use crate::{empty_ok, json_ok, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError};

// #GET /_matrix/client/r0/rooms/{room_id}/event/{event_id}
/// Gets a single event.
///
/// - You have to currently be joined to the room (TODO: Respect history visibility)
#[endpoint]
pub(super) fn get_room_event(_aa: AuthArgs, args: RoomEventReqArgs, depot: &mut Depot) -> JsonResult<RoomEventResBody> {
    let authed = depot.authed_info()?;

    let event = crate::room::timeline::get_pdu(&args.event_id)?.ok_or_else(|| {
        warn!("Event not found, event ID: {:?}", &args.event_id);
        MatrixError::not_found("Event not found.")
    })?;

    if !crate::room::state::user_can_see_event(authed.user_id(), &event.room_id, &args.event_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view this event.").into());
    }

    let mut event = event.clone();
    event.add_age()?;

    json_ok(RoomEventResBody {
        event: event.to_room_event(),
    })
}

// #POST /_matrix/client/r0/rooms/{room_id}/report/{event_id}
/// Reports an inappropriate event to homeserver admins
#[endpoint]
pub(super) fn report(
    _aa: AuthArgs,
    args: RoomEventReqArgs,
    body: JsonBody<ReportContentReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let pdu = match crate::room::timeline::get_pdu(&args.event_id)? {
        Some(pdu) => pdu,
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

// #GET /_matrix/client/r0/rooms/{room_id}/context/{event_id}
/// Allows loading room history around an event.
///
/// - Only works if the user is joined (TODO: always allow, but only show events if the user was
/// joined, depending on history_visibility)
#[endpoint]
pub(super) fn get_context(_aa: AuthArgs, args: ContextReqArgs, depot: &mut Depot) -> JsonResult<ContextResBody> {
    let authed = depot.authed_info()?;

    let (lazy_load_enabled, lazy_load_send_redundant) = match &args.filter.lazy_load_options {
        LazyLoadOptions::Enabled {
            include_redundant_members,
        } => (true, *include_redundant_members),
        _ => (false, false),
    };

    let mut lazy_loaded = HashSet::new();

    let base_token = crate::room::timeline::get_event_sn(&args.event_id)?
        .ok_or(MatrixError::not_found("Base event id not found."))?;

    let base_event =
        crate::room::timeline::get_pdu(&args.event_id)?.ok_or(MatrixError::not_found("Base event not found."))?;

    let room_id = base_event.room_id.clone();

    if !crate::room::state::user_can_see_event(authed.user_id(), &room_id, &args.event_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view this event.").into());
    }

    if !crate::room::lazy_loading::lazy_load_was_sent_before(
        authed.user_id(),
        authed.device_id(),
        &room_id,
        &base_event.sender,
    )? || lazy_load_send_redundant
    {
        lazy_loaded.insert(base_event.sender.as_str().to_owned());
    }

    // Use limit with maximum 100
    let limit = usize::from(args.limit).min(100);

    let base_event = base_event.to_room_event();

    let events_before: Vec<_> =
        crate::room::timeline::get_pdus_backward(authed.user_id(), &room_id, base_token, limit / 2)?
            .into_iter()
            .filter(|(_, pdu)| {
                crate::room::state::user_can_see_event(authed.user_id(), &room_id, &pdu.event_id).unwrap_or(false)
            })
            .collect();

    for (_, event) in &events_before {
        if !crate::room::lazy_loading::lazy_load_was_sent_before(
            authed.user_id(),
            authed.device_id(),
            &room_id,
            &event.sender,
        )? || lazy_load_send_redundant
        {
            lazy_loaded.insert(event.sender.as_str().to_owned());
        }
    }

    let start_token = events_before
        .last()
        .map(|(count, _)| count.to_string())
        .unwrap_or_else(|| base_token.to_string());

    let events_before: Vec<_> = events_before.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

    let events_after: Vec<_> =
        crate::room::timeline::get_pdus_forward(authed.user_id(), &room_id, base_token, limit / 2)?;

    for (_, event) in &events_after {
        if !crate::room::lazy_loading::lazy_load_was_sent_before(
            authed.user_id(),
            authed.device_id(),
            &room_id,
            &event.sender,
        )? || lazy_load_send_redundant
        {
            lazy_loaded.insert(event.sender.as_str().to_owned());
        }
    }

    let frame_id =
        match crate::room::state::get_pdu_frame_id(events_after.last().map_or(&*args.event_id, |(_, e)| &*e.event_id))?
        {
            Some(s) => s,
            None => crate::room::state::get_room_frame_id(&room_id)?.expect("All rooms have state"),
        };

    let state_ids = crate::room::state::get_full_state_ids(frame_id)?;

    let end_token = events_after
        .last()
        .map(|(count, _)| count.to_string())
        .unwrap_or_else(|| base_token.to_string());

    let events_after: Vec<_> = events_after.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

    let mut state = Vec::new();

    for (field_id, event_id) in state_ids {
        let DbRoomStateField {
            event_type, state_key, ..
        } = crate::room::state::get_field(field_id)?;

        if event_type != StateEventType::RoomMember {
            let pdu = match crate::room::timeline::get_pdu(&event_id)? {
                Some(pdu) => pdu,
                None => {
                    error!("Pdu in state not found: {}", event_id);
                    continue;
                }
            };
            state.push(pdu.to_state_event());
        } else if !lazy_load_enabled || lazy_loaded.contains(&state_key) {
            let pdu = match crate::room::timeline::get_pdu(&event_id)? {
                Some(pdu) => pdu,
                None => {
                    error!("Pdu in state not found: {}", event_id);
                    continue;
                }
            };
            state.push(pdu.to_state_event());
        }
    }

    json_ok(ContextResBody {
        start: Some(start_token),
        end: Some(end_token),
        events_before,
        event: Some(base_event),
        events_after,
        state,
    })
}

// #PUT /_matrix/client/r0/rooms/{room_id}/redact/{event_id}/{txn_id}
/// Tries to send a redaction event into the room.
///
/// - TODO: Handle txn id
#[endpoint]
pub(super) async fn send_redact(
    _aa: AuthArgs,
    args: RedactEventReqArgs,
    body: JsonBody<RedactEventReqBody>,
    depot: &mut Depot,
) -> JsonResult<RedactEventResBody> {
    let authed = depot.authed_info()?;

    let event_id = crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomRedaction,
            content: to_raw_value(&RoomRedactionEventContent {
                redacts: Some(args.event_id.clone()),
                reason: body.reason.clone(),
            })
            .expect("event is valid, we just created it"),
            unsigned: None,
            state_key: None,
            redacts: Some(args.event_id.into()),
        },
        authed.user_id(),
        &args.room_id,
    )?
    .event_id;

    let event_id = (*event_id).to_owned();
    json_ok(RedactEventResBody { event_id })
}
#[endpoint]
pub(super) async fn timestamp_to_event(_aa: AuthArgs) -> EmptyResult {
    //TODO:??
    // let authed = depot.authed_info()?;
    empty_ok()
}
