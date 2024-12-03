use std::collections::BTreeMap;
use std::time::Instant;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use tokio::sync::RwLock;

use crate::core::canonical_json::CanonicalJsonObject;
use crate::core::device::{DeviceListUpdateContent, DirectDeviceContent};
use crate::core::events::receipt::{ReceiptEvent, ReceiptEventContent, ReceiptType};
use crate::core::federation::transaction::{Edu, SigningKeyUpdateContent};
use crate::core::federation::transaction::{SendMessageReqBody, SendMessageResBody};
use crate::core::identifiers::*;
use crate::core::to_device::DeviceIdOrAllDevices;
use crate::core::UnixMillis;
use crate::user::NewDbPresence;
use crate::{json_ok, AppError, AuthArgs, DepotExt, JsonResult, MatrixError};

pub fn router() -> Router {
    Router::with_path("send/<txn_id>").put(send_message)
}

// #PUT /_matrix/federation/v1/send/{txn_id}
/// Push EDUs and PDUs to this server.
#[endpoint]
async fn send_message(
    _aa: AuthArgs,
    txn_id: PathParam<OwnedTransactionId>,
    body: JsonBody<SendMessageReqBody>,
    depot: &mut Depot,
) -> JsonResult<SendMessageResBody> {
    let server_name = &crate::config().server_name;
    let mut resolved_map = BTreeMap::new();

    // let pub_key_map = RwLock::new(BTreeMap::new());

    // This is all the auth_events that have been recursively fetched so they don't have to be
    // deserialized over and over again.
    // TODO: make this persist across requests but not in a DB Tree (in globals?)
    // TODO: This could potentially also be some sort of trie (suffix tree) like structure so
    // that once an auth event is known it would know (using indexes maybe) all of the auth
    // events that it references.
    // let mut auth_cache = EventMap::new();

    for pdu in &body.pdus {
        let value: CanonicalJsonObject = serde_json::from_str(pdu.get()).map_err(|e| {
            warn!("Error parsing incoming event {:?}: {:?}", pdu, e);
            AppError::public("Invalid PDU in server response")
        })?;
        let room_id: OwnedRoomId = value
            .get("room_id")
            .and_then(|id| RoomId::parse(id.as_str()?).ok())
            .ok_or(MatrixError::invalid_param("Invalid room id in pdu"))?;

        if crate::room::state::get_room_version(&room_id).is_err() {
            debug!("Server is not in room {room_id}");
            continue;
        }

        let r = crate::parse_incoming_pdu(&pdu);

        let (event_id, value, room_id) = match r {
            Ok(t) => t,
            Err(e) => {
                warn!("Could not parse PDU: {e}");
                warn!("Full PDU: {:?}", &pdu);
                continue;
            }
        };
        // We do not add the event_id field to the pdu here because of signature and hashes checks
        let start_time = Instant::now();
        resolved_map.insert(
            event_id.clone(),
            crate::event::handler::handle_incoming_pdu(&server_name, &event_id, &room_id, value, true).await,
        );

        let elapsed = start_time.elapsed();
        debug!(
            "Handling transaction of event {} took {}m{}s",
            event_id,
            elapsed.as_secs() / 60,
            elapsed.as_secs() % 60
        );
    }

    // for pdu in &resolved_map {
    //     if let Err(e) = pdu.1 {
    //         if matches!(e, MatrixError::not_found(_)) {
    //             warn!("Incoming PDU failed {:?}", pdu);
    //         }
    //     }
    // }

    for edu in &body.edus {
        match edu {
            Edu::Presence(presence) => {
                if !crate::allow_incoming_presence() {
                    continue;
                }

                for update in &presence.push {
                    crate::user::set_presence(
                        NewDbPresence {
                            user_id: update.user_id.clone(),
                            stream_id: None,
                            state: Some(update.presence.to_string()),
                            status_msg: update.status_msg.clone(),
                            last_active_at: Some(UnixMillis(update.last_active_ago)),
                            last_federation_update_at: None,
                            last_user_sync_at: None,
                            currently_active: Some(update.currently_active),
                            occur_sn: None,
                        },
                        true,
                    )?;
                    // for room_id in crate::user::joined_rooms(&update.user_id, 0)? {
                    //     crate::user::set_presence(NewDbPresence {
                    //         user_id: update.user_id.clone(),
                    //         // room_id: Some(room_id),
                    //         stream_id: None,
                    //         state: Some(update.presence.to_string()),
                    //         status_msg: update.status_msg.clone(),
                    //         last_active_at: Some(UnixMillis(update.last_active_ago)),
                    //         last_federation_update_at: None,
                    //         last_user_sync_at: None,
                    //         currently_active: Some(update.currently_active),
                    //     })?;
                    // }
                }
            }
            Edu::Receipt(receipt) => {
                for (room_id, room_updates) in &receipt.0 {
                    for (user_id, user_updates) in &room_updates.read {
                        if let Some((event_id, _)) = user_updates
                            .event_ids
                            .iter()
                            .filter_map(|id| crate::room::timeline::get_event_sn(id).ok().flatten().map(|r| (id, r)))
                            .max_by_key(|(_, count)| *count)
                        {
                            let mut user_receipts = BTreeMap::new();
                            user_receipts.insert(user_id.clone(), user_updates.data.clone());

                            let mut receipts = BTreeMap::new();
                            receipts.insert(ReceiptType::Read, user_receipts);

                            let mut receipt_content = BTreeMap::new();
                            receipt_content.insert(event_id.to_owned(), receipts);

                            let event = ReceiptEvent {
                                content: ReceiptEventContent(receipt_content),
                                room_id: room_id.clone(),
                            };
                            crate::room::receipt::update_read(&user_id, &room_id, event)?;
                        } else {
                            // TODO fetch missing events
                            debug!("No known event ids in read receipt: {:?}", user_updates);
                        }
                    }
                }
            }
            Edu::Typing(typing) => {
                if crate::room::is_joined(&typing.user_id, &typing.room_id)? {
                    if typing.typing {
                        crate::room::typing::add_typing(
                            &typing.user_id,
                            &typing.room_id,
                            3000 + UnixMillis::now().get(),
                        )
                        .await?;
                    } else {
                        crate::room::typing::remove_typing(&typing.user_id, &typing.room_id).await?;
                    }
                }
            }
            Edu::DeviceListUpdate(DeviceListUpdateContent { user_id, .. }) => {
                crate::user::mark_device_key_update(&user_id)?;
            }
            Edu::DirectToDevice(DirectDeviceContent {
                sender,
                ev_type,
                message_id,
                messages,
            }) => {
                // Check if this is a new transaction id
                if crate::transaction_id::existing_txn_id(&sender, None, &message_id)?.is_some() {
                    continue;
                }

                for (target_user_id, map) in messages {
                    for (target_device_id_maybe, event) in map {
                        match target_device_id_maybe {
                            DeviceIdOrAllDevices::DeviceId(target_device_id) => crate::user::add_to_device_event(
                                &sender,
                                target_user_id,
                                &target_device_id,
                                &ev_type.to_string(),
                                event.deserialize_as().map_err(|e| {
                                    warn!("To-Device event is invalid: {event:?} {e}");
                                    MatrixError::invalid_param("Event is invalid")
                                })?,
                            )?,

                            DeviceIdOrAllDevices::AllDevices => {
                                for target_device_id in crate::user::all_device_ids(target_user_id)? {
                                    crate::user::add_to_device_event(
                                        &sender,
                                        target_user_id,
                                        &target_device_id,
                                        &ev_type.to_string(),
                                        event
                                            .deserialize_as()
                                            .map_err(|_| MatrixError::invalid_param("Event is invalid"))?,
                                    )?;
                                }
                            }
                        }
                    }
                }

                // Save transaction id with empty data
                // crate::transaction_id::add_txn_id(&sender, None, &message_id, &[])?;
            }
            Edu::SigningKeyUpdate(SigningKeyUpdateContent {
                user_id,
                master_key,
                self_signing_key,
            }) => {
                if user_id.server_name() != server_name {
                    continue;
                }
                if let Some(master_key) = master_key {
                    crate::user::add_cross_signing_keys(&user_id, &master_key, &self_signing_key, &None, true)?;
                }
            }
            Edu::_Custom(_) => {}
        }
    }

    json_ok(SendMessageResBody {
        pdus: resolved_map
            .into_iter()
            .map(|(e, r)| (e, r.map_err(|e| e.to_string())))
            .collect(),
    })
}
