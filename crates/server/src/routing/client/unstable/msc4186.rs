use salvo::prelude::*;

use crate::{JsonResult, json_ok};

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
pub(super) fn sync_events_v5(_req: &mut Request, _res: &mut Response) -> JsonResult<()> {
    debug_assert!(DEFAULT_BUMP_TYPES.is_sorted(), "DEFAULT_BUMP_TYPES is not sorted");
    let sender_user = body.sender_user.as_ref().expect("user is authenticated");
    let sender_device = body.sender_device.as_ref().expect("user is authenticated");
    let mut body = body.body;

    // Setup watchers, so if there's no response, we can wait for them
    let watcher = services.sync.watch(sender_user, sender_device);

    let next_batch = services.globals.next_count()?;

    let conn_id = body.conn_id.clone();

    let globalsince = body.pos.as_ref().and_then(|string| string.parse().ok()).unwrap_or(0);

    if globalsince != 0
        && !services
            .sync
            .snake_connection_cached(sender_user.clone(), sender_device.clone(), conn_id.clone())
    {
        debug!("Restarting sync stream because it was gone from the database");
        return Err(Error::Request(
            ErrorKind::UnknownPos,
            "Connection data lost since last time".into(),
            http::StatusCode::BAD_REQUEST,
        ));
    }

    // Client / User requested an initial sync
    if globalsince == 0 {
        services
            .sync
            .forget_snake_sync_connection(sender_user.clone(), sender_device.clone(), conn_id.clone());
    }

    // Get sticky parameters from cache
    let known_rooms =
        services
            .sync
            .update_snake_sync_request_with_cache(sender_user.clone(), sender_device.clone(), &mut body);

    let all_joined_rooms: Vec<_> = services
        .rooms
        .state_cache
        .rooms_joined(sender_user)
        .map(ToOwned::to_owned)
        .collect()
        .await;

    let all_invited_rooms: Vec<_> = services
        .rooms
        .state_cache
        .rooms_invited(sender_user)
        .map(|r| r.0)
        .collect()
        .await;

    let all_knocked_rooms: Vec<_> = services
        .rooms
        .state_cache
        .rooms_knocked(sender_user)
        .map(|r| r.0)
        .collect()
        .await;

    let all_rooms: Vec<&RoomId> = all_joined_rooms
        .iter()
        .map(AsRef::as_ref)
        .chain(all_invited_rooms.iter().map(AsRef::as_ref))
        .chain(all_knocked_rooms.iter().map(AsRef::as_ref))
        .collect();

    let all_joined_rooms = all_joined_rooms.iter().map(AsRef::as_ref).collect();
    let all_invited_rooms = all_invited_rooms.iter().map(AsRef::as_ref).collect();

    let pos = next_batch.clone().to_string();

    let mut todo_rooms: TodoRooms = BTreeMap::new();

    let sync_info: SyncInfo<'_> = (sender_user, sender_device, globalsince, &body);
    let mut response = sync_events::v5::Response {
        txn_id: body.txn_id.clone(),
        pos,
        lists: BTreeMap::new(),
        rooms: BTreeMap::new(),
        extensions: sync_events::v5::response::Extensions {
            account_data: collect_account_data(services, sync_info).await,
            e2ee: collect_e2ee(services, sync_info, &all_joined_rooms).await?,
            to_device: collect_to_device(services, sync_info, next_batch).await,
            receipts: collect_receipts(services).await,
            typing: sync_events::v5::response::Typing::default(),
        },
    };

    handle_lists(
        services,
        sync_info,
        &all_invited_rooms,
        &all_joined_rooms,
        &all_rooms,
        &mut todo_rooms,
        &known_rooms,
        &mut response,
    )
    .await;

    fetch_subscriptions(services, sync_info, &known_rooms, &mut todo_rooms).await;

    response.rooms = process_rooms(
        services,
        sender_user,
        next_batch,
        &all_invited_rooms,
        &todo_rooms,
        &mut response,
        &body,
    )
    .await?;

    if response.rooms.iter().all(|(id, r)| {
        r.timeline.is_empty() && r.required_state.is_empty() && !response.extensions.receipts.rooms.contains_key(id)
    }) && response
        .extensions
        .to_device
        .clone()
        .is_none_or(|to| to.events.is_empty())
    {
        // Hang a few seconds so requests are not spammed
        // Stop hanging if new info arrives
        let default = Duration::from_secs(30);
        let duration = cmp::min(body.timeout.unwrap_or(default), default);
        _ = tokio::time::timeout(duration, watcher).await;
    }

    trace!(
        rooms=?response.rooms.len(),
        account_data=?response.extensions.account_data.rooms.len(),
        receipts=?response.extensions.receipts.rooms.len(),
        "responding to request with"
    );
    Ok(response)
}
