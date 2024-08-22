use crate::events::space::child::HierarchySpaceChildEvent;
use crate::{room::RoomType, serde::RawJson, space::SpaceRoomJoinRule, OwnedMxcUri, OwnedRoomAliasId, OwnedRoomId};

use salvo::prelude::*;
/// Spaces endpoints.
use serde::{Deserialize, Serialize};

/// The summary of a parent space.
///
/// To create an instance of this type, first create a `SpaceHierarchyParentSummaryInit` and convert
/// it via `SpaceHierarchyParentSummary::from` / `.into()`.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct SpaceHierarchyParentSummary {
    /// The canonical alias of the room, if any.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub canonical_alias: Option<OwnedRoomAliasId>,

    /// The name of the room, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The number of members joined to the room.
    pub num_joined_members: u64,

    /// The ID of the room.
    pub room_id: OwnedRoomId,

    /// The topic of the room, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,

    /// Whether the room may be viewed by guest users without joining.
    pub world_readable: bool,

    /// Whether guest users may join the room and participate in it.
    ///
    /// If they can, they will be subject to ordinary power level rules like any other user.
    pub guest_can_join: bool,

    /// The URL for the room's avatar, if one is set.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub avatar_url: Option<OwnedMxcUri>,

    /// The join rule of the room.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub join_rule: SpaceRoomJoinRule,

    /// The type of room from `m.room.create`, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_type: Option<RoomType>,

    /// The stripped `m.space.child` events of the space-room.
    ///
    /// If the room is not a space-room, this should be empty.
    pub children_state: Vec<RawJson<HierarchySpaceChildEvent>>,

    /// If the room is a restricted room, these are the room IDs which are specified by the join
    /// rules.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub allowed_room_ids: Vec<OwnedRoomId>,
}

/// Initial set of mandatory fields of `SpaceHierarchyParentSummary`.
///
/// This struct will not be updated even if additional fields are added to
/// `SpaceHierarchyParentSummary` in a new (non-breaking) release of the Matrix specification.
#[derive(Debug)]
#[allow(clippy::exhaustive_structs)]
pub struct SpaceHierarchyParentSummaryInit {
    /// The number of members joined to the room.
    pub num_joined_members: u64,

    /// The ID of the room.
    pub room_id: OwnedRoomId,

    /// Whether the room may be viewed by guest users without joining.
    pub world_readable: bool,

    /// Whether guest users may join the room and participate in it.
    ///
    /// If they can, they will be subject to ordinary power level rules like any other user.
    pub guest_can_join: bool,

    /// The join rule of the room.
    pub join_rule: SpaceRoomJoinRule,

    /// The stripped `m.space.child` events of the space-room.
    ///
    /// If the room is not a space-room, this should be empty.
    pub children_state: Vec<RawJson<HierarchySpaceChildEvent>>,

    /// If the room is a restricted room, these are the room IDs which are specified by the join
    /// rules.
    pub allowed_room_ids: Vec<OwnedRoomId>,
}

impl From<SpaceHierarchyParentSummaryInit> for SpaceHierarchyParentSummary {
    fn from(init: SpaceHierarchyParentSummaryInit) -> Self {
        let SpaceHierarchyParentSummaryInit {
            num_joined_members,
            room_id,
            world_readable,
            guest_can_join,
            join_rule,
            children_state,
            allowed_room_ids,
        } = init;

        Self {
            canonical_alias: None,
            name: None,
            num_joined_members,
            room_id,
            topic: None,
            world_readable,
            guest_can_join,
            avatar_url: None,
            join_rule,
            room_type: None,
            children_state,
            allowed_room_ids,
        }
    }
}

