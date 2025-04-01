mod event;
pub(super) mod membership;
mod message;
mod receipt;
mod relation;
mod state;
mod tag;
mod thread;
pub(crate) use membership::knock_room;

use std::cmp::max;
use std::collections::BTreeMap;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::json;
use serde_json::value::to_raw_value;

use crate::core::UnixMillis;
use crate::core::client::directory::{PublicRoomsFilteredReqBody, PublicRoomsReqArgs};
use crate::core::client::room::CreateRoomResBody;
use crate::core::client::room::{
    AliasesResBody, CreateRoomReqBody, RoomPreset, SetReadMarkerReqBody, UpgradeRoomReqBody, UpgradeRoomResBody,
};
use crate::core::client::space::{HierarchyReqArgs, HierarchyResBody};
use crate::core::directory::{PublicRoomFilter, PublicRoomsResBody, RoomNetwork};
use crate::core::events::receipt::{Receipt, ReceiptEvent, ReceiptEventContent, ReceiptThread, ReceiptType};
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::GuestAccess;
use crate::core::events::room::guest_access::RoomGuestAccessEventContent;
use crate::core::events::room::history_visibility::HistoryVisibility;
use crate::core::events::room::history_visibility::RoomHistoryVisibilityEventContent;
use crate::core::events::room::join_rules::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::events::room::tombstone::RoomTombstoneEventContent;
use crate::core::events::room::topic::RoomTopicEventContent;
use crate::core::events::{RoomAccountDataEventType, StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::room::Visibility;
use crate::core::serde::{CanonicalJsonObject, JsonValue};
use crate::event::PduBuilder;
use crate::{AppError, AppResult, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError, empty_ok, hoops, json_ok};

pub fn public_router() -> Router {
    Router::with_path("rooms")
        .push(Router::with_path("{room_id}").push(Router::with_path("initialSync").get(initial_sync)))
}
pub fn authed_router() -> Router {
    Router::with_path("rooms")
        .push(
            Router::with_hoop(hoops::limit_rate).push(
                Router::with_path("{room_id}")
                    .push(Router::with_path("forget").post(membership::forget_room))
                    .push(Router::with_path("leave").post(membership::leave_room))
                    .push(Router::with_path("join").post(membership::join_room_by_id))
                    .push(Router::with_path("invite").post(membership::invite_user))
                    .push(Router::with_path("read_markers").post(set_read_markers))
                    .push(Router::with_path("aliases").get(get_aliases))
                    .push(Router::with_path("hierarchy").get(get_hierarchy))
                    .push(Router::with_path("threads").get(thread::list_threads))
                    .push(Router::with_path("typing/{user_id}").put(state::send_typing))
                    .push(
                        Router::with_path("receipt/{receipt_type}/{event_id}")
                            .post(receipt::send_receipt)
                            .put(receipt::send_receipt),
                    )
                    .push(Router::with_path("timestamp_to_event").get(event::timestamp_to_event)),
            ),
        )
        .push(
            Router::with_hoop(hoops::limit_rate).push(
                Router::with_path("{room_id}")
                    .push(Router::with_path("ban").post(membership::ban_user))
                    .push(Router::with_path("unban").post(membership::unban_user))
                    .push(Router::with_path("kick").post(membership::kick_user))
                    .push(Router::with_path("members").get(membership::get_members))
                    .push(Router::with_path("joined_members").get(membership::joined_members))
                    .push(
                        Router::with_path("state").get(state::get_state).push(
                            Router::with_path("{event_type}")
                                .put(state::send_state_for_empty_key)
                                .get(state::state_for_empty_key)
                                .push(
                                    Router::with_path("{state_key}")
                                        .put(state::send_state_for_key)
                                        .get(state::state_for_key),
                                ),
                        ),
                    )
                    .push(Router::with_path("context").push(Router::with_path("{event_id}").get(event::get_context)))
                    .push(
                        Router::with_path("relations").push(
                            Router::with_path("{event_id}").get(relation::get_relation).push(
                                Router::with_path("{rel_type}")
                                    .get(relation::get_relation_by_rel_type)
                                    .push(
                                        Router::with_path("{event_type}")
                                            .get(relation::get_relation_by_rel_type_and_event_type),
                                    ),
                            ),
                        ),
                    )
                    .push(Router::with_path("upgrade").post(upgrade))
                    .push(Router::with_path("messages").get(message::get_messages))
                    .push(Router::with_path("send/{event_type}").post(message::post_message))
                    .push(Router::with_path("send/{event_type}/{txn_id}").put(message::send_message))
                    .push(Router::with_path("redact/{event_id}/{txn_id}").put(event::send_redact))
                    .push(
                        Router::with_path("tags")
                            .get(tag::list_tags)
                            .push(Router::with_path("{tag}").put(tag::upsert_tag).delete(tag::delete_tag)),
                    )
                    .push(
                        Router::with_path("event").push(
                            Router::with_path("{event_id}")
                                .get(event::get_room_event)
                                .post(event::report),
                        ),
                    ),
            ),
        )
}

#[endpoint]
async fn initial_sync(_aa: AuthArgs) -> EmptyResult {
    empty_ok()
}
/// #POST /_matrix/client/r0/rooms/{room_id}/read_markers
/// Sets different types of read markers.
///
/// - Updates fully-read account data event to `fully_read`
/// - If `read_receipt` is set: Update private marker and public read receipt EDU
#[endpoint]
fn set_read_markers(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    body: JsonBody<SetReadMarkerReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;
    let room_id = room_id.into_inner();
    if let Some(fully_read) = &body.fully_read {
        let fully_read_event = crate::core::events::fully_read::FullyReadEvent {
            content: crate::core::events::fully_read::FullyReadEventContent {
                event_id: fully_read.clone(),
            },
        };
        crate::user::set_data(
            authed.user_id(),
            Some(room_id.clone()),
            &RoomAccountDataEventType::FullyRead.to_string(),
            serde_json::to_value(fully_read_event.content).expect("to json value always works"),
        )?;
    }

    if body.private_read_receipt.is_some() || body.read_receipt.is_some() {
        crate::room::user::reset_notification_counts(authed.user_id(), &room_id)?;
    }

    if let Some(event_id) = &body.private_read_receipt {
        let event_sn = crate::event::ensure_event_sn(&room_id, &event_id)?;
        crate::room::receipt::set_private_read(&room_id, authed.user_id(), event_id, event_sn)?;
    }

    if let Some(event) = &body.read_receipt {
        let mut user_receipts = BTreeMap::new();
        user_receipts.insert(
            authed.user_id().clone(),
            Receipt {
                ts: Some(UnixMillis::now()),
                thread: ReceiptThread::Unthreaded,
            },
        );

        let mut receipts = BTreeMap::new();
        receipts.insert(ReceiptType::Read, user_receipts);

        let mut receipt_content = BTreeMap::new();
        receipt_content.insert(event.to_owned(), receipts);

        crate::room::receipt::update_read(
            authed.user_id(),
            &room_id,
            ReceiptEvent {
                content: ReceiptEventContent(receipt_content),
                room_id: room_id.clone(),
            },
        )?;
    }
    empty_ok()
}

/// #GET /_matrix/client/r0/rooms/{room_id}/aliases
/// Lists all aliases of the room.
///
/// - Only users joined to the room are allowed to call this
/// TODO: Allow any user to call it if history_visibility is world readable
#[endpoint]
async fn get_aliases(_aa: AuthArgs, room_id: PathParam<OwnedRoomId>, depot: &mut Depot) -> JsonResult<AliasesResBody> {
    let authed = depot.authed_info()?;

    if !crate::room::is_joined(authed.user_id(), &room_id)? {
        return Err(MatrixError::forbidden("You don't have permission to view this room.").into());
    }

    json_ok(AliasesResBody {
        aliases: crate::room::local_aliases_for_room(&room_id)?,
    })
}

/// #GET /_matrix/client/v1/rooms/{room_id}/hierarchy``
/// Paginates over the space tree in a depth-first manner to locate child rooms of a given space.
#[endpoint]
async fn get_hierarchy(_aa: AuthArgs, args: HierarchyReqArgs, depot: &mut Depot) -> JsonResult<HierarchyResBody> {
    let authed = depot.authed_info()?;
    let skip = args.from.as_ref().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let limit = args.limit.unwrap_or(10).min(100) as usize;
    let max_depth = args.max_depth.map_or(3, u64::from).min(10) + 1; // +1 to skip the space room itself
    let body = crate::room::space::get_hierarchy(
        authed.user_id(),
        &args.room_id,
        limit,
        skip,
        max_depth,
        args.suggested_only,
    )
    .await?;
    json_ok(body)
}

/// #POST /_matrix/client/r0/rooms/{room_id}/upgrade
/// Upgrades the room.
///
/// - Creates a replacement room
/// - Sends a tombstone event into the current room
/// - Sender user joins the room
/// - Transfers some state events
/// - Moves local aliases
/// - Modifies old room power levels to prevent users from speaking
#[endpoint]
async fn upgrade(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    body: JsonBody<UpgradeRoomReqBody>,
    depot: &mut Depot,
) -> JsonResult<UpgradeRoomResBody> {
    let authed = depot.authed_info()?;
    let room_id = room_id.into_inner();

    if !crate::supported_room_versions().contains(&body.new_version) {
        return Err(MatrixError::unsupported_room_version("This server does not support that room version.").into());
    }

    // Create a replacement room
    let replacement_room = RoomId::new(crate::server_name());
    crate::room::ensure_room(&replacement_room, &crate::default_room_version())?;

    // Send a m.room.tombstone event to the old room to indicate that it is not intended to be used any further
    // Fail if the sender does not have the required permissions
    let tombstone_event_id = crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomTombstone,
            content: to_raw_value(&RoomTombstoneEventContent {
                body: "This room has been replaced".to_owned(),
                replacement_room: replacement_room.clone(),
            })?,
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?
    .event_id;

    // Get the old room creation event
    let mut create_event_content = serde_json::from_str::<CanonicalJsonObject>(
        crate::room::state::get_room_state(&room_id, &StateEventType::RoomCreate, "")?
            .ok_or_else(|| AppError::internal("Found room without m.room.create event."))?
            .content
            .get(),
    )
    .map_err(|_| AppError::internal("Invalid room event in database."))?;

    // Use the m.room.tombstone event as the predecessor
    let predecessor = Some(crate::core::events::room::create::PreviousRoom::new(
        room_id.clone(),
        (*tombstone_event_id).to_owned(),
    ));

    // Send a m.room.create event containing a predecessor field and the applicable room_version
    create_event_content.insert(
        "creator".into(),
        json!(&authed.user_id())
            .try_into()
            .map_err(|_| MatrixError::bad_json("Error forming creation event"))?,
    );
    create_event_content.insert(
        "room_version".into(),
        json!(&body.new_version)
            .try_into()
            .map_err(|_| MatrixError::bad_json("Error forming creation event"))?,
    );
    create_event_content.insert(
        "predecessor".into(),
        json!(predecessor)
            .try_into()
            .map_err(|_| MatrixError::bad_json("Error forming creation event"))?,
    );

    // Validate creation event content
    let de_result = serde_json::from_str::<CanonicalJsonObject>(
        to_raw_value(&create_event_content)
            .expect("Error forming creation event")
            .get(),
    );

    if de_result.is_err() {
        return Err(MatrixError::bad_json("Error forming creation event").into());
    }

    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomCreate,
            content: to_raw_value(&create_event_content).expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &replacement_room,
    )?;

    // Join the new room
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content: to_raw_value(&RoomMemberEventContent {
                membership: MembershipState::Join,
                display_name: crate::user::display_name(authed.user_id())?,
                avatar_url: crate::user::avatar_url(authed.user_id())?,
                is_direct: None,
                third_party_invite: None,
                blurhash: crate::user::blurhash(authed.user_id())?,
                reason: None,
                join_authorized_via_users_server: None,
            })
            .expect("event is valid, we just created it"),
            state_key: Some(authed.user_id().to_string()),
            ..Default::default()
        },
        authed.user_id(),
        &replacement_room,
    )?;

    // Recommended transferable state events list from the specs
    let transferable_state_events = vec![
        StateEventType::RoomServerAcl,
        StateEventType::RoomEncryption,
        StateEventType::RoomName,
        StateEventType::RoomAvatar,
        StateEventType::RoomTopic,
        StateEventType::RoomGuestAccess,
        StateEventType::RoomHistoryVisibility,
        StateEventType::RoomJoinRules,
        StateEventType::RoomPowerLevels,
    ];

    // Replicate transferable state events to the new room
    for event_ty in transferable_state_events {
        let event_content = match crate::room::state::get_room_state(&room_id, &event_ty, "")? {
            Some(v) => v.content.clone(),
            None => continue, // Skipping missing events.
        };

        crate::room::timeline::build_and_append_pdu(
            PduBuilder {
                event_type: event_ty.to_string().into(),
                content: event_content,
                state_key: Some("".to_owned()),
                ..Default::default()
            },
            authed.user_id(),
            &replacement_room,
        )?;
    }

    // Moves any local aliases to the new room
    for alias in crate::room::local_aliases_for_room(&room_id)? {
        crate::room::set_alias(&replacement_room, &alias, authed.user_id())?;
    }

    // Get the old room power levels
    let mut power_levels_event_content: RoomPowerLevelsEventContent = serde_json::from_str(
        crate::room::state::get_room_state(&room_id, &StateEventType::RoomPowerLevels, "")?
            .ok_or_else(|| AppError::internal("Found room without m.room.create event."))?
            .content
            .get(),
    )
    .map_err(|_| AppError::internal("Invalid room event in database."))?;

    // Setting events_default and invite to the greater of 50 and users_default + 1
    let new_level = max(50, power_levels_event_content.users_default + 1);
    power_levels_event_content.events_default = new_level;
    power_levels_event_content.invite = new_level;

    // Modify the power levels in the old room to prevent sending of events and inviting new users
    let _ = crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomPowerLevels,
            content: to_raw_value(&power_levels_event_content).expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;
    // Return the replacement room id
    json_ok(UpgradeRoomResBody { replacement_room })
}

