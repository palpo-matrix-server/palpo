use std::collections::BTreeMap;
use std::collections::HashSet;

use salvo::prelude::*;

use crate::core::client::message::{
    CreateMessageEventReqArgs, MessageEventsReqArgs, MessageEventsResBody, SendMessageEventResBody,
};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::{exts::*, json_ok, AuthArgs, JsonResult, MatrixError, PduBuilder};

// #GET /_matrix/client/r0/rooms/{room_id}/messages
/// Allows paginating through room history.
///
/// - Only works if the user is joined (TODO: always allow, but only show events where the user was
/// joined, depending on history_visibility)
#[endpoint]
pub(super) async fn get_messages(
    _aa: AuthArgs,
    args: MessageEventsReqArgs,
    depot: &mut Depot,
) -> JsonResult<MessageEventsResBody> {
    let authed = depot.authed_info()?;

    let from: i64 = args
        .from
        .as_ref()
        .map(|from| from.parse())
        .transpose()?
        .unwrap_or_default();

    let to: Option<i64> = args.to.as_ref().map(|to| to.parse()).transpose()?;

    crate::room::lazy_loading::lazy_load_confirm_delivery(authed.user_id(), authed.device_id(), &args.room_id, from)?;

    let limit = usize::from(args.limit).min(100);

    let next_token;

    let mut resp = MessageEventsResBody::default();

    let mut lazy_loaded = HashSet::new();
    match args.dir {
        crate::core::Direction::Forward => {
            let events_after: Vec<_> =
                crate::room::timeline::get_pdus_forward(authed.user_id(), &args.room_id, from, limit)?;

            for (_, event) in &events_after {
                /* TODO: Remove this when these are resolved:
                 * https://github.com/vector-im/element-android/issues/3417
                 * https://github.com/vector-im/element-web/issues/21034
                if !crate::room::lazy_loading.lazy_load_was_sent_before(
                    authed.user_id(),
                    authed.user_id(),
                    &body.room_id,
                    &event.sender,
                )? {
                    lazy_loaded.insert(event.sender.clone());
                }
                */
                lazy_loaded.insert(event.sender.clone());
            }

            next_token = events_after.last().map(|(count, _)| count).copied();

            let events_after: Vec<_> = events_after.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

            resp.start = from.to_string();
            resp.end = next_token.map(|sn| sn.to_string());
            resp.chunk = events_after;
        }
        crate::core::Direction::Backward => {
            crate::room::timeline::backfill_if_required(&args.room_id, from).await?;
            let events_before: Vec<_> =
                crate::room::timeline::get_pdus_backward(authed.user_id(), &args.room_id, from, limit)?;

            for (_, event) in &events_before {
                /* TODO: Remove this when these are resolved:
                 * https://github.com/vector-im/element-android/issues/3417
                 * https://github.com/vector-im/element-web/issues/21034
                if !crate::room::lazy_loading.lazy_load_was_sent_before(
                    authed.user_id(),
                    authed.device_id(),
                    &args.room_id,
                    &event.sender,
                )? {
                    lazy_loaded.insert(event.sender.clone());
                }
                */
                lazy_loaded.insert(event.sender.clone());
            }

            next_token = events_before.last().map(|(count, _)| count).copied();

            let events_before: Vec<_> = events_before.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

            resp.start = from.to_string();
            resp.end = next_token.map(|sn| sn.to_string());
            resp.chunk = events_before;
        }
    }

    resp.state = Vec::new();
    for ll_id in &lazy_loaded {
        if let Some(member_event) =
            crate::room::state::get_state(&args.room_id, &StateEventType::RoomMember, ll_id.as_str())?
        {
            resp.state.push(member_event.to_state_event());
        }
    }

    // TODO: enable again when we are sure clients can handle it
    /*
    if let Some(next_token) = next_token {
        crate::room::lazy_loading.lazy_load_mark_sent(
            authed.user_id(),
            authed.device_id(),
            &body.room_id,
            lazy_loaded,
            next_token,
        );
    }
    */

    json_ok(resp)
}

// #PUT /_matrix/client/r0/rooms/{room_id}/send/{event_type}/{txn_id}
/// Send a message event into the room.
///
/// - Is a NOOP if the txn id was already used before and returns the same event id again
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
#[endpoint]
pub(super) async fn send_message(
    _aa: AuthArgs,
    args: CreateMessageEventReqArgs,
    req: &mut Request,
    depot: &mut Depot,
) -> JsonResult<SendMessageEventResBody> {
    let authed = depot.authed_info()?;

    // Forbid m.room.encrypted if encryption is disabled
    if TimelineEventType::RoomEncrypted == args.event_type.to_string().into() && !crate::allow_encryption() {
        return Err(MatrixError::forbidden("Encryption has been disabled").into());
    }

    let payload = req.payload().await?;
    // Check if this is a new transaction id
    if let Some(event_id) =
        crate::transaction_id::existing_txn_id(authed.user_id(), Some(authed.device_id()), &args.txn_id)?
    {
        return json_ok(SendMessageEventResBody::new(event_id));
    }

    let mut unsigned = BTreeMap::new();
    unsigned.insert("transaction_id".to_owned(), args.txn_id.to_string().into());

    let event_id = crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: args.event_type.to_string().into(),
            content: serde_json::from_slice(payload).map_err(|_| MatrixError::bad_json("Invalid JSON body."))?,
            unsigned: Some(unsigned),
            state_key: None,
            redacts: None,
        },
        authed.user_id(),
        &args.room_id,
    )?
    .event_id;

    crate::transaction_id::add_txn_id(
        &event_id,
        &args.room_id,
        authed.user_id(),
        Some(authed.device_id()),
        &args.txn_id,
    )?;

    json_ok(SendMessageEventResBody::new((*event_id).to_owned()))
}