/// The summary of a space's child.
///
/// To create an instance of this type, first create a `SpaceHierarchyChildSummaryInit` and convert
/// it via `SpaceHierarchyChildSummary::from` / `.into()`.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct SpaceHierarchyChildSummary {
    /// The canonical alias of the room, if any.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub canonical_alias: Option<OwnedRoomAliasId>,

    /// The name of the room, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The number of members joined to the room.
    pub num_joined_members: u64,

    /// The ID of the room.
    pub room_id: OwnedRoomId,

    /// The topic of the room, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,

    /// Whether the room may be viewed by guest users without joining.
    pub world_readable: bool,

    /// Whether guest users may join the room and participate in it.
    ///
    /// If they can, they will be subject to ordinary power level rules like any other user.
    pub guest_can_join: bool,

    /// The URL for the room's avatar, if one is set.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub avatar_url: Option<OwnedMxcUri>,

    /// The join rule of the room.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub join_rule: SpaceRoomJoinRule,

    /// The type of room from `m.room.create`, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_type: Option<RoomType>,

    /// If the room is a restricted room, these are the room IDs which are specified by the join
    /// rules.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub allowed_room_ids: Vec<OwnedRoomId>,
}

/// Initial set of mandatory fields of `SpaceHierarchyChildSummary`.
///
/// This struct will not be updated even if additional fields are added to
/// `SpaceHierarchyChildSummary` in a new (non-breaking) release of the Matrix specification.
#[derive(Debug)]
#[allow(clippy::exhaustive_structs)]
pub struct SpaceHierarchyChildSummaryInit {
    /// The number of members joined to the room.
    pub num_joined_members: u64,

    /// The ID of the room.
    pub room_id: OwnedRoomId,

    /// Whether the room may be viewed by guest users without joining.
    pub world_readable: bool,

    /// Whether guest users may join the room and participate in it.
    ///
    /// If they can, they will be subject to ordinary power level rules like any other user.
    pub guest_can_join: bool,

    /// The join rule of the room.
    pub join_rule: SpaceRoomJoinRule,

    /// If the room is a restricted room, these are the room IDs which are specified by the join
    /// rules.
    pub allowed_room_ids: Vec<OwnedRoomId>,
}

impl From<SpaceHierarchyChildSummaryInit> for SpaceHierarchyChildSummary {
    fn from(init: SpaceHierarchyChildSummaryInit) -> Self {
        let SpaceHierarchyChildSummaryInit {
            num_joined_members,
            room_id,
            world_readable,
            guest_can_join,
            join_rule,
            allowed_room_ids,
        } = init;

        Self {
            canonical_alias: None,
            name: None,
            num_joined_members,
            room_id,
            topic: None,
            world_readable,
            guest_can_join,
            avatar_url: None,
            join_rule,
            room_type: None,
            allowed_room_ids,
        }
    }
}
/// `GET /_matrix/federation/*/hierarchy/{room_id}`
///
/// Get the space tree in a depth-first manner to locate child rooms of a given space.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1hierarchyroomid
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         unstable => "/_matrix/federation/unstable/org.matrix.msc2946/hierarchy/:room_id",
//         1.2 => "/_matrix/federation/v1/hierarchy/:room_id",
//     }
// };

/// Request type for the `hierarchy` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct HierarchyReqBody {
    /// The room ID of the space to get a hierarchy for.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Whether or not the server should only consider suggested rooms.
    ///
    /// Suggested rooms are annotated in their `m.space.child` event contents.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub suggested_only: bool,
}

/// Response type for the `hierarchy` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct HierarchyResBody {
    /// A summary of the space’s children.
    ///
    /// Rooms which the requesting server cannot peek/join will be excluded.
    pub children: Vec<SpaceHierarchyChildSummary>,

    /// The list of room IDs the requesting server doesn’t have a viable way to peek/join.
    ///
    /// Rooms which the responding server cannot provide details on will be outright
    /// excluded from the response instead.
    pub inaccessible_children: Vec<OwnedRoomId>,

    /// A summary of the requested room.
    pub room: SpaceHierarchyParentSummary,
}

impl HierarchyResBody {
    /// Creates a new `Response` with the given room summary.
    pub fn new(room_summary: SpaceHierarchyParentSummary) -> Self {
        Self {
            children: Vec::new(),
            inaccessible_children: Vec::new(),
            room: room_summary,
        }
    }
}