/// #GET /_matrix/client/r0/publicRooms
/// Lists the public rooms on this server.
///
/// - Rooms are ordered by the number of joined members
#[endpoint]
pub(super) async fn get_public_rooms(_aa: AuthArgs, args: PublicRoomsReqArgs) -> JsonResult<PublicRoomsResBody> {
    let body = crate::directory::get_public_rooms(
        args.server.as_deref(),
        args.limit,
        args.since.as_deref(),
        &PublicRoomFilter::default(),
        &RoomNetwork::Matrix,
    )
    .await?;
    json_ok(body)
}
/// #POST /_matrix/client/r0/publicRooms
/// Lists the public rooms on this server.
///
/// - Rooms are ordered by the number of joined members
#[endpoint]
pub(super) async fn get_filtered_public_rooms(
    _aa: AuthArgs,
    args: JsonBody<PublicRoomsFilteredReqBody>,
) -> JsonResult<PublicRoomsResBody> {
    let body = crate::directory::get_public_rooms(
        args.server.as_deref(),
        args.limit,
        args.since.as_deref(),
        &args.filter,
        &args.room_network,
    )
    .await?;
    json_ok(body)
}

/// #POST /_matrix/client/r0/createRoom
/// Creates a new room.
///
/// - Room ID is randomly generated
/// - Create alias if room_alias_name is set
/// - Send create event
/// - Join sender user
/// - Send power levels event
/// - Send canonical room alias
/// - Send join rules
/// - Send history visibility
/// - Send guest access
/// - Send events listed in initial state
/// - Send events implied by `name` and `topic`
/// - Send invite events
#[endpoint]
pub(super) async fn create_room(
    _aa: AuthArgs,
    body: JsonBody<CreateRoomReqBody>,
    depot: &mut Depot,
) -> JsonResult<CreateRoomResBody> {
    let authed = depot.authed_info()?;
    let room_id = RoomId::new(crate::server_name());
    crate::room::ensure_room(&room_id, &crate::default_room_version())?;

    if !crate::allow_room_creation() && authed.appservice.is_none() && !authed.is_admin() {
        return Err(MatrixError::forbidden("Room creation has been disabled.").into());
    }

    let alias: Option<OwnedRoomAliasId> = if let Some(localpart) = &body.room_alias_name {
        // TODO: Check for invalid characters and maximum length
        let alias = RoomAliasId::parse(format!("#{}:{}", localpart, crate::server_name()))
            .map_err(|_| MatrixError::invalid_param("Invalid alias."))?;

        if crate::room::resolve_local_alias(&alias)?.is_some() {
            return Err(MatrixError::room_in_use("Room alias already exists.").into());
        } else {
            Some(alias)
        }
    } else {
        None
    };

    let room_version = match body.room_version.clone() {
        Some(room_version) => {
            if crate::supported_room_versions().contains(&room_version) {
                room_version
            } else {
                return Err(
                    MatrixError::unsupported_room_version("This server does not support that room version.").into(),
                );
            }
        }
        None => crate::default_room_version(),
    };

    let content = match &body.creation_content {
        Some(content) => {
            let mut content = content
                .deserialize_as::<CanonicalJsonObject>()
                .expect("Invalid creation content");
            content.insert(
                "creator".into(),
                json!(&authed.user_id())
                    .try_into()
                    .map_err(|_| MatrixError::bad_json("Invalid creation content"))?,
            );
            content.insert(
                "room_version".into(),
                json!(room_version.as_str())
                    .try_into()
                    .map_err(|_| MatrixError::bad_json("Invalid creation content"))?,
            );
            content
        }
        None => {
            // TODO: Add correct value for v11
            let mut content = serde_json::from_str::<CanonicalJsonObject>(
                to_raw_value(&RoomCreateEventContent::new_v1(authed.user_id().clone()))
                    .map_err(|_| MatrixError::bad_json("Invalid creation content"))?
                    .get(),
            )
            .unwrap();
            content.insert(
                "room_version".into(),
                json!(room_version.as_str())
                    .try_into()
                    .map_err(|_| MatrixError::bad_json("Invalid creation content"))?,
            );
            content
        }
    };

    // Validate creation content
    let de_result =
        serde_json::from_str::<CanonicalJsonObject>(to_raw_value(&content).expect("Invalid creation content").get());

    if de_result.is_err() {
        return Err(MatrixError::bad_json("Invalid creation content").into());
    }

    // 1. The room create event
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomCreate,
            content: to_raw_value(&content).expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 2. Let the room creator join
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomMember,
            content: to_raw_value(&RoomMemberEventContent {
                membership: MembershipState::Join,
                display_name: crate::user::display_name(authed.user_id())?,
                avatar_url: crate::user::avatar_url(authed.user_id())?,
                is_direct: Some(body.is_direct),
                third_party_invite: None,
                blurhash: crate::user::blurhash(authed.user_id())?,
                reason: None,
                join_authorized_via_users_server: None,
            })
            .expect("event is valid, we just created it"),
            state_key: Some(authed.user_id().to_string()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 3. Power levels
    // Figure out preset. We need it for preset specific events
    let preset = body.preset.clone().unwrap_or(match &body.visibility {
        Visibility::Private => RoomPreset::PrivateChat,
        Visibility::Public => RoomPreset::PublicChat,
        _ => RoomPreset::PrivateChat, // Room visibility should not be custom
    });

    let mut users = BTreeMap::new();
    users.insert(authed.user_id().clone(), 100);

    if preset == RoomPreset::TrustedPrivateChat {
        for invitee_id in &body.invite {
            users.insert(invitee_id.clone(), 100);
        }
    }

    let power_levels_content =
        default_power_levels_content(body.power_level_content_override.as_ref(), &body.visibility, users)?;

    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomPowerLevels,
            content: to_raw_value(&power_levels_content)?,
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 4. Canonical room alias
    if let Some(room_alias_id) = &alias {
        crate::room::timeline::build_and_append_pdu(
            PduBuilder {
                event_type: TimelineEventType::RoomCanonicalAlias,
                content: to_raw_value(&RoomCanonicalAliasEventContent {
                    alias: Some(room_alias_id.to_owned()),
                    alt_aliases: vec![],
                })
                .expect("We checked that alias earlier, it must be fine"),
                state_key: Some("".to_owned()),
                ..Default::default()
            },
            authed.user_id(),
            &room_id,
        )
        .unwrap();
    }

    // 5. Events set by preset
    // 5.1 Join Rules
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomJoinRules,
            content: to_raw_value(&RoomJoinRulesEventContent::new(match preset {
                RoomPreset::PublicChat => JoinRule::Public,
                // according to spec "invite" is the default
                _ => JoinRule::Invite,
            }))
            .expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 5.2 History Visibility
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomHistoryVisibility,
            content: to_raw_value(&RoomHistoryVisibilityEventContent::new(HistoryVisibility::Shared))
                .expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 5.3 Guest Access
    crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: TimelineEventType::RoomGuestAccess,
            content: to_raw_value(&RoomGuestAccessEventContent::new(match preset {
                RoomPreset::PublicChat => GuestAccess::Forbidden,
                _ => GuestAccess::CanJoin,
            }))
            .expect("event is valid, we just created it"),
            state_key: Some("".to_owned()),
            ..Default::default()
        },
        authed.user_id(),
        &room_id,
    )?;

    // 6. Events listed in initial_state
    for event in &body.initial_state {
        let mut pdu_builder = event.deserialize_as::<PduBuilder>().map_err(|e| {
            warn!("Invalid initial state event: {:?}", e);
            MatrixError::invalid_param("Invalid initial state event.")
        })?;

        // Implicit state key defaults to ""
        pdu_builder.state_key.get_or_insert_with(|| "".to_owned());

        // Silently skip encryption events if they are not allowed
        if pdu_builder.event_type == TimelineEventType::RoomEncryption && !crate::allow_encryption() {
            continue;
        }

        crate::room::timeline::build_and_append_pdu(pdu_builder, authed.user_id(), &room_id)?;
    }

    // 7. Events implied by name and topic
    if let Some(name) = &body.name {
        crate::room::timeline::build_and_append_pdu(
            PduBuilder {
                event_type: TimelineEventType::RoomName,
                content: to_raw_value(&RoomNameEventContent::new(name.clone()))
                    .expect("event is valid, we just created it"),
                state_key: Some("".to_owned()),
                ..Default::default()
            },
            authed.user_id(),
            &room_id,
        )?;
    }

    if let Some(topic) = &body.topic {
        crate::room::timeline::build_and_append_pdu(
            PduBuilder {
                event_type: TimelineEventType::RoomTopic,
                content: to_raw_value(&RoomTopicEventContent { topic: topic.clone() })
                    .expect("event is valid, we just created it"),
                state_key: Some("".to_owned()),
                ..Default::default()
            },
            authed.user_id(),
            &room_id,
        )?;
    }

    // 8. Events implied by invite (and TODO: invite_3pid)
    for user_id in &body.invite {
        if let Err(e) = crate::membership::invite_user(authed.user_id(), user_id, &room_id, None, body.is_direct).await
        {
            tracing::error!("Failed to invite user {}: {:?}", user_id, e);
        }
    }

    // Homeserver specific stuff
    if let Some(alias) = alias {
        crate::room::set_alias(&room_id, &alias, authed.user_id())?;
    }

    if body.visibility == Visibility::Public {
        crate::room::directory::set_public(&room_id, true)?;
    }

    info!("{} created a room", authed.user_id());
    json_ok(CreateRoomResBody { room_id })
}

