use std::collections::hash_map;

use salvo::prelude::*;

use crate::core::client::sync_events;
use crate::{AppError, AuthArgs, DepotExt, JsonResult, json_ok};

/// #GET /_matrix/client/r0/sync
/// Synchronize the client's state with the latest state on the server.
///
/// - This endpoint takes a `since` parameter which should be the `next_batch` value from a
/// previous request for incremental syncs.
///
/// Calling this endpoint without a `since` parameter returns:
/// - Some of the most recent events of each timeline
/// - Notification counts for each room
/// - Joined and invited member counts, heroes
/// - All state events
///
/// Calling this endpoint with a `since` parameter from a previous `next_batch` returns:
/// For joined rooms:
/// - Some of the most recent events of each timeline that happened after since
/// - If user joined the room after since: All state events (unless lazy loading is activated) and
/// all device list updates in that room
/// - If the user was already in the room: A list of all events that are in the state now, but were
/// not in the state at `since`
/// - If the state we send contains a member event: Joined and invited member counts, heroes
/// - Device list updates that happened after `since`
/// - If there are events in the timeline we send or the user send updated his read mark: Notification counts
/// - EDUs that are active now (read receipts, typing updates, presence)
/// - TODO: Allow multiple sync streams to support Pantalaimon
///
/// For invited rooms:
/// - If the user was invited after `since`: A subset of the state of the room at the point of the invite
///
/// For left rooms:
/// - If the user left after `since`: prev_batch token, empty state (TODO: subset of the state at the point of the leave)
///
/// - Sync is handled in an async task, multiple requests from the same device with the same
/// `since` will be cached
#[endpoint]
pub(super) async fn sync_events_v3(
    _aa: AuthArgs,
    args: sync_events::v3::SyncEventsReqArgs,
    depot: &mut Depot,
) -> JsonResult<sync_events::v3::SyncEventsResBody> {
    let authed = depot.authed_info()?.clone();
    let mut rx = match crate::SYNC_RECEIVERS
        .write()
        .unwrap()
        .entry((authed.user_id().clone(), authed.device_id().clone()))
    {
        hash_map::Entry::Vacant(v) => {
            let (tx, rx) = tokio::sync::watch::channel(None);
            v.insert((args.since.clone(), rx.clone()));
            tokio::spawn({
                let user_id = authed.user_id().to_owned();
                let device_id = authed.device_id().to_owned();
                crate::user::ping_presence(&user_id, &args.set_presence)?;
                async move {
                    if let Err(e) = crate::sync_v3::sync_events(user_id, device_id, args, tx).await {
                        tracing::error!(error = ?e, "sync_events error 1");
                    }
                }
            });
            rx
        }
        hash_map::Entry::Occupied(mut o) => {
            if o.get().0 != args.since || args.since.is_none() {
                let (tx, rx) = tokio::sync::watch::channel(None);
                if args.since.is_some() {
                    o.insert((args.since.clone(), rx.clone()));
                }
                tokio::spawn({
                    let user_id = authed.user_id().to_owned();
                    let device_id = authed.device_id().to_owned();
                    crate::user::ping_presence(&user_id, &args.set_presence)?;
                    async move {
                        if let Err(e) = crate::sync_v3::sync_events(user_id, device_id, args, tx).await {
                            tracing::error!(error = ?e, "sync_events error 2");
                        }
                    }
                });
                rx
            } else {
                o.get().1.clone()
            }
        }
    };

    let we_have_to_wait = rx.borrow().is_none();
    if we_have_to_wait {
        if let Err(e) = rx.changed().await {
            error!("Error waiting for sync: {}", e);
        }
    }

    let result = match rx
        .borrow()
        .as_ref()
        .expect("When sync channel changes it's always set to some")
    {
        Ok(response) => json_ok(response.clone()),
        Err(error) => Err(AppError::public(error.to_string())),
    };
    result
}
