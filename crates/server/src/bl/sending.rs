use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose, Engine as _};
use diesel::prelude::*;
use futures_util::stream::{FuturesUnordered, StreamExt};
use palpo_core::events::push_rules::PushRulesEventContent;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex, Semaphore};

use crate::core::appservice::event::PushEventsReqBody;
use crate::core::device::DeviceListUpdateContent;
use crate::core::events::receipt::ReceiptType;
use crate::core::events::receipt::{ReceiptContent, ReceiptData, ReceiptMap};
use crate::core::events::{AnySyncEphemeralRoomEvent, GlobalAccountDataEventType};
use crate::core::federation::transaction::{Edu, SendMessageReqBody, SendMessageResBody};
use crate::core::identifiers::*;
pub use crate::core::sending::*;
use crate::core::{device_id, push, UnixMillis};
use crate::{db, utils, AppError, AppResult, PduEvent};

use super::{curr_sn, outgoing_requests};

#[derive(Identifiable, Queryable, Insertable, Debug, Clone)]
#[diesel(table_name = outgoing_requests)]
pub struct DbOutgoingRequest {
    pub id: i64,
    pub kind: String,
    pub appservice_id: Option<String>,
    pub user_id: Option<OwnedUserId>,
    pub pushkey: Option<String>,
    pub server_id: Option<OwnedServerName>,
    pub pdu_id: Option<OwnedEventId>,
    pub edu_json: Option<Vec<u8>>,
    pub state: String,
    pub data: Option<Vec<u8>>,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = outgoing_requests)]
