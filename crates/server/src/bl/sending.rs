use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose, Engine as _};
use diesel::prelude::*;
use futures_util::stream::FuturesUnordered;
use palpo_core::OwnedEventId;
use tokio::sync::Semaphore;
use tracing::{debug, error, warn};

use crate::core::appservice::event::PushEventsReqBody;
use crate::core::events::receipt::{ReceiptContent, ReceiptData, ReceiptMap};
use crate::core::federation::transaction::Edu;
use crate::core::federation::transaction::{SendMessageReqBody, SendMessageResBody};
use crate::core::identifiers::*;
use crate::core::presence::{PresenceContent, PresenceUpdate};
pub use crate::core::sending::*;
use crate::core::{
    device::DeviceListUpdateContent,
    device_id,
    events::{push_rules::PushRulesEvent, receipt::ReceiptType, AnySyncEphemeralRoomEvent, GlobalAccountDataEventType},
    push, OwnedServerName, OwnedUserId, ServerName, UnixMillis, UserId,
};
use crate::schema::*;
use crate::{db, utils, AppError, AppResult, PduEvent};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OutgoingKind {
    Appservice(String),
    Push(OwnedUserId, String), // user and pushkey
    Normal(OwnedServerName),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SendingEventType {
    Pdu(OwnedEventId), // pduid
    Edu(Vec<u8>),      // pdu json
}

/// The state for a given state hash.
pub(super) static MAXIMUM_REQUESTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));
// pub sender: mpsc::UnboundedSender<(OutgoingKind, SendingEventType, Vec<u8>)>;
// receiver: Mutex<mpsc::UnboundedReceiver<(OutgoingKind, SendingEventType, Vec<u8>)>>;

enum TransactionStatus {
    Running,
    Failed(u32, Instant), // number of times failed, time of last failure
    Retrying(u32),        // number of times failed
}

pub fn start_handler() {
    tokio::spawn(async move {
        handler().await.unwrap();
    });
}

async fn handler() -> AppResult<()> {
    // TODO: fixme
    panic!("fixme")
    // let mut receiver = receiver.lock().await;
    // let mut futures = FuturesUnordered::new();
    // let mut current_transaction_status = HashMap::<OutgoingKind, TransactionStatus>::new();

    // // Retry requests we could not finish yet
    // let mut initial_transactions = HashMap::<OutgoingKind, Vec<SendingEventType>>::new();

    // for (key, outgoing_kind, event) in inactive_requests() {
    //     let entry = initial_transactions
    //         .entry(outgoing_kind.clone())
    //         .or_insert_with(Vec::new);

    //     if entry.len() > 30 {
    //         warn!(
    //             "Dropping some current events: {:?} {:?} {:?}",
    //             key, outgoing_kind, event
    //         );
    //         delete_active_request(key)?;
    //         continue;
    //     }

    //     entry.push(event);
    // }

    // for (outgoing_kind, events) in initial_transactions {
    //     current_transaction_status.insert(outgoing_kind.clone(), TransactionStatus::Running);
    //     futures.push(handle_events(outgoing_kind.clone(), events));
    // }

    // loop {
    //     select! {
    //         Some(response) = futures.next() => {
    //             match response {
    //                 Ok(outgoing_kind) => {
    //                     delete_all_active_requests_for(&outgoing_kind)?;

    //                     // Find events that have been added since starting the last request
    //                     let new_events = queued_requests(&outgoing_kind).into_iter().take(30).collect::<Vec<_>>();

    //                     if !new_events.is_empty() {
    //                         // Insert pdus we found
    //                         mark_as_active(&new_events)?;

    //                         futures.push(
    //                             handle_events(
    //                                 outgoing_kind.clone(),
    //                                 new_events.into_iter().map(|(event, _)| event).collect(),
    //                             )
    //                         );
    //                     } else {
    //                         current_transaction_status.remove(&outgoing_kind);
    //                     }
    //                 }
    //                 Err((outgoing_kind, _)) => {
    //                     current_transaction_status.entry(outgoing_kind).and_modify(|e| *e = match e {
    //                         TransactionStatus::Running => TransactionStatus::Failed(1, Instant::now()),
    //                         TransactionStatus::Retrying(n) => TransactionStatus::Failed(*n+1, Instant::now()),
    //                         TransactionStatus::Failed(_, _) => {
    //                             error!("Request that was not even running failed?!");
    //                             return
    //                         },
    //                     });
    //                 }
    //             };
    //         },
    //         Some((outgoing_kind, event, key)) = receiver.recv() => {
    //             if let Ok(Some(events)) = select_events(
    //                 &outgoing_kind,
    //                 vec![(event, key)],
    //                 &mut current_transaction_status,
    //                 &mut *db::connect()?,
    //             ) {
    //                 futures.push(handle_events(outgoing_kind, events));
    //             }
    //         }
    //     }
    // }
}

