/// Endpoints to retrieve information from a homeserver about a resource.

/// `GET /_matrix/federation/*/query/directory`
///
/// Get mapped room ID and resident homeservers for a given room alias.
use std::collections::BTreeMap;

use crate::sending::{SendRequest, SendResult};
use crate::user::ProfileField;
use crate::{OwnedRoomId, OwnedServerName, OwnedUserId, RoomAliasId};
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1querydirectory
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/query/directory",
//     }
// };

pub fn directory_request(room_alias: &RoomAliasId) -> SendResult<SendRequest> {
    Ok(crate::sending::get(room_alias.server_name().build_url(&format!(
        "federation/v1/query/directory?room_alias={room_alias}"
    ))?))
}

/// Request type for the `get_room_information` endpoint.

// #[derive(ToSchema, Deserialize, Debug)]
// pub struct RoomInfoReqArgs {
//     /// Room alias to query.
//     #[salvo(parameter(parameter_in = Query))]
//     pub room_alias: OwnedRoomAliasId,
// }

/// Response type for the `get_room_information` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct RoomInfoResBody {
    /// Room ID mapped to queried alias.
    pub room_id: OwnedRoomId,

    /// An array of server names that are likely to hold the given room.
    pub servers: Vec<OwnedServerName>,
}
impl RoomInfoResBody {
    /// Creates a new `Response` with the given room IDs and servers.
    pub fn new(room_id: OwnedRoomId, servers: Vec<OwnedServerName>) -> Self {
        Self { room_id, servers }
    }
}

/// `GET /_matrix/federation/*/query/profile`
///
/// Get profile information, such as a display name or avatar, for a given user.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1queryprofile

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/query/profile",
//     }
// };

pub fn profile_request(args: ProfileReqArgs) -> SendResult<SendRequest> {
    Ok(crate::sending::get(args.user_id.server_name().build_url(&format!(
        "/federation/v1/query/profile?user_id={}{}",
        args.user_id,
        args.field.map(|field|format!("&field={}", field.to_string())).unwrap_or_default()
    ))?))
}

/// Request type for the `get_profile_information` endpoint.

#[derive(ToParameters, Deserialize, Debug)]
pub struct ProfileReqArgs {
    /// User ID to query.
    #[salvo(parameter(parameter_in = Query))]
    pub user_id: OwnedUserId,

    /// Profile field to query.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<ProfileField>,
}

/// `GET /_matrix/federation/*/query/{queryType}`
///
/// Performs a single query request on the receiving homeserver. The query arguments are dependent
/// on which type of query is being made.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1queryquerytype

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/federation/v1/query/:query_type",
//     }
// };

/// Request type for the `get_custom_information` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct CustomReqBody {
    /// The type of query to make.
    #[salvo(parameter(parameter_in = Path))]
    pub query_type: String,

    /// The query parameters.
    pub params: BTreeMap<String, String>,
}

/// Response type for the `get_custom_information` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct CustomResBody {
    /// The body of the response.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub body: JsonValue,
}

impl CustomResBody {
    /// Creates a new response with the given body.
    pub fn new(body: JsonValue) -> Self {
        Self { body }
    }
}
