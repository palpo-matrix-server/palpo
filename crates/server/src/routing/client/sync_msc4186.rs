use std::cmp::{self, Ordering};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::sync_events::{self, v5::*};
use crate::core::events::receipt::{SyncReceiptEvent, combine_receipt_event_contents};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{AnyRawAccountDataEvent, StateEventType, TimelineEventType};
use crate::data;
use crate::event::ignored_filter;
use crate::room::filter_rooms;
use crate::room::{self, state, timeline};
use crate::routing::prelude::*;
use crate::sync_v3::{DEFAULT_BUMP_TYPES, share_encrypted_room};

/// `POST /_matrix/client/unstable/org.matrix.simplified_msc3575/sync`
/// ([MSC4186])
///
/// A simplified version of sliding sync ([MSC3575]).
///
/// Get all new events in a sliding window of rooms since the last sync or a
/// given point in time.
///
/// [MSC3575]: https://github.com/matrix-org/matrix-spec-proposals/pull/3575
/// [MSC4186]: https://github.com/matrix-org/matrix-spec-proposals/pull/4186
#[handler]
pub(super) async fn sync_events_v5(
    _aa: AuthArgs,
    args: SyncEventsReqArgs,
    req_body: JsonBody<SyncEventsReqBody>,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBody> {
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let device_id = authed.device_id();

    let since_sn: i64 = args
        .pos
        .as_ref()
        .and_then(|string| string.parse().ok())
        .unwrap_or_default();

    let mut req_body = req_body.into_inner();

    let _conn_id = req_body.conn_id.clone();

    if since_sn == 0
        && let Some(conn_id) = &req_body.conn_id
    {
        crate::sync_v5::forget_sync_request_connection(
            sender_id.to_owned(),
            device_id.to_owned(),
            conn_id.to_owned(),
        )
    }

    // Get sticky parameters from cache
    let known_rooms = crate::sync_v5::update_sync_request_with_cache(
        sender_id.to_owned(),
        device_id.to_owned(),
        &mut req_body,
    );

    let mut res_body =
        crate::sync_v5::sync_events(sender_id, device_id, since_sn, &req_body, &known_rooms)
            .await?;

    if since_sn > data::curr_sn()? || (args.pos.is_some() && res_body.is_empty()) {
        // Hang a few seconds so requests are not spammed
        // Stop hanging if new info arrives
        let default = Duration::from_secs(30);
        let duration = cmp::min(args.timeout.unwrap_or(default), default);
        // Setup watchers, so if there's no response, we can wait for them
        let watcher = crate::watcher::watch(sender_id, device_id);
        _ = tokio::time::timeout(duration, watcher).await;
        res_body =
            crate::sync_v5::sync_events(sender_id, device_id, since_sn, &req_body, &known_rooms)
                .await?;
    }

    trace!(
        rooms=?res_body.rooms.len(),
        account_data=?res_body.extensions.account_data.rooms.len(),
        receipts=?res_body.extensions.receipts.rooms.len(),
        "responding to request with"
    );
    json_ok(res_body)
}