#[tracing::instrument(skip_all)]
fn select_events(
    outgoing_kind: &OutgoingKind,
    new_events: Vec<(SendingEventType, Vec<u8>)>, // Events we want to send: event and full key
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

    // TODO: fixme
    // if retry {
    //     // We retry the previous transaction
    //     for (_, e) in active_requests_for(outgoing_kind) {
    //         events.push(e);
    //     }
    // } else {
    //     mark_as_active(&new_events)?;
    //     for (e, _) in new_events {
    //         events.push(e);
    //     }
    //
    //     if let OutgoingKind::Normal(server_name) = outgoing_kind {
    //         if let Ok((select_edus, last_count)) = select_edus(server_name) {
    //             events.extend(select_edus.into_iter().map(SendingEventType::Edu));
    //         }
    //     }
    // }

    Ok(Some(events))
}

#[tracing::instrument(skip(server_name))]
pub fn select_edus(server_name: &ServerName) -> AppResult<(Vec<Vec<u8>>, i64)> {
    let mut events = Vec::new();
    let mut max_edu_sn = last_edu_sn(server_name)?;
    let mut device_list_changes = HashSet::new();
    let conf = crate::config();

    // u64: count of last edu
    let since_sn = last_edu_sn(server_name)?;

    'outer: for room_id in crate::room::server_rooms(server_name)? {
        // Look for device list updates in this room
        device_list_changes.extend(
            crate::room::keys_changed_users(&room_id, since_sn, None)?
                .into_iter()
                .filter(|user_id| user_id.server_name() == &conf.server_name),
        );
        if crate::allow_outcoming_presence() {
            // Look for presence updates in this room
            let mut presence_updates = Vec::new();

            for (user_id, sn, presence_event) in crate::user::presence_since(&room_id, since_sn) {
                if sn > max_edu_sn {
                    max_edu_sn = sn;
                }

                if user_id.server_name() != &conf.server_name {
                    continue;
                }

                presence_updates.push(PresenceUpdate {
                    user_id,
                    presence: presence_event.content.presence,
                    currently_active: presence_event.content.currently_active.unwrap_or(false),
                    last_active_ago: presence_event.content.last_active_ago.unwrap_or(0),
                    status_msg: presence_event.content.status_msg,
                });
            }

            let presence_content = Edu::Presence(PresenceContent::new(presence_updates));
            events.push(serde_json::to_vec(&presence_content).expect("PresenceEvent can be serialized"));
        }

        // Look for read receipts in this room
        for (user_id, event_id, read_receipt) in crate::room::receipt::read_receipts(&room_id, since_sn)? {
            let sn = crate::event::get_event_sn(&event_id)?;
            if sn > max_edu_sn {
                max_edu_sn = sn;
            }

            if user_id.server_name() != &conf.server_name {
                continue;
            }

            let event: AnySyncEphemeralRoomEvent = serde_json::from_str(read_receipt.inner().get())
                .map_err(|_| AppError::internal("Invalid edu event in read_receipts."))?;
            let federation_event = match event {
                AnySyncEphemeralRoomEvent::Receipt(r) => {
                    let mut read = BTreeMap::new();

                    let (event_id, mut receipt) = r
                        .content
                        .0
                        .into_iter()
                        .next()
                        .expect("we only use one event per read receipt");
                    let receipt = receipt
                        .remove(&ReceiptType::Read)
                        .expect("our read receipts always set this")
                        .remove(&user_id)
                        .expect("our read receipts always have the user here");

                    read.insert(
                        user_id,
                        ReceiptData {
                            data: receipt.clone(),
                            event_ids: vec![event_id.to_owned()],
                        },
                    );

                    let receipt_map = ReceiptMap { read };

                    let mut receipts = BTreeMap::new();
                    receipts.insert(room_id.to_owned(), receipt_map);

                    Edu::Receipt(ReceiptContent(receipts))
                }
                _ => {
                    AppError::internal("Invalid event type in read_receipts");
                    continue;
                }
            };

            events.push(serde_json::to_vec(&federation_event).expect("json can be serialized"));

            if events.len() >= 20 {
                break 'outer;
            }
        }
    }

    for user_id in device_list_changes {
        // Empty prev id forces synapse to resync: https://github.com/matrix-org/synapse/blob/98aec1cc9da2bd6b8e34ffb282c85abf9b8b42ca/synapse/handlers/device.py#L767
        // Because synapse resyncs, we can just insert dummy data
        let edu = Edu::DeviceListUpdate(DeviceListUpdateContent {
            user_id,
            device_id: device_id!("dummy").to_owned(),
            device_display_name: Some("Dummy".to_owned()),
            stream_id: 1,
            prev_id: Vec::new(),
            deleted: None,
            keys: None,
        });

        events.push(serde_json::to_vec(&edu).expect("json can be serialized"));
    }

    Ok((events, max_edu_sn))
}

