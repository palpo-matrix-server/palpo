use std::collections::BTreeMap;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use ulid::Ulid;

use crate::core::device::DirectDeviceContent;
use crate::core::federation::transaction::Edu;
use crate::core::to_device::{DeviceIdOrAllDevices, SendEventToDeviceReqArgs, SendEventToDeviceReqBody};
use crate::{empty_ok, AuthArgs, DepotExt, EmptyResult, MatrixError};

pub fn authed_router() -> Router {
    Router::with_path("sendToDevice/{event_type}/{txn_id}").put(send_to_device)
}

/// #PUT /_matrix/client/r0/sendToDevice/{event_type}/{txn_id}
/// Send a to-device event to a set of client devices.
#[endpoint]
fn send_to_device(
    _aa: AuthArgs,
    args: SendEventToDeviceReqArgs,
    body: JsonBody<SendEventToDeviceReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    println!("===============send_to_device   0");
    let authed = depot.authed_info()?;
    println!("===============send_to_device   1");
    // Check if this is a new transaction id
    if crate::transaction_id::existing_txn_id(authed.user_id(), Some(authed.device_id()), &args.txn_id)?.is_some() {
        return empty_ok();
    }

    println!("===============send_to_device   2");
    for (target_user_id, map) in &body.messages {
        println!("===============send_to_device   3");
        for (target_device_id_maybe, event) in map {
            println!("===============send_to_device   4");
            if target_user_id.server_name() != &crate::config().server_name {
                println!("===============send_to_device   5");
                let mut map = BTreeMap::new();
                map.insert(target_device_id_maybe.clone(), event.clone());
                let mut messages = BTreeMap::new();
                messages.insert(target_user_id.clone(), map);

                let message_id = Ulid::new();
                crate::sending::send_reliable_edu(
                    target_user_id.server_name(),
                    serde_json::to_vec(&Edu::DirectToDevice(DirectDeviceContent {
                        sender: authed.user_id().clone(),
                        ev_type: args.event_type.clone(),
                        message_id: message_id.to_string().into(),
                        messages,
                    }))
                    .expect("DirectToDevice EDU can be serialized"),
                    &message_id.to_string(),
                )?;

                continue;
            }

            println!("===============send_to_device   6");
            match target_device_id_maybe {
                DeviceIdOrAllDevices::DeviceId(target_device_id) => crate::user::add_to_device_event(
                    authed.user_id(),
                    target_user_id,
                    target_device_id,
                    &args.event_type.to_string(),
                    event
                        .deserialize_as()
                        .map_err(|_| MatrixError::invalid_param("Event is invalid"))?,
                )?,

                DeviceIdOrAllDevices::AllDevices => {
                    println!("===============send_to_device   7");
                    for target_device_id in crate::user::all_device_ids(target_user_id)? {
                        println!("===============send_to_device   8");
                        crate::user::add_to_device_event(
                            authed.user_id(),
                            target_user_id,
                            &target_device_id,
                            &args.event_type.to_string(),
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
    // crate::transaction_id::add_txn_id(authed.user_id(), Some(authed.device_id()), &args.txn_id, &[])?;

    empty_ok()
}

#[endpoint]
pub(super) async fn for_dehydrated(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