/// creates the power_levels_content for the PDU builder
fn default_power_levels_content(
    power_level_content_override: Option<&RoomPowerLevelsEventContent>,
    visibility: &Visibility,
    users: BTreeMap<OwnedUserId, i64>,
) -> AppResult<serde_json::Value> {
    let mut power_levels_content = serde_json::to_value(RoomPowerLevelsEventContent {
        users,
        ..Default::default()
    })
    .expect("event is valid, we just created it");

    // secure proper defaults of sensitive/dangerous permissions that moderators
    // (power level 50) should not have easy access to
    power_levels_content["events"]["m.room.power_levels"] = serde_json::to_value(100).expect("100 is valid Value");
    power_levels_content["events"]["m.room.server_acl"] = serde_json::to_value(100).expect("100 is valid Value");
    power_levels_content["events"]["m.room.tombstone"] = serde_json::to_value(100).expect("100 is valid Value");
    power_levels_content["events"]["m.room.encryption"] = serde_json::to_value(100).expect("100 is valid Value");
    power_levels_content["events"]["m.room.history_visibility"] =
        serde_json::to_value(100).expect("100 is valid Value");

    // always allow users to respond (not post new) to polls. this is primarily
    // useful in read-only announcement rooms that post a public poll.
    power_levels_content["events"]["org.matrix.msc3381.poll.response"] =
        serde_json::to_value(0).expect("0 is valid Value");
    power_levels_content["events"]["m.poll.response"] = serde_json::to_value(0).expect("0 is valid Value");

    // synapse does this too. clients do not expose these permissions. it prevents
    // default users from calling public rooms, for obvious reasons.
    if *visibility == Visibility::Public {
        power_levels_content["events"]["m.call.invite"] = serde_json::to_value(50).expect("50 is valid Value");
        power_levels_content["events"]["m.call"] = serde_json::to_value(50).expect("50 is valid Value");
        power_levels_content["events"]["m.call.member"] = serde_json::to_value(50).expect("50 is valid Value");
        power_levels_content["events"]["org.matrix.msc3401.call"] =
            serde_json::to_value(50).expect("50 is valid Value");
        power_levels_content["events"]["org.matrix.msc3401.call.member"] =
            serde_json::to_value(50).expect("50 is valid Value");
    }

    if let Some(power_level_content_override) = power_level_content_override {
        let JsonValue::Object(json) = serde_json::to_value(power_level_content_override)
            .map_err(|_| MatrixError::bad_json("Invalid power_level_content_override."))?
        else {
            return Err(MatrixError::bad_json("Invalid power_level_content_override.").into());
        };

        for (key, value) in json {
            power_levels_content[key] = value;
        }
    }

    Ok(power_levels_content)
}