#[tracing::instrument(skip(pdu_id, user, pushkey))]
pub fn send_push_pdu(pdu_id: &EventId, user: &UserId, pushkey: String) -> AppResult<()> {
    // TODO: fixme
    // let outgoing_kind = OutgoingKind::Push(user.to_owned(), pushkey);
    // let event = SendingEventType::Pdu(pdu_id.to_owned());
    // let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    // sender
    //     .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
    //     .unwrap();

    Ok(())
}

#[tracing::instrument(skip(server, pdu_id))]
pub fn send_pdu(server: &ServerName, pdu_id: &EventId) -> AppResult<()> {
    // TODO: fixme
    // let requests = servers
    //     .into_iter()
    //     .map(|server| (OutgoingKind::Normal(server), SendingEventType::Pdu(pdu_id.to_owned())))
    //     .collect::<Vec<_>>();
    // let keys = queue_requests(&requests.iter().map(|(o, e)| (o, e.clone())).collect::<Vec<_>>())?;
    // for ((outgoing_kind, event), key) in requests.into_iter().zip(keys) {
    //     sender.send((outgoing_kind.to_owned(), event, key)).unwrap();
    // }

    Ok(())
}

#[tracing::instrument(skip(server, serialized))]
pub fn send_reliable_edu(server: &ServerName, serialized: Vec<u8>, id: &str) -> AppResult<()> {
    // TODO: fixme
    // let outgoing_kind = OutgoingKind::Normal(server.to_owned());
    // let event = SendingEventType::Edu(serialized);
    // let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    // sender
    //     .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
    //     .unwrap();

    Ok(())
}

#[tracing::instrument]
pub fn send_pdu_appservice(appservice_id: String, pdu_id: &EventId) -> AppResult<()> {
    // TODO: fixme
    // let outgoing_kind = OutgoingKind::Appservice(appservice_id);
    // let event = SendingEventType::Pdu(pdu_id.to_owned());
    // let keys = queue_requests(&[(&outgoing_kind, event.clone())])?;
    // sender
    //     .send((outgoing_kind, event, keys.into_iter().next().unwrap()))
    //     .unwrap();

    Ok(())
}

