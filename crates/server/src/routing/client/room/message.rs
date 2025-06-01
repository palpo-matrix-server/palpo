use std::collections::BTreeMap;
use std::collections::HashSet;

use diesel::prelude::*;
use salvo::prelude::*;

use crate::core::client::message::{
    CreateMessageReqArgs, CreateMessageWithTxnReqArgs, MessagesReqArgs, MessagesResBody, SendMessageResBody,
};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::serde::JsonValue;
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::room::timeline;
use crate::{AuthArgs, JsonResult, MatrixError, PduBuilder, config, exts::*, json_ok, room};

/// #GET /_matrix/client/r0/rooms/{room_id}/messages
/// Allows paginating through room history.
///
/// - Only works if the user is joined (TODO: always allow, but only show events where the user was
/// joined, depending on history_visibility)
#[endpoint]
pub(super) async fn get_messages(
    _aa: AuthArgs,
    args: MessagesReqArgs,
    depot: &mut Depot,
) -> JsonResult<MessagesResBody> {
    let authed = depot.authed_info()?;

    let is_joined = diesel_exists!(
        room_users::table
            .filter(room_users::room_id.eq(&args.room_id))
            .filter(room_users::user_id.eq(authed.user_id()))
            .filter(room_users::membership.eq("join")),
        &mut connect()?
    )?;
    let until_sn = if !is_joined {
        let Some((until_sn, forgotten)) = room_users::table
            .filter(room_users::room_id.eq(&args.room_id))
            .filter(room_users::user_id.eq(authed.user_id()))
            .filter(room_users::membership.eq("leave"))
            .select((room_users::event_sn, room_users::forgotten))
            .first::<(i64, bool)>(&mut connect()?)
            .optional()?
        else {
            return Err(MatrixError::forbidden("You aren't a member of the room.", None).into());
        };
        if forgotten {
            return Err(MatrixError::forbidden("You aren't a member of the room.", None).into());
        }
        Some(until_sn)
    } else {
        None
    };

    let from: i64 = args
        .from
        .as_ref()
        .map(|from| from.parse())
        .transpose()?
        .unwrap_or_else(|| match args.dir {
            crate::core::Direction::Forward => 0,
            crate::core::Direction::Backward => i64::MAX,
        });
    let to: Option<i64> = args.to.as_ref().map(|to| to.parse()).transpose()?;

    crate::room::lazy_loading::lazy_load_confirm_delivery(authed.user_id(), authed.device_id(), &args.room_id, from)?;

    let limit = usize::from(args.limit).min(100);
    let next_token;
    let mut resp = MessagesResBody::default();
    let mut lazy_loaded = HashSet::new();
    match args.dir {
        crate::core::Direction::Forward => {
            let events = timeline::get_pdus_forward(
                authed.user_id(),
                &args.room_id,
                from,
                until_sn,
                Some(&args.filter),
                limit,
            )?;

            for (_, event) in &events {
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

            next_token = events.last().map(|(sn, _)| sn).copied();

            let events: Vec<_> = events.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

            resp.start = from.to_string();
            resp.end = next_token.map(|sn| sn.to_string());
            resp.chunk = events;
        }
        crate::core::Direction::Backward => {
            timeline::backfill_if_required(&args.room_id, from).await?;
            let from = if let Some(until_sn) = until_sn {
                until_sn.min(from)
            } else {
                from
            };
            let events =
                timeline::get_pdus_backward(authed.user_id(), &args.room_id, from, None, Some(&args.filter), limit)?;

            for (_, event) in &events {
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

            next_token = events.last().map(|(sn, _)| sn).copied();

            let events: Vec<_> = events.into_iter().map(|(_, pdu)| pdu.to_room_event()).collect();

            resp.start = from.to_string();
            resp.end = next_token.map(|sn| sn.to_string());
            resp.chunk = events;
        }
    }

    resp.state = Vec::new();
    for ll_id in &lazy_loaded {
        if let Ok(member_event) = room::get_state(&args.room_id, &StateEventType::RoomMember, ll_id.as_str(), None) {
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

/// #PUT /_matrix/client/r0/rooms/{room_id}/send/{event_type}/{txn_id}
/// Send a message event into the room.
///
/// - Is a NOOP if the txn id was already used before and returns the same event id again
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
#[endpoint]
pub(super) async fn send_message(
    _aa: AuthArgs,
    args: CreateMessageWithTxnReqArgs,
    req: &mut Request,
    depot: &mut Depot,
) -> JsonResult<SendMessageResBody> {
    println!("DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD");
    let authed = depot.authed_info()?;

    // Forbid m.room.encrypted if encryption is disabled
    if TimelineEventType::RoomEncrypted == args.event_type.to_string().into() && !config::allow_encryption() {
        return Err(MatrixError::forbidden("Encryption has been disabled", None).into());
    }

    let payload = req.payload().await?;
    // Ensure it's valid JSON.
    let _content: JsonValue =
        serde_json::from_slice(payload).map_err(|_| MatrixError::bad_json("Invalid JSON body."))?;

    let state_lock = room::lock_state(&args.room_id).await;
    // Check if this is a new transaction id
    if let Some(event_id) = crate::transaction_id::get_event_id(
        &args.txn_id,
        authed.user_id(),
        Some(authed.device_id()),
        Some(&args.room_id),
    )? {
        return json_ok(SendMessageResBody::new(event_id));
    }

    let mut unsigned = BTreeMap::new();
    unsigned.insert("transaction_id".to_owned(), args.txn_id.to_string().into());

    let event_id = timeline::build_and_append_pdu(
        PduBuilder {
            event_type: args.event_type.to_string().into(),
            content: serde_json::from_slice(payload).map_err(|_| MatrixError::bad_json("Invalid JSON body."))?,
            unsigned: Some(unsigned),
            timestamp: if authed.appservice().is_some() {
                args.timestamp
            } else {
                None
            },
            ..Default::default()
        },
        authed.user_id(),
        &args.room_id,
        &state_lock,
    )?
    .event_id;

    crate::transaction_id::add_txn_id(
        &args.txn_id,
        authed.user_id(),
        Some(authed.device_id()),
        Some(&args.room_id),
        Some(&event_id),
    )?;
    
    json_ok(SendMessageResBody::new((*event_id).to_owned()))
}

/// #POST /_matrix/client/r0/rooms/{room_id}/send/{event_type}
/// Send a message event into the room.
///
/// - Is a NOOP if the txn id was already used before and returns the same event id again
/// - The only requirement for the content is that it has to be valid json
/// - Tries to send the event into the room, auth rules will determine if it is allowed
#[endpoint]
pub(super) async fn post_message(
    _aa: AuthArgs,
    args: CreateMessageReqArgs,
    req: &mut Request,
    depot: &mut Depot,
) -> JsonResult<SendMessageResBody> {
    let authed = depot.authed_info()?;

    let state_lock = room::lock_state(&args.room_id).await;
    // Forbid m.room.encrypted if encryption is disabled
    if TimelineEventType::RoomEncrypted == args.event_type.to_string().into() && !config::allow_encryption() {
        return Err(MatrixError::forbidden("Encryption has been disabled", None).into());
    }

    let payload = req.payload().await?;
    // Ensure it's valid JSON.
    let content: JsonValue =
        serde_json::from_slice(payload).map_err(|_| MatrixError::bad_json("Invalid JSON body."))?;
    if !content.is_object() {
        return Err(MatrixError::bad_json("JSON body is not object.").into());
    }

    let event_id = timeline::build_and_append_pdu(
        PduBuilder {
            event_type: args.event_type.to_string().into(),
            content: serde_json::from_slice(payload).map_err(|_| MatrixError::bad_json("Invalid JSON body."))?,
            unsigned: Some(BTreeMap::new()),
            ..Default::default()
        },
        authed.user_id(),
        &args.room_id,
        &state_lock,
    )?
    .event_id;

    json_ok(SendMessageResBody::new((*event_id).to_owned()))
}
