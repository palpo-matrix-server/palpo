use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::events::{GlobalAccountDataEventType, StateEventType};
use crate::push::{RuleKind, RuleScope};
use crate::{OwnedEventId, OwnedMxcUri, OwnedRoomId, OwnedUserId};

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomEventReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}
#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomTypingReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomUserReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserRoomReqArgs {
    /// The user whose tags will be retrieved.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The room from which tags will be retrieved.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserEventTypeReqArgs {
    /// The ID of the user to set account_data for.
    ///
    /// The access token must be authorized to make requests for this user ID.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The event type of the account_data to set.
    ///
    /// Custom types should be namespaced to avoid clashes.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: GlobalAccountDataEventType,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomEventTypeReqArgs {
    /// The ID of the room the event is in.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The ID of the event.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: StateEventType,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserRoomEventTypeReqArgs {
    /// The ID of the user to set account_data for.
    ///
    /// The access token must be authorized to make requests for this user ID.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event type of the account_data to set.
    ///
    /// Custom types should be namespaced to avoid clashes.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: GlobalAccountDataEventType,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserFilterReqArgs {
    /// The user ID to download a filter for.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The ID of the filter to download.
    #[salvo(parameter(parameter_in = Path))]
    pub filter_id: String,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct ScopeKindRuleReqArgs {
    /// The scope to fetch rules from.
    #[salvo(parameter(parameter_in = Path))]
    pub scope: RuleScope,

    /// The kind of rule.
    #[salvo(parameter(parameter_in = Path))]
    pub kind: RuleKind,

    /// The identifier for the rule.
    #[salvo(parameter(parameter_in = Path))]
    pub rule_id: String,
}

///  GET /_matrix/federation/v1/query/profile
/// `GET /_matrix/client/*/profile/{user_id}`
///
/// Get all profile information of an user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3profileuser_id
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/client/r0/profile/:user_id",
//         1.1 => "/_matrix/client/v3/profile/:user_id",
//     }
// };

// /// Request type for the `get_profile` endpoint.

/// Response type for the `get_profile` endpoint.
#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct ProfileResBody {
    /// The user's avatar URL, if set.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub avatar_url: Option<OwnedMxcUri>,

    /// The user's display name, if set.
    #[serde(skip_serializing_if = "Option::is_none", rename = "displayname")]
    pub display_name: Option<String>,

    /// The [BlurHash](https://blurha.sh) for the avatar pointed to by `avatar_url`.
    ///
    /// This uses the unstable prefix in
    /// [MSC2448](https://github.com/matrix-org/matrix-spec-proposals/pull/2448).
    #[serde(rename = "xyz.amorgan.blurhash", skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}
impl ProfileResBody {
    /// Creates a new `Response` with the given avatar URL and display name.
    pub fn new(avatar_url: Option<OwnedMxcUri>, display_name: Option<String>) -> Self {
        Self {
            avatar_url,
            display_name,
            blurhash: None,
        }
    }
}