#[tracing::instrument(skip(events, kind))]
async fn handle_events(
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
                            .ok_or_else(|| {
                                (
                                    kind.clone(),
                                    AppError::internal(
                                        "[Appservice] Event in servernameevent_data not found in database.",
                                    ),
                                )
                            })?
                            .to_room_event(),
                    ),
                    SendingEventType::Edu(_) => {
                        // Appservices don't need EDUs (?)
                    }
                }
            }

            let permit = crate::sending::MAXIMUM_REQUESTS.acquire().await;

            let registration = crate::appservice::get_registration(id)
                .map_err(|e| (kind.clone(), e))?
                .ok_or_else(|| {
                    (
                        kind.clone(),
                        AppError::internal("[Appservice] Could not load registration from database"),
                    )
                })?;
            let req_body = PushEventsReqBody { events: pdu_jsons };

            let txn_id = (&*general_purpose::URL_SAFE_NO_PAD.encode(utils::hash_keys(
                &events
                    .iter()
                    .map(|e| match e {
                        SendingEventType::Edu(b) => b,
                        SendingEventType::Pdu(b) => b.as_bytes(),
                    })
                    .collect::<Vec<_>>(),
            )));
            let url = registration
                .build_url(&format!("/app/v1/transactions/{}", txn_id))
                .map_err(|e| (kind.clone(), e.into()))?;
            let response = crate::sending::post(url)
                .stuff(req_body)
                .map_err(|e| (kind.clone(), e.into()))?
                .send::<()>()
                .await
                .map(|_response| kind.clone())
                .map_err(|e| (kind.clone(), e.into()));

            drop(permit);
            response
        }
        OutgoingKind::Push(user_id, pushkey) => {
            let mut pdus = Vec::new();

            for event in &events {
                match event {
                    SendingEventType::Pdu(event_id) => {
                        pdus.push(
                            crate::room::timeline::get_pdu(event_id)
                                .map_err(|e| (kind.clone(), e))?
                                .ok_or_else(|| {
                                    (
                                        kind.clone(),
                                        AppError::internal(
                                            "[Push] Event in servernamevent_datas not found in database.",
                                        ),
                                    )
                                })?,
                        );
                    }
                    SendingEventType::Edu(_) => {
                        // Push gateways don't need EDUs (?)
                    }
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

                let rules_for_user = crate::user::get_data::<PushRulesEvent>(
                    user_id,
                    None,
                    &GlobalAccountDataEventType::PushRules.to_string(),
                )
                .unwrap_or_default()
                .map(|ev: PushRulesEvent| ev.content.global)
                .unwrap_or_else(|| push::Ruleset::server_default(user_id));

                let unread = crate::room::user::notification_count(user_id, &pdu.room_id)
                    .map_err(|e| (kind.clone(), e.into()))?
                    .try_into()
                    .expect("notification count can't go that high");

                let permit = crate::sending::MAXIMUM_REQUESTS.acquire().await;

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
                        let raw = PduEvent::convert_to_outgoing_federation_event(
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
                }
            }

            let permit = crate::sending::MAXIMUM_REQUESTS.acquire().await;

            let txn_id = &*general_purpose::URL_SAFE_NO_PAD.encode(utils::hash_keys(
                &events
                    .iter()
                    .map(|e| match e {
                        SendingEventType::Edu(b) => b,
                        SendingEventType::Pdu(b) => b.as_bytes(),
                    })
                    .collect::<Vec<_>>(),
            ));
            let response = crate::sending::post(
                server
                    .build_url(&format!("federation/v1/send/{txn_id}"))
                    .map_err(|e| (OutgoingKind::Normal(server.clone()), e.into()))?,
            )
            .stuff(SendMessageReqBody {
                origin: crate::server_name().to_owned(),
                pdus: pdu_jsons,
                edus: edu_jsons,
                origin_server_ts: UnixMillis::now(),
            })
            .map_err(|e| (kind.clone(), e.into()))?
            .send::<SendMessageResBody>()
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

// #[tracing::instrument(skip(request))]
// pub async fn send_federation_request<T>(request: reqwest::Request) -> AppResult<T>
// where
//     T: Debug,
// {
//     debug!("Waiting for permit");
//     let permit = MAXIMUM_REQUESTS.acquire().await;
//     debug!("Got permit");
//     let response = tokio::time::timeout(Duration::from_secs(2 * 60), crate::federation::send_request(request))
//         .await
//         .map_err(|_| {
//             warn!("Timeout waiting for server response of {}", request.url());
//             AppError::public("Timeout waiting for server response")
//         })?;
//     drop(permit);

//     response
// }

// #[tracing::instrument(skip(registration, request))]
// pub async fn send_appservice_request<T>(registration: Registration, request: T) -> AppResult<T::IncomingResponse>
// where
//     T: Debug,
// {
//     let permit = MAXIMUM_REQUESTS.acquire().await;
//     let response = crate::appservice::send_request(registration, request).await;
//     drop(permit);

//     response
// }

fn active_requests() -> AppResult<Vec<(Vec<u8>, OutgoingKind, SendingEventType)>> {
    // self.servercurrentevent_data
    //     .iter()
    //     .map(|(key, v)| parse_servercurrentevent(&key, v).map(|(k, e)| (key, k, e))),
    // TODO: fixme
    panic!("not implemented")
}

fn active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<Vec<(Vec<u8>, SendingEventType)>> {
    // let prefix = outgoing_kind.get_prefix();
    //     self.servercurrentevent_data
    //         .scan_prefix(prefix)
    //         .map(|(key, v)| parse_servercurrentevent(&key, v).map(|(_, e)| (key, e))),
    // TODO: fixme
    panic!("not implemented")
}

fn delete_active_request(key: Vec<u8>) -> AppResult<()> {
    // TODO: fixme
    panic!("not implemented")
    // self.servercurrentevent_data.remove(&key)
}

fn delete_all_active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    // TODO: fixme
    // let prefix = outgoing_kind.get_prefix();
    // for (key, _) in self.servercurrentevent_data.scan_prefix(prefix) {
    //     self.servercurrentevent_data.remove(&key)?;
    // }

    Ok(())
}

fn delete_all_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    // TODO: fixme
    // let prefix = outgoing_kind.get_prefix();
    // for (key, _) in self.servercurrentevent_data.scan_prefix(prefix.clone()) {
    //     self.servercurrentevent_data.remove(&key).unwrap();
    // }

    // for (key, _) in self.servernameevent_data.scan_prefix(prefix) {
    //     self.servernameevent_data.remove(&key).unwrap();
    // }

    Ok(())
}

