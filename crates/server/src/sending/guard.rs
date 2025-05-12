use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose};
use diesel::prelude::*;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use serde_json::value::to_raw_value;
use tokio::sync::{Mutex, Semaphore, mpsc};

use super::sender;
use super::{
    EduBuf, EduVec, MPSC_RECEIVER, MPSC_SENDER, OutgoingKind, SELECT_EDU_LIMIT, SELECT_PRESENCE_LIMIT,
    SELECT_RECEIPT_LIMIT, SendingEventType, TransactionStatus,
};
use crate::core::appservice::Registration;
use crate::core::appservice::event::{PushEventsReqBody, push_events_request};
use crate::core::device::DeviceListUpdateContent;
use crate::core::events::GlobalAccountDataEventType;
use crate::core::events::push_rules::PushRulesEventContent;
use crate::core::events::receipt::{ReceiptContent, ReceiptData, ReceiptMap, ReceiptType};
use crate::core::federation::transaction::{Edu, SendMessageReqBody, SendMessageResBody, send_message_request};
use crate::core::identifiers::*;
use crate::core::presence::{PresenceContent, PresenceUpdate};
pub use crate::core::sending::*;
use crate::core::serde::{CanonicalJsonObject, RawJsonValue};
use crate::core::{Seqnum, UnixMillis, device_id, push};
use crate::data::connect;
use crate::data::schema::*;
use crate::data::sending::{DbOutgoingRequest, NewDbOutgoingRequest};
use crate::{AppError, AppResult, config, data, exts::*, utils};

pub fn start() {
    let (sender, receiver) = mpsc::unbounded_channel();
    let _ = MPSC_SENDER.set(sender);
    let _ = MPSC_RECEIVER.set(Mutex::new(receiver));
    tokio::spawn(async move {
        process().await.unwrap();
    });
}

