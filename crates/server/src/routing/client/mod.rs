mod account;
mod admin;
mod appservice;
mod auth;
mod device;
mod directory;
mod key;
mod presence;
mod profile;
mod push_rule;
mod pusher;
mod register;
mod room;
mod room_key;
mod session;
mod third_party;
mod to_device;
mod unstable;
mod user;
mod user_directory;
mod voip;

pub(crate) mod media;

use std::collections::{BTreeMap, BTreeSet, HashSet, hash_map};
use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::UserId;
use crate::core::client::discovery::{
    Capabilities, CapabilitiesResBody, RoomVersionStability, RoomVersionsCapability, VersionsResBody,
};
use crate::core::client::search::{ResultCategories, SearchReqArgs, SearchReqBody, SearchResBody};
use crate::core::client::sync_events::{
    AccountDataV4, E2eeV4, ExtensionsV4, ReceiptsV4, SlidingOpV4, SyncEventsReqArgsV3, SyncEventsReqArgsV4,
    SyncEventsReqBodyV4, SyncEventsResBodyV3, SyncEventsResBodyV4, SyncListV4, SyncOpV4, ToDeviceV4, TypingV4,
    UnreadNotificationsCount,
};
use crate::core::device::DeviceLists;
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::{AppError, AuthArgs, DepotExt, EmptyResult, JsonResult, empty_ok, hoops, json_ok};

pub fn router() -> Router {
    let mut client = Router::with_path("client").oapi_tag("client");
    for v in ["v3", "v1", "r0"] {
        client = client
            .push(
                Router::with_path(v)
                    .push(account::public_router())
                    .push(profile::public_router())
                    .push(register::public_router())
                    .push(session::public_router())
                    .push(room::public_router())
                    .push(directory::public_router())
                    .push(media::self_auth_router())
                    .push(
                        Router::with_path("publicRooms")
                            .get(room::get_public_rooms)
                            .post(room::get_filtered_public_rooms),
                    ),
            )
            .push(
                Router::with_path(v)
                    .hoop(hoops::auth_by_access_token)
                    .push(account::authed_router())
                    .push(register::authed_router())
                    .push(session::authed_router())
                    .push(device::authed_router())
                    .push(room_key::authed_router())
                    .push(room::authed_router())
                    .push(user::authed_router())
                    .push(directory::authed_router())
                    .push(user_directory::authed_router())
                    .push(key::authed_router())
                    .push(profile::authed_router())
                    .push(voip::authed_router())
                    .push(appservice::authed_router())
                    .push(admin::authed_router())
                    .push(third_party::authed_router())
                    .push(to_device::authed_router())
                    .push(auth::authed_router())
                    .push(pusher::authed_router())
                    .push(push_rule::authed_router())
                    .push(presence::authed_router())
                    .push(Router::with_path("joined_rooms").get(room::membership::joined_rooms))
                    .push(Router::with_path("join/{room_id_or_alias}").post(room::membership::join_room_by_id_or_alias))
                    .push(Router::with_path("createRoom").post(room::create_room))
                    .push(Router::with_path("notifications").get(get_notifications))
                    .push(Router::with_path("sync").get(sync_events_v3))
                    .push(
                        Router::with_path("dehydrated_device")
                            .get(device::dehydrated)
                            .put(device::upsert_dehydrated)
                            .delete(device::delete_dehydrated)
                            .push(Router::with_path("{device_id}/events").post(to_device::for_dehydrated)),
                    ),
            )
            .push(
                Router::with_path(v)
                    .hoop(hoops::limit_rate)
                    .hoop(hoops::auth_by_access_token)
                    .push(Router::with_path("search").post(search))
                    .push(Router::with_path("capabilities").get(get_capabilities))
                    .push(Router::with_path("knock/{room_id_or_alias}").post(room::knock_room)),
            )
    }
    client
        .push(Router::with_path("versions").get(supported_versions))
        .push(unstable::router())
}

/// #POST /_matrix/client/r0/search
/// Searches rooms for messages.
///
/// - Only works if the user is currently joined to the room (TODO: Respect history visibility)
#[endpoint]
fn search(
    _aa: AuthArgs,
    args: SearchReqArgs,
    body: JsonBody<SearchReqBody>,
    depot: &mut Depot,
) -> JsonResult<SearchResBody> {
    let authed = depot.authed_info()?;

    let search_criteria = body.search_categories.room_events.as_ref().unwrap();
    let room_events =
        crate::event::search::search_pdus(authed.user_id(), &search_criteria, args.next_batch.as_deref())?;
    json_ok(SearchResBody::new(ResultCategories { room_events }))
}

/// #GET /_matrix/client/r0/capabilities
/// Get information on the supported feature set and other relevent capabilities of this server.
#[endpoint]
async fn get_capabilities(_aa: AuthArgs) -> JsonResult<CapabilitiesResBody> {
    let mut available = BTreeMap::new();
    for room_version in &*crate::UNSTABLE_ROOM_VERSIONS {
        available.insert(room_version.clone(), RoomVersionStability::Unstable);
    }
    for room_version in &*crate::STABLE_ROOM_VERSIONS {
        available.insert(room_version.clone(), RoomVersionStability::Stable);
    }
    json_ok(CapabilitiesResBody {
        capabilities: Capabilities {
            room_versions: RoomVersionsCapability {
                default: crate::default_room_version(),
                available,
            },
            ..Default::default()
        },
    })
}

/// #GET /_matrix/client/versions
/// Get the versions of the specification and unstable features supported by this server.
///
/// - Versions take the form MAJOR.MINOR.PATCH
/// - Only the latest PATCH release will be reported for each MAJOR.MINOR value
/// - Unstable features are namespaced and may include version information in their name
///
/// Note: Unstable features are used while developing new features. Clients should avoid using
/// unstable features in their stable releases
#[endpoint]
async fn supported_versions() -> JsonResult<VersionsResBody> {
    json_ok(VersionsResBody {
        versions: vec![
            "r0.5.0".to_owned(),
            "r0.6.0".to_owned(),
            "v1.1".to_owned(),
            "v1.2".to_owned(),
            "v1.3".to_owned(),
            "v1.4".to_owned(),
            "v1.5".to_owned(),
        ],
        unstable_features: BTreeMap::from_iter([("org.matrix.e2e_cross_signing".to_owned(), true)]),
    })
}

#[endpoint]
async fn get_notifications(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    // TODO: get_notifications
    let _authed = depot.authed_info()?;
    empty_ok()
}

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
async fn sync_events_v3(
    _aa: AuthArgs,
    args: SyncEventsReqArgsV3,
    depot: &mut Depot,
) -> JsonResult<SyncEventsResBodyV3> {
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
                    if let Err(e) = crate::sync::sync_events(user_id, device_id, args, tx).await {
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
                        if let Err(e) = crate::sync::sync_events(user_id, device_id, args, tx).await {
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
