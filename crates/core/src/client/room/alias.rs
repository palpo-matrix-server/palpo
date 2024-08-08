/// `GET /_matrix/client/*/rooms/{room_id}/aliases`
///
/// Get a list of aliases maintained by the local server for the given room.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3roomsroomidaliases
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{OwnedRoomAliasId, OwnedRoomId, OwnedServerName};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         unstable => "/_matrix/client/unstable/org.matrix.msc2432/rooms/:room_id/aliases",
//         1.0 => "/_matrix/client/r0/rooms/:room_id/aliases",
//         1.1 => "/_matrix/client/v3/rooms/:room_id/aliases",
//     }
// };

// /// Request type for the `aliases` endpoint.

// pub struct Requesxt {
//     /// The room ID to get aliases of.
//     #[salvo(parameter(parameter_in = Path))]
//     pub room_id: OwnedRoomId,
// }

/// Response type for the `aliases` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct AliasesResBody {
    /// The server's local aliases on the room.
    pub aliases: Vec<OwnedRoomAliasId>,
}
impl AliasesResBody {
    /// Creates a new `Response` with the given aliases.
    pub fn new(aliases: Vec<OwnedRoomAliasId>) -> Self {
        Self { aliases }
    }
}

/// Endpoints for room aliases.
/// `GET /_matrix/client/*/directory/room/{roomAlias}`
///
/// Resolve a room alias to a room ID.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3directoryroomroomalias

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/client/r0/directory/room/:room_alias",
//         1.1 => "/_matrix/client/v3/directory/room/:room_alias",
//     }
// };

// /// Request type for the `get_alias` endpoint.

// pub struct Requesxt {
//     /// The room alias.
//     #[salvo(parameter(parameter_in = Path))]
//     pub room_alias: OwnedRoomAliasId,
// }

/// Response type for the `get_alias` endpoint.

#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct AliasResBody {
    /// The room ID for this room alias.
    pub room_id: OwnedRoomId,

    /// A list of servers that are aware of this room ID.
    pub servers: Vec<OwnedServerName>,
}
impl AliasResBody {
    /// Creates a new `Response` with the given room id and servers
    pub fn new(room_id: OwnedRoomId, servers: Vec<OwnedServerName>) -> Self {
        Self { room_id, servers }
    }
}

/// `PUT /_matrix/client/*/directory/room/{roomAlias}`
///
/// Add an alias to a room.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3directoryroomroomalias
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/directory/room/:room_alias",
//         1.1 => "/_matrix/client/v3/directory/room/:room_alias",
//     }
// };

/// Request type for the `create_alias` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct SetAliasReqBody {
    // /// The room alias to set.
    // #[salvo(parameter(parameter_in = Path))]
    // pub room_alias: OwnedRoomAliasId,
    /// The room ID to set.
    pub room_id: OwnedRoomId,
}

// `DELETE /_matrix/client/*/directory/room/{roomAlias}`
//
// Remove an alias from a room.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#delete_matrixclientv3directoryroomroomalias

// const METADATA: Metadata = metadata! {
//     method: DELETE,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/directory/room/:room_alias",
//         1.1 => "/_matrix/client/v3/directory/room/:room_alias",
//     }
// };

// /// Request type for the `delete_alias` endpoint.

// pub struct DeleteAliasReqArgs {
//     /// The room alias to remove.
//     #[salvo(parameter(parameter_in = Path))]
//     pub room_alias: OwnedRoomAliasId,
// }