pub struct NewDbOutgoingRequest {
    pub kind: String,
    pub appservice_id: Option<String>,
    pub user_id: Option<OwnedUserId>,
    pub pushkey: Option<String>,
    pub server_id: Option<OwnedServerName>,
    pub pdu_id: Option<OwnedEventId>,
    pub edu_json: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OutgoingKind {
    Appservice(String),
    Push(OwnedUserId, String), // user and pushkey
    Normal(OwnedServerName),
}

impl OutgoingKind {
    pub fn name(&self) -> &'static str {
        match self {
            OutgoingKind::Appservice(_) => "appservice",
            OutgoingKind::Push(_, _) => "push",
            OutgoingKind::Normal(_) => "normal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SendingEventType {
    Pdu(OwnedEventId), // pduid
    Edu(Vec<u8>),      // pdu json
}

pub static MPSC_SENDER: OnceLock<mpsc::UnboundedSender<(OutgoingKind, SendingEventType, i64)>> = OnceLock::new();
pub static MPSC_RECEIVER: OnceLock<Mutex<mpsc::UnboundedReceiver<(OutgoingKind, SendingEventType, i64)>>> =
    OnceLock::new();

pub fn sender() -> mpsc::UnboundedSender<(OutgoingKind, SendingEventType, i64)> {
    MPSC_SENDER.get().expect("sender should set").clone()
}

/// The state for a given state hash.
pub fn max_request() -> Arc<Semaphore> {
    static MAX_REQUESTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    MAX_REQUESTS
        .get_or_init(|| Arc::new(Semaphore::new(crate::config().max_concurrent_requests as usize)))
        .clone()
}

enum TransactionStatus {
    Running,
    Failed(u32, Instant), // number of times failed, time of last failure
    Retrying(u32),        // number of times failed
}

pub fn start_handler() {
    let (sender, receiver) = mpsc::unbounded_channel();
    MPSC_SENDER.set(sender);
    MPSC_RECEIVER.set(Mutex::new(receiver));
    tokio::spawn(async move {
        handler().await.unwrap();
    });
}

async fn handler() -> AppResult<()> {
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
        futures.push(handle_events(outgoing_kind.clone(), events));
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
                                handle_events(
                                    outgoing_kind.clone(),
                                    new_events.into_iter().map(|(_, event)| event).collect(),
                                )
                            );
                        } else {
                            current_transaction_status.remove(&outgoing_kind);
                        }
                    }
                    Err((outgoing_kind, _)) => {
                        current_transaction_status.entry(outgoing_kind).and_modify(|e| *e = match e {
                            TransactionStatus::Running => TransactionStatus::Failed(1, Instant::now()),
                            TransactionStatus::Retrying(n) => TransactionStatus::Failed(*n+1, Instant::now()),
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
                    futures.push(handle_events(outgoing_kind, events));
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
        for (_, e) in active_requests_for(outgoing_kind)?.into_iter() {
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

#[tracing::instrument(skip(server_name))]
pub fn select_edus(server_name: &ServerName) -> AppResult<(Vec<Vec<u8>>, i64)> {
    let mut events = Vec::new();
    let mut max_edu_sn = curr_sn()?;
    let mut device_list_changes = HashSet::new();
    let conf = crate::config();

    // u64: count of last
    let since_sn = curr_sn()?;

    'outer: for room_id in crate::room::server_rooms(server_name)? {
        // Look for device list updates in this room
        device_list_changes.extend(
            crate::room::keys_changed_users(&room_id, since_sn, None)?
                .into_iter()
                .filter(|user_id| user_id.server_name() == &conf.server_name),
        );

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

#[tracing::instrument(skip(servers, pdu_id))]
pub fn send_pdu<S: Iterator<Item = OwnedServerName>>(servers: S, pdu_id: &EventId) -> AppResult<()> {
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

#[tracing::instrument(skip(server, serialized))]
pub fn send_reliable_edu(server: &ServerName, serialized: Vec<u8>, id: &str) -> AppResult<()> {
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
                                        "[Appservice] Event in outgoing_requests not found in database.",
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

            let txn_id = &*general_purpose::URL_SAFE_NO_PAD.encode(utils::hash_keys(
                &events
                    .iter()
                    .map(|e| match e {
                        SendingEventType::Edu(b) => b,
                        SendingEventType::Pdu(b) => b.as_bytes(),
                    })
                    .collect::<Vec<_>>(),
            ));
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

                let rules_for_user = crate::user::get_data::<PushRulesEventContent>(
                    user_id,
                    None,
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

            let max_requst = crate::sending::max_request();
            let permit = max_requst.acquire().await;

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

#[tracing::instrument(skip(request))]
pub async fn send_federation_request<T>(destination: &ServerName, request: reqwest::Request) -> AppResult<T>
where
    T: for<'de> Deserialize<'de> + Debug,
{
    debug!("Waiting for permit");
    let max_request = max_request();
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

    response?.json().await.map_err(Into::into)
}

// #[tracing::instrument(skip(registration, request))]
// pub async fn send_appservice_request<T>(registration: Registration, request: T) -> AppResult<T::IncomingResponse>
// where
//     T: Debug,
// {
//     let permit = acquire_request().await;
//     let response = crate::appservice::send_request(registration, request).await;
//     drop(permit);

//     response
// }

fn active_requests() -> AppResult<Vec<(i64, OutgoingKind, SendingEventType)>> {
    Ok(outgoing_requests::table
        .filter(outgoing_requests::state.eq("pending"))
        .load::<DbOutgoingRequest>(&mut *db::connect()?)?
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
    diesel::delete(outgoing_requests::table.find(id)).execute(&mut *db::connect()?)?;
    Ok(())
}

fn delete_all_active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    diesel::delete(
        outgoing_requests::table
            .filter(outgoing_requests::kind.eq(outgoing_kind.name()))
            .filter(outgoing_requests::state.eq("pending")),
    )
    .execute(&mut *db::connect()?)?;

    Ok(())
}

fn delete_all_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<()> {
    diesel::delete(outgoing_requests::table.filter(outgoing_requests::kind.eq(outgoing_kind.name())))
        .execute(&mut *db::connect()?)?;

    Ok(())
}

fn queue_requests(requests: &[(&OutgoingKind, SendingEventType)]) -> AppResult<Vec<i64>> {
    let mut ids = Vec::new();
    for (outgoing_kind, event) in requests {
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
            .get_result::<i64>(&mut *db::connect()?)?;
        ids.push(id);
    }
    Ok(ids)
}

fn active_requests_for(outgoing_kind: &OutgoingKind) -> AppResult<Vec<(i64, SendingEventType)>> {
    // let prefix = outgoing_kind.get_prefix();
    //     self.servercurrentevent_data
    //         .scan_prefix(prefix)
    //         .map(|(key, v)| parse_servercurrentevent(&key, v).map(|(_, e)| (key, e))),
    // TODO: fixme
    Ok(vec![])
}

fn queued_requests(outgoing_kind: &OutgoingKind) -> AppResult<Vec<(i64, SendingEventType)>> {
    Ok(outgoing_requests::table
        .filter(outgoing_requests::kind.eq(outgoing_kind.name()))
        .load::<DbOutgoingRequest>(&mut *db::connect()?)?
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
            .set((outgoing_requests::data.eq(value), outgoing_requests::state.eq("done")))
            .execute(&mut *db::connect()?)?;
    }

    Ok(())
}