fn queue_requests(requests: &[(&OutgoingKind, SendingEventType)]) -> AppResult<Vec<Vec<u8>>> {
    // TODO: fixme
    panic!("not implemented")

    // let mut batch = Vec::new();
    // let mut keys = Vec::new();
    // for (outgoing_kind, event) in requests {
    //     let mut key = outgoing_kind.get_prefix();
    //     if let SendingEventType::Pdu(value) = &event {
    //         key.extend_from_slice(value)
    //     } else {
    //         key.extend_from_slice(&crate::next_sn()?.to_be_bytes())
    //     }
    //     let value = if let SendingEventType::Edu(value) = &event {
    //         &**value
    //     } else {
    //         &[]
    //     };
    //     batch.push((key.clone(), value.to_owned()));
    //     keys.push(key);
    // }
    // self.servernameevent_data.insert_batch(&mut batch.into_iter())?;
    // Ok(keys)
}

fn queued_requests(outgoing_kind: &OutgoingKind) -> AppResult<(SendingEventType, Vec<u8>)> {
    // TODO: fixme
    panic!("not implemented")

    // let prefix = outgoing_kind.get_prefix();
    // self.servernameevent_data
    //     .scan_prefix(prefix)
    //     .map(|(k, v)| parse_servercurrentevent(&k, v).map(|(_, ev)| (ev, k)))
}
fn mark_as_active(events: &[(SendingEventType, Vec<u8>)]) -> AppResult<()> {
    // TODO: fixme
    // for (e, key) in events {
    //     let value = if let SendingEventType::Edu(value) = &e {
    //         &**value
    //     } else {
    //         &[]
    //     };
    //     self.servercurrentevent_data.insert(key, value)?;
    //     self.servernameevent_data.remove(key)?;
    // }

    Ok(())
}

fn last_edu_sn(server_name: &ServerName) -> AppResult<i64> {
    // TODO: fixme
    panic!("todo")
    // Ok(events::table.filter(events::server_id.eq(server_name))
    //     .find(event_id)
    //     .select(events::sn)
    //     .order(events::sn.desc())
    //     .first::<i64>()?
    //     .map(|sn| sn as u64))
}