async fn process() -> AppResult<()> {
    let mut receiver = MPSC_RECEIVER.get().expect("receiver should exist").lock().await;
    let mut futures = FuturesUnordered::new();
    let mut current_transaction_status = HashMap::<OutgoingKind, TransactionStatus>::new();

    // Retry requests we could not finish yet
    let mut initial_transactions = HashMap::<OutgoingKind, Vec<SendingEventType>>::new();

    for (id, outgoing_kind, event) in active_requests()? {
        let entry = initial_transactions
            .entry(outgoing_kind.clone())
            .or_insert_with(Vec::new);

        if entry.len() > 30 {
            warn!("Dropping some current events: {:?} {:?} {:?}", id, outgoing_kind, event);
            delete_request(id)?;
            continue;
        }

        entry.push(event);
    }

    for (outgoing_kind, events) in initial_transactions {
        current_transaction_status.insert(outgoing_kind.clone(), TransactionStatus::Running);
        futures.push(send_events(outgoing_kind.clone(), events));
    }

    loop {
        tokio::select! {
            Some(response) = futures.next() => {
                match response {
                    Ok(outgoing_kind) => {
                        delete_all_active_requests_for(&outgoing_kind)?;

                        // Find events that have been added since starting the last request
                        let new_events = queued_requests(&outgoing_kind).unwrap_or_default().into_iter().take(30).collect::<Vec<_>>();

                        if !new_events.is_empty() {
                            // Insert pdus we found
                            mark_as_active(&new_events)?;

                            futures.push(
                                send_events(
                                    outgoing_kind.clone(),
                                    new_events.into_iter().map(|(_, event)| event).collect(),
                                )
                            );
                        } else {
                            current_transaction_status.remove(&outgoing_kind);
                        }
                    }
                    Err((outgoing_kind, event)) => {
                        tracing::error!("Failed to send event: {:?}", event);
                        current_transaction_status.entry(outgoing_kind).and_modify(|e| *e = match e {
                            TransactionStatus::Running => TransactionStatus::Failed(1, Instant::now()),
                            TransactionStatus::Retrying(n) => TransactionStatus::Failed(n.saturating_add(1), Instant::now()),
                            TransactionStatus::Failed(_, _) => {
                                error!("Request that was not even running failed?!");
                                return
                            },
                        });
                    }
                };
            },
            Some((outgoing_kind, event, id)) = receiver.recv() => {
                if let Ok(Some(events)) = select_events(
                    &outgoing_kind,
                    vec![(id, event)],
                    &mut current_transaction_status,
                ) {
                    futures.push(send_events(outgoing_kind, events));
                }
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn select_events(
    outgoing_kind: &OutgoingKind,
    new_events: Vec<(i64, SendingEventType)>, // Events we want to send: event and full key
    current_transaction_status: &mut HashMap<OutgoingKind, TransactionStatus>,
) -> AppResult<Option<Vec<SendingEventType>>> {
    let mut retry = false;
    let mut allow = true;

    let entry = current_transaction_status.entry(outgoing_kind.clone());

    entry
        .and_modify(|e| match e {
            TransactionStatus::Running | TransactionStatus::Retrying(_) => {
                allow = false; // already running
            }
            TransactionStatus::Failed(tries, time) => {
                // Fail if a request has failed recently (exponential backoff)
                let mut min_elapsed_duration = Duration::from_secs(30) * (*tries) * (*tries);
                if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
                    min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
                }

                if time.elapsed() < min_elapsed_duration {
                    allow = false;
                } else {
                    retry = true;
                    *e = TransactionStatus::Retrying(*tries);
                }
            }
        })
        .or_insert(TransactionStatus::Running);

    if !allow {
        return Ok(None);
    }

    let mut events = Vec::new();

    if retry {
        // We retry the previous transaction
        for (_, e) in active_requests_for(outgoing_kind)? {
            events.push(e);
        }
    } else {
        mark_as_active(&new_events)?;
        for (_, e) in new_events {
            events.push(e);
        }

        if let OutgoingKind::Normal(server_name) = outgoing_kind {
            if let Ok((select_edus, last_count)) = select_edus(server_name) {
                events.extend(select_edus.into_iter().map(SendingEventType::Edu));
            }
        }
    }

    Ok(Some(events))
}

/// Look for device changes
#[tracing::instrument(level = "trace", skip(server_name, max_edu_sn))]
fn select_edus_device_changes(
    server_name: &ServerName,
    since_sn: Seqnum,
    max_edu_sn: &Seqnum,
    events_len: &AtomicUsize,
) -> AppResult<EduVec> {
    let mut events = EduVec::new();
    let server_rooms = crate::room::server_joined_rooms(server_name)?;

    let mut device_list_changes = HashSet::<OwnedUserId>::new();
    for room_id in server_rooms {
        let keys_changed = data::user::room_keys_changed(&room_id, since_sn, None)?
            .into_iter()
            .filter(|(user_id, _)| user_id.is_local());

        for (user_id, event_sn) in keys_changed {
            // max_edu_sn.fetch_max(event_sn, Ordering::Relaxed);
            if !device_list_changes.insert(user_id.clone()) {
                continue;
            }

            // Empty prev id forces synapse to resync; because synapse resyncs,
            // we can just insert placeholder data
            let edu = Edu::DeviceListUpdate(DeviceListUpdateContent {
                user_id,
                device_id: device_id!("placeholder").to_owned(),
                device_display_name: Some("Placeholder".to_owned()),
                stream_id: 1,
                prev_id: Vec::new(),
                deleted: None,
                keys: None,
            });

            let mut buf = EduBuf::new();
            serde_json::to_writer(&mut buf, &edu).expect("failed to serialize device list update to JSON");

            events.push(buf);
            if events_len.fetch_add(1, Ordering::Relaxed) >= SELECT_EDU_LIMIT - 1 {
                return Ok(events);
            }
        }
    }

    Ok(events)
}

/// Look for read receipts in this room
#[tracing::instrument(level = "trace", skip(server_name, max_edu_sn))]
fn select_edus_receipts(server_name: &ServerName, since_sn: Seqnum, max_edu_sn: &Seqnum) -> AppResult<Option<EduBuf>> {
    let mut num = 0;
    let receipts: BTreeMap<OwnedRoomId, ReceiptMap> = crate::room::server_joined_rooms(server_name)?
        .into_iter()
        .filter_map(|room_id| {
            let receipt_map = select_edus_receipts_room(&room_id, since_sn, max_edu_sn, &mut num).ok()?;

            receipt_map.read.is_empty().eq(&false).then_some((room_id, receipt_map))
        })
        .collect();

    if receipts.is_empty() {
        return Ok(None);
    }

    let receipt_content = Edu::Receipt(ReceiptContent::new(receipts));

    let mut buf = EduBuf::new();
    serde_json::to_writer(&mut buf, &receipt_content).expect("Failed to serialize Receipt EDU to JSON vec");

    Ok(Some(buf))
}
/// Look for read receipts in this room
#[tracing::instrument(level = "trace", skip(since_sn, max_edu_sn))]
fn select_edus_receipts_room(
    room_id: &RoomId,
    since_sn: Seqnum,
    max_edu_sn: &Seqnum,
    num: &mut usize,
) -> AppResult<ReceiptMap> {
    let receipts = crate::room::receipt::read_receipts(room_id, since_sn)?;

    let mut read = BTreeMap::<OwnedUserId, ReceiptData>::new();
    for (user_id, read_receipt) in receipts {
        // if count > since_sn {
        //     break;
        // }

        // max_edu_sn.fetch_max(occur_sn, Ordering::Relaxed);
        if !user_id.is_local() {
            continue;
        }

        // let Ok(event) = serde_json::from_str(read_receipt.inner().get()) else {
        //     error!(
        //         ?user_id,
        //         ?read_receipt,
        //         "Invalid edu event in read_receipts."
        //     );
        //     continue;
        // };

        // let AnySyncEphemeralRoomEvent::Receipt(r) = event else {
        //     error!(?user_id, ?event, "Invalid event type in read_receipts");
        //     continue;
        // };

        let (event_id, mut receipt) = read_receipt
            .0
            .into_iter()
            .next()
            .expect("we only use one event per read receipt");

        let receipt = receipt
            .remove(&ReceiptType::Read)
            .expect("our read receipts always set this")
            .remove(&user_id)
            .expect("our read receipts always have the user here");

        let receipt_data = ReceiptData {
            data: receipt,
            event_ids: vec![event_id.clone()],
        };

        if read.insert(user_id.to_owned(), receipt_data).is_none() {
            *num = num.saturating_add(1);
            if *num >= SELECT_RECEIPT_LIMIT {
                break;
            }
        }
    }

    Ok(ReceiptMap { read })
}

/// Look for presence
#[tracing::instrument(level = "trace", skip(server_name, max_edu_sn))]
fn select_edus_presence(server_name: &ServerName, since_sn: Seqnum, max_edu_sn: &Seqnum) -> AppResult<Option<EduBuf>> {
    let presences_since = crate::data::user::presences_since(since_sn)?;

    let mut presence_updates = HashMap::<OwnedUserId, PresenceUpdate>::new();
    for (user_id, presence_event) in presences_since {
        // max_edu_sn.fetch_max(occur_sn, Ordering::Relaxed);
        if !user_id.is_local() {
            continue;
        }

        if !crate::room::state::server_can_see_user(server_name, &user_id)? {
            continue;
        }

        let update = PresenceUpdate {
            user_id: user_id.clone(),
            presence: presence_event.content.presence,
            currently_active: presence_event.content.currently_active.unwrap_or(false),
            status_msg: presence_event.content.status_msg,
            last_active_ago: presence_event.content.last_active_ago.unwrap_or(0),
        };

        presence_updates.insert(user_id, update);
        if presence_updates.len() >= SELECT_PRESENCE_LIMIT {
            break;
        }
    }

    if presence_updates.is_empty() {
        return Ok(None);
    }

    let presence_content = Edu::Presence(PresenceContent {
        push: presence_updates.into_values().collect(),
    });

    let mut buf = EduBuf::new();
    serde_json::to_writer(&mut buf, &presence_content).expect("failed to serialize Presence EDU to JSON");

    Ok(Some(buf))
}

#[tracing::instrument(skip(server_name))]
pub fn select_edus(server_name: &ServerName) -> AppResult<(EduVec, i64)> {
    let max_edu_sn = data::curr_sn()?;
    let conf = crate::config();

    let since_sn = data::curr_sn()?;

    let events_len = AtomicUsize::default();
    let device_changes = select_edus_device_changes(server_name, since_sn, &max_edu_sn, &events_len)?;

    let mut events = device_changes;
    if conf.allow_outgoing_read_receipts {
        if let Some(receipts) = select_edus_receipts(server_name, since_sn, &max_edu_sn)? {
            events.push(receipts);
        }
    }

    if conf.allow_outgoing_presence {
        if let Some(presence) = select_edus_presence(server_name, since_sn, &max_edu_sn)? {
            events.push(presence);
        }
    }

    Ok((events, max_edu_sn))
}

#[tracing::instrument(skip(pdu_id, user, pushkey))]
pub fn send_push_pdu(pdu_id: &EventId, user: &UserId, pushkey: String) -> AppResult<()> {
    let outgoing_kind = OutgoingKind::Push(user.to_owned(), pushkey);
    let event = SendingEventType::Pdu(pdu_id.to_owned());
    let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    sender()
        .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
        .unwrap();

    Ok(())
}

#[tracing::instrument(level = "debug")]
pub fn send_pdu_room(room_id: &RoomId, pdu_id: &EventId) -> AppResult<()> {
    let servers = room_joined_servers::table
        .filter(room_joined_servers::room_id.eq(room_id))
        .filter(room_joined_servers::server_id.ne(config::server_name()))
        .select(room_joined_servers::server_id)
        .load::<OwnedServerName>(&mut connect()?)?;
    send_pdu_servers(servers.into_iter(), pdu_id)
}

#[tracing::instrument(skip(servers, pdu_id), level = "debug")]
pub fn send_pdu_servers<S: Iterator<Item = OwnedServerName>>(servers: S, pdu_id: &EventId) -> AppResult<()> {
    println!("sssSending pdu to servers pdu_id: {pdu_id}");
    let requests = servers
        .into_iter()
        .map(|server| (OutgoingKind::Normal(server), SendingEventType::Pdu(pdu_id.to_owned())))
        .collect::<Vec<_>>();
    let keys = queue_requests(&requests.iter().map(|(o, e)| (o, e.clone())).collect::<Vec<_>>())?;
    for ((outgoing_kind, event), key) in requests.into_iter().zip(keys) {
        sender().send((outgoing_kind.to_owned(), event, key)).unwrap();
    }

    Ok(())
}

#[tracing::instrument(skip(room_id, edu), level = "debug")]
pub fn send_edu_room(room_id: &RoomId, edu: &Edu) -> AppResult<()> {
    let servers = room_joined_servers::table
        .filter(room_joined_servers::room_id.eq(room_id))
        .filter(room_joined_servers::server_id.ne(config::server_name()))
        .select(room_joined_servers::server_id)
        .load::<OwnedServerName>(&mut connect()?)?;
    send_edu_servers(servers.into_iter(), edu)
}

#[tracing::instrument(skip(servers, edu), level = "debug")]
pub fn send_edu_servers<S: Iterator<Item = OwnedServerName>>(servers: S, edu: &Edu) -> AppResult<()> {
    let mut serialized = EduBuf::new();
    serde_json::to_writer(&mut serialized, &edu).expect("Serialized Edu");

    let requests = servers
        .into_iter()
        .map(|server| {
            (
                OutgoingKind::Normal(server),
                SendingEventType::Edu(serialized.to_owned()),
            )
        })
        .collect::<Vec<_>>();
    let keys = queue_requests(&requests.iter().map(|(o, e)| (o, e.clone())).collect::<Vec<_>>())?;
    for ((outgoing_kind, event), key) in requests.into_iter().zip(keys) {
        sender()
            .send((outgoing_kind.to_owned(), event, key))
            .map_err(|e| AppError::internal(e.to_string()))?;
    }

    Ok(())
}
#[tracing::instrument(skip(server, edu), level = "debug")]
pub fn send_edu_server(server: &ServerName, edu: &Edu) -> AppResult<()> {
    let mut serialized = EduBuf::new();
    serde_json::to_writer(&mut serialized, &edu).expect("Serialized Edu");

    let outgoing_kind = OutgoingKind::Normal(server.to_owned());
    let event = SendingEventType::Edu(serialized.to_owned());
    let key = queue_request(&outgoing_kind, &event)?;
    sender()
        .send((outgoing_kind, event, key))
        .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(())
}

#[tracing::instrument(skip(server, edu))]
pub fn send_reliable_edu(server: &ServerName, edu: &Edu, id: &str) -> AppResult<()> {
    let mut serialized = EduBuf::new();
    serde_json::to_writer(&mut serialized, &edu).expect("Serialized Edu");

    let outgoing_kind = OutgoingKind::Normal(server.to_owned());
    let event = SendingEventType::Edu(serialized);
    let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    sender()
        .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
        .unwrap();

    Ok(())
}

#[tracing::instrument]
pub fn send_pdu_appservice(appservice_id: String, pdu_id: &EventId) -> AppResult<()> {
    let outgoing_kind = OutgoingKind::Appservice(appservice_id);
    let event = SendingEventType::Pdu(pdu_id.to_owned());
    let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    sender()
        .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
        .unwrap();

    Ok(())
}

#[tracing::instrument(skip(events, kind))]
async fn send_events(
    kind: OutgoingKind,
    events: Vec<SendingEventType>,
) -> Result<OutgoingKind, (OutgoingKind, AppError)> {
    match &kind {
        OutgoingKind::Appservice(id) => {
            let mut pdu_jsons = Vec::new();
            for event in &events {
                match event {
                    SendingEventType::Pdu(event_id) => pdu_jsons.push(
                        crate::room::timeline::get_pdu(event_id)
                            .map_err(|e| (kind.clone(), e))?
                            .to_room_event(),
                    ),
                    SendingEventType::Edu(_) => {
                        // Appservices don't need EDUs (?)
                    }
                    SendingEventType::Flush => {}
                }
            }

            let max_request = crate::sending::max_request();
            let permit = max_request.acquire().await;

            let registration = crate::appservice::get_registration(id)
                .map_err(|e| (kind.clone(), e))?
                .ok_or_else(|| {
                    (
                        kind.clone(),
                        AppError::internal("[Appservice] Could not load registration from database"),
                    )
                })?;
            let req_body = PushEventsReqBody { events: pdu_jsons };

            let txn_id =
                &*general_purpose::URL_SAFE_NO_PAD.encode(utils::hash_keys(events.iter().filter_map(|e| match e {
                    SendingEventType::Edu(b) => Some(&**b),
                    SendingEventType::Pdu(b) => Some(b.as_bytes()),
                    SendingEventType::Flush => None,
                })));
            let request = push_events_request(registration.url.as_deref().unwrap_or_default(), txn_id, req_body)
                .map_err(|e| (kind.clone(), e.into()))?
                .into_inner();
            let response = crate::appservice::send_request(registration, request)
                .await
                .map_err(|e| (kind.clone(), e.into()))
                .map(|_response| kind.clone());

            drop(permit);
            response
        }
        OutgoingKind::Push(user_id, pushkey) => {
            let mut pdus = Vec::new();

            for event in &events {
                match event {
                    SendingEventType::Pdu(event_id) => {
                        pdus.push(crate::room::timeline::get_pdu(event_id).map_err(|e| (kind.clone(), e))?);
                    }
                    SendingEventType::Edu(_) => {
                        // Push gateways don't need EDUs (?)
                    }
                    SendingEventType::Flush => {}
                }
            }

            for pdu in pdus {
                // Redacted events are not notification targets (we don't send push for them)
                if let Some(unsigned) = &pdu.unsigned {
                    if let Ok(unsigned) = serde_json::from_str::<serde_json::Value>(unsigned.get()) {
                        if unsigned.get("redacted_because").is_some() {
                            continue;
                        }
                    }
                }
                let pusher = match crate::user::pusher::get_pusher(user_id, pushkey)
                    .map_err(|e| (OutgoingKind::Push(user_id.clone(), pushkey.clone()), e))?
                {
                    Some(pusher) => pusher,
                    None => continue,
                };

                let rules_for_user = data::user::get_global_data::<PushRulesEventContent>(
                    user_id,
                    &GlobalAccountDataEventType::PushRules.to_string(),
                )
                .unwrap_or_default()
                .map(|content: PushRulesEventContent| content.global)
                .unwrap_or_else(|| push::Ruleset::server_default(user_id));

                let unread = crate::room::user::notification_count(user_id, &pdu.room_id)
                    .map_err(|e| (kind.clone(), e.into()))?
                    .try_into()
                    .expect("notification count can't go that high");

                let max_request = crate::sending::max_request();
                let permit = max_request.acquire().await;

                let _response = crate::user::pusher::send_push_notice(user_id, unread, &pusher, rules_for_user, &pdu)
                    .await
                    .map(|_response| kind.clone())
                    .map_err(|e| (kind.clone(), e));

                drop(permit);
            }
            Ok(OutgoingKind::Push(user_id.clone(), pushkey.clone()))
        }
        OutgoingKind::Normal(server) => {
            let mut edu_jsons = Vec::new();
            let mut pdu_jsons = Vec::new();

            for event in &events {
                match event {
                    SendingEventType::Pdu(pdu_id) => {
                        // TODO: check room version and remove event_id if needed
                        let raw = crate::sending::convert_to_outgoing_federation_event(
                            crate::room::timeline::get_pdu_json(pdu_id)
                                .map_err(|e| (OutgoingKind::Normal(server.clone()), e.into()))?
                                .ok_or_else(|| {
                                    error!("event not found: {server} {pdu_id:?}");
                                    (
                                        OutgoingKind::Normal(server.clone()),
                                        AppError::internal(
                                            "[Normal] Event in servernamevent_datas not found in database.",
                                        ),
                                    )
                                })?,
                        );
                        pdu_jsons.push(raw);
                    }
                    SendingEventType::Edu(edu) => {
                        if let Ok(raw) = serde_json::from_slice(edu) {
                            edu_jsons.push(raw);
                        }
                    }
                    SendingEventType::Flush => {} // flush only; no new content
                }
            }

            let max_request = crate::sending::max_request();
            let permit = max_request.acquire().await;

            let txn_id =
                &*general_purpose::URL_SAFE_NO_PAD.encode(utils::hash_keys(events.iter().filter_map(|e| match e {
                    SendingEventType::Edu(b) => Some(&**b),
                    SendingEventType::Pdu(b) => Some(b.as_bytes()),
                    SendingEventType::Flush => None,
                })));
            println!("============{}  ===================send_message_request 0", crate::config::server_name());
            let request = send_message_request(
                &server.origin().await,
                txn_id,
                SendMessageReqBody {
                    origin: config::server_name().to_owned(),
                    pdus: pdu_jsons,
                    edus: edu_jsons,
                    origin_server_ts: UnixMillis::now(),
                },
            )
            .map_err(|e| (kind.clone(), e.into()))?
            .into_inner();
            let response = crate::sending::send_federation_request(server, request)
                .await
                .map_err(|e| (kind.clone(), e.into()))?
                .json::<SendMessageResBody>()
                .await
                .map(|response| {
                    for pdu in response.pdus {
                        if pdu.1.is_err() {
                            warn!("Failed to send to {}: {:?}", server, pdu);
                        }
                    }
                    kind.clone()
                })
                .map_err(|e| (kind, e.into()));

            drop(permit);

            response
        }
    }
}

#[tracing::instrument(skip(request))]
pub async fn send_federation_request(
    destination: &ServerName,
    request: reqwest::Request,
) -> AppResult<reqwest::Response> {
    debug!("Waiting for permit");
    let max_request = super::max_request();
    let permit = max_request.acquire().await;
    debug!("Got permit");
    let url = request.url().clone();
    let response = tokio::time::timeout(
        Duration::from_secs(2 * 60),
        crate::federation::send_request(destination, request),
    )
    .await
    .map_err(|_| {
        warn!("Timeout waiting for server response of {}", url);
        AppError::public("Timeout waiting for server response")
    })?;
    drop(permit);

    response.map_err(Into::into)
}

#[tracing::instrument(skip_all)]
pub async fn send_appservice_request<T>(registration: Registration, request: reqwest::Request) -> AppResult<T>
where
    T: for<'de> Deserialize<'de> + Debug,
{
    // let permit = acquire_request().await;
    let response = crate::appservice::send_request(registration, request).await?;
    // drop(permit);

    Ok(response.json().await?)
}

fn active_requests() -> AppResult<Vec<(i64, OutgoingKind, SendingEventType)>> {
    Ok(outgoing_requests::table
        .filter(outgoing_requests::state.eq("pending"))
        .load::<DbOutgoingRequest>(&mut connect()?)?
        .into_iter()
        .filter_map(|item| {
            let kind = match item.kind.as_str() {
                "appservice" => {
                    if let Some(appservice_id) = &item.appservice_id {
                        OutgoingKind::Appservice(appservice_id.clone())
                    } else {
                        return None;
                    }
                }
                "push" => {
                    if let (Some(user_id), Some(pushkey)) = (item.user_id.clone(), item.pushkey.clone()) {
                        OutgoingKind::Push(user_id, pushkey)
                    } else {
                        return None;
                    }
                }
                "normal" => {
                    if let Some(server_id) = &item.server_id {
                        OutgoingKind::Normal(server_id.to_owned())
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let event = if let Some(value) = item.edu_json {
                SendingEventType::Edu(value)
            } else if let Some(pdu_id) = item.pdu_id {
                SendingEventType::Pdu(pdu_id)
            } else {
                return None;
            };
            Some((item.id, kind, event))
        })
        .collect())
}

fn delete_request(id: i64) -> AppResult<()> {
    diesel::delete(outgoing_requests::table.find(id)).execute(&mut connect()?)?;
    Ok(())
}

fn delete_all_active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    diesel::delete(
        outgoing_requests::table
            .filter(outgoing_requests::kind.eq(outgoing_kind.name()))
            .filter(outgoing_requests::state.eq("pending")),
    )
    .execute(&mut connect()?)?;

    Ok(())
}

fn delete_all_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    diesel::delete(outgoing_requests::table.filter(outgoing_requests::kind.eq(outgoing_kind.name())))
        .execute(&mut connect()?)?;

    Ok(())
}

fn queue_requests(requests: &[(&OutgoingKind, SendingEventType)]) -> AppResult<Vec<i64>> {
    let mut ids = Vec::new();
    for (outgoing_kind, event) in requests {
        ids.push(queue_request(outgoing_kind, event)?);
    }
    Ok(ids)
}
fn queue_request(outgoing_kind: &OutgoingKind, event: &SendingEventType) -> AppResult<i64> {
    let appservice_id = if let OutgoingKind::Appservice(service_id) = outgoing_kind {
        Some(service_id.clone())
    } else {
        None
    };
    let (user_id, pushkey) = if let OutgoingKind::Push(user_id, pushkey) = outgoing_kind {
        (Some(user_id.clone()), Some(pushkey.clone()))
    } else {
        (None, None)
    };
    let server_id = if let OutgoingKind::Normal(server_id) = outgoing_kind {
        Some(server_id.clone())
    } else {
        None
    };
    let (pdu_id, edu_json) = match event {
        SendingEventType::Pdu(pdu_id) => (Some(pdu_id.to_owned()), None),
        SendingEventType::Edu(edu_json) => (None, Some(edu_json.clone())),
        SendingEventType::Flush => (None, None),
    };
    let id = diesel::insert_into(outgoing_requests::table)
        .values(&NewDbOutgoingRequest {
            kind: outgoing_kind.name().to_owned(),
            appservice_id,
            user_id,
            pushkey,
            server_id,
            pdu_id,
            edu_json,
        })
        .returning(outgoing_requests::id)
        .get_result::<i64>(&mut connect()?)?;
    Ok(id)
}

fn active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<Vec<(i64, SendingEventType)>> {
    let list = outgoing_requests::table
        .filter(outgoing_requests::kind.eq(outgoing_kind.name()))
        .load::<DbOutgoingRequest>(&mut connect()?)?
        .into_iter()
        .filter_map(|r| {
            if let Some(value) = r.edu_json {
                Some((r.id, SendingEventType::Edu(value)))
            } else if let Some(pdu_id) = r.pdu_id {
                Some((r.id, SendingEventType::Pdu(pdu_id)))
            } else {
                None
            }
        })
        .collect();

    Ok(list)
}

fn queued_requests(outgoing_kind: &OutgoingKind) -> AppResult<Vec<(i64, SendingEventType)>> {
    Ok(outgoing_requests::table
        .filter(outgoing_requests::kind.eq(outgoing_kind.name()))
        .load::<DbOutgoingRequest>(&mut connect()?)?
        .into_iter()
        .filter_map(|r| {
            if let Some(value) = r.edu_json {
                Some((r.id, SendingEventType::Edu(value)))
            } else if let Some(pdu_id) = r.pdu_id {
                Some((r.id, SendingEventType::Pdu(pdu_id)))
            } else {
                None
            }
        })
        .collect())
}
fn mark_as_active(events: &[(i64, SendingEventType)]) -> AppResult<()> {
    for (id, e) in events {
        let value = if let SendingEventType::Edu(value) = &e {
            &**value
        } else {
            &[]
        };
        diesel::update(outgoing_requests::table.find(id))
            .set((
                outgoing_requests::data.eq(value),
                outgoing_requests::state.eq("pending"),
            ))
            .execute(&mut connect()?)?;
    }

    Ok(())
}

/// This does not return a full `Pdu` it is only to satisfy palpo's types.
#[tracing::instrument]
pub fn convert_to_outgoing_federation_event(mut pdu_json: CanonicalJsonObject) -> Box<RawJsonValue> {
    if let Some(unsigned) = pdu_json.get_mut("unsigned").and_then(|val| val.as_object_mut()) {
        unsigned.remove("transaction_id");
    }

    pdu_json.remove("event_id");
    pdu_json.remove("event_sn");

    // TODO: another option would be to convert it to a canonical string to validate size
    // and return a Result<RawJson<...>>
    // serde_json::from_str::<RawJson<_>>(
    //     crate::core::serde::to_canonical_json_string(pdu_json).expect("CanonicalJson is valid serde_json::Value"),
    // )
    // .expect("RawJson::from_value always works")

    to_raw_value(&pdu_json).expect("CanonicalJson is valid serde_json::Value")
}
