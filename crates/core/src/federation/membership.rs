//! Room membership endpoints.

use reqwest::Url;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::events::{AnyStrippedStateEvent, StateEventType, room::member::RoomMemberEventContent};
use crate::identifiers::*;
use crate::sending::{SendRequest, SendResult};
use crate::{RawJsonValue, UnixMillis, serde::RawJson};

pub fn invite_user_request_v2(
    origin: &str,
    args: InviteUserReqArgs,
    body: InviteUserReqBodyV2,
) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v2/invite/{}/{}",
        args.room_id, args.event_id
    ))?;
    crate::sending::put(url).stuff(body)
}
#[derive(ToParameters, Deserialize, Debug)]
pub struct InviteUserReqArgs {
    /// The room ID that is about to be joined.
    ///
    /// Do not use this. Instead, use the `room_id` field inside the PDU.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event ID for the join event.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct InviteUserReqBodyV2 {
    // /// The room ID that the user is being invited to.
    // #[salvo(parameter(parameter_in = Path))]
    // pub room_id: OwnedRoomId,

    // /// The event ID for the invite event, generated by the inviting server.
    // #[salvo(parameter(parameter_in = Path))]
    // pub event_id: OwnedEventId,
    /// The version of the room where the user is being invited to.
    pub room_version: RoomVersionId,

    /// The invite event which needs to be signed.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub event: Box<RawJsonValue>,

    /// An optional list of simplified events to help the receiver of the invite identify the room.
    pub invite_room_state: Vec<RawJson<AnyStrippedStateEvent>>,
}
crate::json_body_modifier!(InviteUserReqBodyV2);

#[derive(ToSchema, Deserialize, Debug)]
pub struct InviteUserReqBodyV1 {
    /// The matrix ID of the user who sent the original `m.room.third_party_invite`.
    pub sender: OwnedUserId,

    /// The name of the inviting homeserver.
    pub origin: OwnedServerName,

    /// A timestamp added by the inviting homeserver.
    pub origin_server_ts: UnixMillis,

    /// The value `m.room.member`.
    #[serde(rename = "type")]
    pub kind: StateEventType,

    /// The user ID of the invited member.
    pub state_key: OwnedUserId,

    /// The content of the event.
    pub content: RoomMemberEventContent,

    /// Information included alongside the event that is not signed.
    #[serde(default, skip_serializing_if = "UnsignedEventContent::is_empty")]
    pub unsigned: UnsignedEventContentV1,
}

#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct InviteUserResBodyV2 {
    /// The signed invite event.
    #[salvo(schema(value_type = Object))]
    pub event: Box<RawJsonValue>,
}

#[derive(ToSchema, Serialize, Debug)]
pub struct InviteUserResBodyV1 {
    /// The signed invite event.
    #[serde(with = "crate::federation::serde::v1_pdu")]
    #[salvo(schema(value_type = Object))]
    pub event: Box<RawJsonValue>,
}

#[derive(ToSchema, Deserialize, Serialize, Debug)]
#[salvo(schema(value_type = Object))]
pub struct SendJoinReqBody(
    /// The invite event which needs to be signed.
    pub Box<RawJsonValue>,
);
crate::json_body_modifier!(SendJoinReqBody);

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct SendJoinResBodyV2(
    /// The signed invite event.
    pub RoomStateV2,
);

#[derive(ToSchema, Serialize, Debug)]
pub struct SendJoinResBodyV1 (
    /// Full state of the room.
    pub RoomStateV1,
);

impl SendJoinResBodyV1 {
    /// Creates a new `Response` with the given room state.
    pub fn new(room_state: RoomStateV1) -> Self {
        Self (room_state)
    }
}

