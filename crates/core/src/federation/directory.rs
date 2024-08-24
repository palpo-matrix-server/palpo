/// Room directory endpoints.

/// `GET /_matrix/federation/*/publicRooms`
///
/// Get all the public rooms for the homeserver.
use std::collections::BTreeMap;

use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::directory::{PublicRoomFilter, QueryCriteria, RoomNetwork, Server};
use crate::federation::discovery::ServerSigningKeys;
use crate::serde::RawJson;use crate::sending::{SendResult,SendRequest};
use crate::{EventId, OwnedServerName, OwnedServerSigningKeyId, RoomId, ServerName, UnixMillis};

/// `POST /_matrix/federation/*/publicRooms`
///
/// Get a homeserver's public rooms with an optional filter.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#post_matrixfederationv1publicrooms

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/publicRooms",
//     }
// };

pub fn public_rooms_request(server: &ServerName, body: PublicRoomsReqBody) -> SendResult<SendRequest> {
    crate::sending::get(server.build_url("federation/v1/publicRooms")?)
        .stuff(body)
}

/// Request type for the `get_filtered_public_rooms` endpoint.

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct PublicRoomsReqBody {
    /// Limit for the number of results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,

    /// Pagination token from a previous request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,

    /// Filter to apply to the results.
    #[serde(default, skip_serializing_if = "PublicRoomFilter::is_empty")]
    pub filter: PublicRoomFilter,

    /// Network to fetch the public room lists from.
    #[serde(flatten, skip_serializing_if = "crate::serde::is_default")]
    pub room_network: RoomNetwork,
}
crate::json_body_modifier!(PublicRoomsReqBody);
/// `GET /.well-known/matrix/server` ([spec])
///
/// Get discovery information about the domain.
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#getwell-knownmatrixserver

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/.well-known/matrix/server",
//     }
// };

/// Response type for the `discover_homeserver` endpoint.

#[derive(ToSchema, Serialize, Debug)]
pub struct ServerResBody {
    /// The server name to delegate server-server communications to, with optional port.
    #[serde(rename = "m.server")]
    pub server: OwnedServerName,
}

impl ServerResBody {
    /// Creates a new `Response` with the given homeserver.
    pub fn new(server: OwnedServerName) -> Self {
        Self { server }
    }
}

/// `POST /_matrix/key/*/query`
///
/// Query for keys from multiple servers in a batch format. The receiving (notary) server must sign
/// the keys returned by the queried servers.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#post_matrixkeyv2query

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/key/v2/query",
//     }
// };

/// Request type for the `get_remote_server_keys_batch` endpoint.

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct RemoteServerKeysBatchReqBody {
    /// The query criteria.
    ///
    /// The outer string key on the object is the server name (eg: matrix.org). The inner
    /// string key is the Key ID to query for the particular server. If no key IDs are given to
    /// be queried, the notary server should query for all keys. If no servers are given, the
    /// notary server must return an empty server_keys array in the response.
    ///
    /// The notary server may return multiple keys regardless of the Key IDs given.
    pub server_keys: BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>>,
}
crate::json_body_modifier!(RemoteServerKeysBatchReqBody);

/// Response type for the `get_remote_server_keys_batch` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct RemoteServerKeysBatchResBody {
    /// The queried server's keys, signed by the notary server.
    pub server_keys: Vec<RawJson<ServerSigningKeys>>,
}
impl RemoteServerKeysBatchResBody {
    /// Creates a new `Response` with the given keys.
    pub fn new(server_keys: Vec<RawJson<ServerSigningKeys>>) -> Self {
        Self { server_keys }
    }
}
/// `GET /_matrix/key/*/query/{serverName}`
///
/// Query for another server's keys. The receiving (notary) server must sign the keys returned by
/// the queried server.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixkeyv2queryservername

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/key/v2/query/:server_name",
//     }
// };

/// Request type for the `get_remote_server_keys` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct RemoteServerKeysReqArgs {
    /// The server's DNS name to query
    #[salvo(parameter(parameter_in = Path))]
    pub server_name: OwnedServerName,

    /// A millisecond POSIX timestamp in milliseconds indicating when the returned certificates
    /// will need to be valid until to be useful to the requesting server.
    ///
    /// If not supplied, the current time as determined by the receiving server is used.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default = "UnixMillis::now")]
    pub minimum_valid_until_ts: UnixMillis,
}

/// `GET /_matrix/federation/*/version`
///
/// Get the implementation name and version of this homeserver.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1version

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/federation/v1/version",
//     }
// };

/// Response type for the `get_server_version` endpoint.
#[derive(ToSchema, Serialize, Default, Debug)]

pub struct ServerVersionResBody {
    /// Information about the homeserver implementation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,
}

impl ServerVersionResBody {
    /// Creates an empty `Response`.
    pub fn new() -> Self {
        Default::default()
    }
}

/// Endpoint to receive metadata about implemented matrix versions.
///
/// Get the supported matrix versions of this homeserver
/// [GET /_matrix/federation/versions](https://github.com/matrix-org/matrix-spec-proposals/pull/3723)
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         unstable => "/_matrix/federation/unstable/org.matrix.msc3723/versions",
//     }
// };

/// Response type for the `get_server_versions` endpoint.
#[derive(ToSchema, Serialize, Default, Debug)]

pub struct ServerVersionsResBody {
    /// A list of Matrix Server API protocol versions supported by the homeserver.
    pub versions: Vec<String>,
}
impl ServerVersionsResBody {
    /// Creates an empty `Response`.
    pub fn new() -> Self {
        Default::default()
    }
}

/// Response type for the `get_remote_server_keys` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct RemoteServerKeysResBody {
    /// The queried server's keys, signed by the notary server.
    pub server_keys: Vec<RawJson<ServerSigningKeys>>,
}
impl RemoteServerKeysResBody {
    /// Creates a new `Response` with the given keys.
    pub fn new(server_keys: Vec<RawJson<ServerSigningKeys>>) -> Self {
        Self { server_keys }
    }
}

/// `GET /_matrix/key/*/server`
///
/// Get the homeserver's published signing keys.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixkeyv2server
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/key/v2/server",
//     }
// };

/// Response type for the `get_server_keys` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct ServerKeysResBody {
    /// Queried server key, signed by the notary server.
    pub server_key: RawJson<ServerSigningKeys>,
}

impl ServerKeysResBody {
    /// Creates a new `Response` with the given server key.
    pub fn new(server_key: RawJson<ServerSigningKeys>) -> Self {
        Self { server_key }
    }
}

impl From<RawJson<ServerSigningKeys>> for ServerKeysResBody {
    fn from(server_key: RawJson<ServerSigningKeys>) -> Self {
        Self::new(server_key)
    }
}