/// Full state of the room.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct RoomStateV2 {
    /// Whether `m.room.member` events have been omitted from `state`.
    ///
    /// Defaults to `false`.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub members_omitted: bool,

    /// The full set of authorization events that make up the state of the room,
    /// and their authorization events, recursively.
    ///
    /// If the request had `omit_members` set to `true`, then any events that are returned in
    /// `state` may be omitted from `auth_chain`, whether or not membership events are omitted
    /// from `state`.
    #[salvo(schema(value_type = Vec<Object>))]
    pub auth_chain: Vec<Box<RawJsonValue>>,

    /// The room state.
    ///
    /// If the request had `omit_members` set to `true`, events of type `m.room.member` may be
    /// omitted from the response to reduce the size of the response. If this is done,
    /// `members_omitted` must be set to `true`.
    #[salvo(schema(value_type = Object))]
    pub state: Vec<Box<RawJsonValue>>,

    /// The signed copy of the membership event sent to other servers by the
    /// resident server, including the resident server's signature.
    ///
    /// Required if the room version supports restricted join rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[salvo(schema(value_type = Object))]
    pub event: Option<Box<RawJsonValue>>,

    /// A list of the servers active in the room (ie, those with joined members) before the join.
    ///
    /// Required if `members_omitted` is set to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers_in_room: Option<Vec<String>>,
}

impl RoomStateV2 {
    /// Creates an empty `RoomState` with the given `origin`.
    ///
    /// With the `unstable-unspecified` feature, this method doesn't take any parameters.
    /// See [matrix-spec#374](https://github.com/matrix-org/matrix-spec/issues/374).
    pub fn new() -> Self {
        Self {
            auth_chain: Vec::new(),
            state: Vec::new(),
            event: None,
            members_omitted: false,
            servers_in_room: None,
        }
    }
}

/// Full state of the room.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct RoomStateV1 {
    /// The full set of authorization events that make up the state of the room,
    /// and their authorization events, recursively.
    #[salvo(schema(value_type = Vec<Object>))]
    pub auth_chain: Vec<Box<RawJsonValue>>,

    /// The room state.
    #[salvo(schema(value_type = Vec<Object>))]
    pub state: Vec<Box<RawJsonValue>>,

    /// The signed copy of the membership event sent to other servers by the
    /// resident server, including the resident server's signature.
    ///
    /// Required if the room version supports restricted join rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[salvo(schema(value_type = Vec<Object>))]
    pub event: Option<Box<RawJsonValue>>,
}
impl RoomStateV1 {
    /// Creates an empty `RoomState` with the given `origin`.
    ///
    /// With the `unstable-unspecified` feature, this method doesn't take any parameters.
    /// See [matrix-spec#374](https://github.com/matrix-org/matrix-spec/issues/374).
    pub fn new() -> Self {
        Self {
            auth_chain: Vec::new(),
            state: Vec::new(),
            event: None,
        }
    }
}


/// Information included alongside an event that is not signed.
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct UnsignedEventContentV1 {
    /// An optional list of simplified events to help the receiver of the invite identify the room.
    /// The recommended events to include are the join rules, canonical alias, avatar, and name of
    /// the room.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    #[salvo(schema(value_type = Vec<Object>))]
    pub invite_room_state: Vec<RawJson<AnyStrippedStateEvent>>,
}

impl UnsignedEventContentV1 {
    /// Creates an empty `UnsignedEventContent`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Checks whether all of the fields are empty.
    pub fn is_empty(&self) -> bool {
        self.invite_room_state.is_empty()
    }
}

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/make_leave/:room_id/:user_id",
//     }
// };

pub fn make_leave_request(origin: &str, room_id: &RoomId, user_id: &UserId) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v1/make_leave/{room_id}/{user_id}"
    ))?;
    Ok(crate::sending::get(url))
}
#[derive(ToParameters, Deserialize, Debug)]
pub struct MakeLeaveReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,
}

/// Response type for the `get_leave_event` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct MakeLeaveResBody {
    /// The version of the room where the server is trying to leave.
    ///
    /// If not provided, the room version is assumed to be either "1" or "2".
    pub room_version: Option<RoomVersionId>,

    /// An unsigned template event.
    ///
    /// Note that events have a different format depending on the room version - check the room
    /// version specification for precise event formats.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub event: Box<RawJsonValue>,
}
impl MakeLeaveResBody {
    /// Creates a new `Response` with:
    /// * the version of the room where the server is trying to leave.
    /// * an unsigned template event.
    pub fn new(room_version: Option<RoomVersionId>, event: Box<RawJsonValue>) -> Self {
        Self { room_version, event }
    }
}

// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v2/send_leave/:room_id/:event_id",
//     }
// };

pub fn send_leave_request_v2(
    origin: &str,
    args: SendLeaveReqArgsV2,
    body: SendLeaveReqBody,
) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v2/send_leave/{}/{}",
        args.room_id, args.event_id
    ))?;
    crate::sending::put(url).stuff(body)
}
#[derive(ToParameters, Deserialize, Serialize, Debug)]
pub struct SendLeaveReqArgsV2 {
    /// The room ID that is about to be left.
    ///
    /// Do not use this. Instead, use the `room_id` field inside the PDU.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event ID for the leave event.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}
/// Request type for the `create_leave_event` endpoint.
#[derive(ToSchema, Deserialize, Serialize, Debug)]
#[salvo(schema(value_type = Object))]
pub struct SendLeaveReqBody(
    /// The PDU.
    pub Box<RawJsonValue>,
);
crate::json_body_modifier!(SendLeaveReqBody);

/// `PUT /_matrix/federation/*/send_join/{room_id}/{event_id}`
///
/// Send a join event to a resident server.

// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v2/send_join/:room_id/:event_id",
//     }
// };

pub fn send_join_request(origin: &str, args: SendJoinArgs, body: SendJoinReqBody) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v2/send_join/{}/{}?omit_members={}",
        &args.room_id, &args.event_id, args.omit_members
    ))?;
    crate::sending::put(url).stuff(body)
}
/// Request type for the `create_join_event` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct SendJoinArgs {
    /// The room ID that is about to be joined.
    ///
    /// Do not use this. Instead, use the `room_id` field inside the PDU.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event ID for the join event.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,

    /// Indicates whether the calling server can accept a reduced response.
    ///
    /// If `true`, membership events are omitted from `state` and redundant events are omitted from
    /// `auth_chain` in the response.
    ///
    /// If the room to be joined has no `m.room.name` nor `m.room.canonical_alias` events in its
    /// current state, the resident server should determine the room members who would be
    /// included in the `m.heroes` property of the room summary as defined in the [Client-Server
    /// `/sync` response]. The resident server should include these members' membership events in
    /// the response `state` field, and include the auth chains for these membership events in
    /// the response `auth_chain` field.
    ///
    /// [Client-Server `/sync` response]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3sync
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub omit_members: bool,
}

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/make_join/:room_id/:user_id",
//     }
// };

pub fn make_join_request(origin: &str, args: MakeJoinReqArgs) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v1/make_join/{}/{}?ver=[{}]",
        args.room_id,
        args.user_id,
        args.ver
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join(",")
    ))?;
    Ok(crate::sending::get(url))
}

/// Request type for the `create_join_event_template` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct MakeJoinReqArgs {
    /// The room ID that is about to be joined.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The user ID the join event will be for.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The room versions the sending server has support for.
    ///
    /// Defaults to `&[RoomVersionId::V1]`.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default = "default_ver", skip_serializing_if = "is_default_ver")]
    pub ver: Vec<RoomVersionId>,
}

/// Response type for the `create_join_event_template` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct MakeJoinResBody {
    /// The version of the room where the server is trying to join.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_version: Option<RoomVersionId>,

    /// An unsigned template event.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub event: Box<RawJsonValue>,
}

impl MakeJoinResBody {
    /// Creates a new `Response` with the given template event.
    pub fn new(event: Box<RawJsonValue>) -> Self {
        Self {
            room_version: None,
            event,
        }
    }
}

fn default_ver() -> Vec<RoomVersionId> {
    vec![RoomVersionId::V1]
}

fn is_default_ver(ver: &[RoomVersionId]) -> bool {
    *ver == [RoomVersionId::V1]
}
