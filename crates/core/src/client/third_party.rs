use std::collections::BTreeMap;

use salvo::prelude::*;
use serde::Serialize;

use crate::{third_party::Location, third_party::Protocol, third_party::User};

// `GET /_matrix/client/*/thirdparty/protocols`
//
// Fetches the overall metadata about protocols supported by the homeserver.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartyprotocols
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/thirdparty/protocols",
//         1.1 => "/_matrix/client/v3/thirdparty/protocols",
//     }
// };

/// Response type for the `get_protocols` endpoint.

#[derive(ToSchema, Serialize, Default, Debug)]

pub struct ProtocolsResBody {
    /// Metadata about protocols supported by the homeserver.
    pub protocols: BTreeMap<String, Protocol>,
}

impl ProtocolsResBody {
    /// Creates a new `Response` with the given protocols.
    pub fn new(protocols: BTreeMap<String, Protocol>) -> Self {
        Self { protocols }
    }
}

// `GET /_matrix/client/*/thirdparty/protocol/{protocol}`
//
// Fetches the metadata from the homeserver about a particular third party protocol.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartyprotocolprotocol
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/thirdparty/protocol/:protocol",
//         1.1 => "/_matrix/client/v3/thirdparty/protocol/:protocol",
//     }
// };

/// Request type for the `get_protocol` endpoint.
// pub struct ProtocolReqBody {
//     /// The name of the protocol.
//     #[salvo(parameter(parameter_in = Path))]
//     pub protocol: String,
// }

/// Response type for the `get_protocol` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct ProtocolResBody {
    /// Metadata about the protocol.
    pub protocol: Protocol,
}
impl ProtocolResBody {
    /// Creates a new `Response` with the given protocol.
    pub fn new(protocol: Protocol) -> Self {
        Self { protocol }
    }
}

// `GET /_matrix/client/*/thirdparty/location/{protocol}`
//
// Fetches third party locations for a protocol.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartylocationprotocol
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/thirdparty/location/:protocol",
//         1.1 => "/_matrix/client/v3/thirdparty/location/:protocol",
//     }
// };

/// Request type for the `get_location_for_protocol` endpoint.
// pub struct LocationForProtocolReqBody {
//     /// The protocol used to communicate to the third party network.
//     #[salvo(parameter(parameter_in = Path))]
//     pub protocol: String,

//     /// One or more custom fields to help identify the third party location.
//     // The specification is incorrect for this parameter. See [matrix-spec#560](https://github.com/matrix-org/matrix-spec/issues/560).
//
//     pub fields: BTreeMap<String, String>,
// }

/// Response type for the `get_location_for_protocol` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct LocationForProtocolResBody {
    /// List of matched third party locations.
    pub locations: Vec<Location>,
}

impl LocationForProtocolResBody {
    /// Creates a new `Response` with the given locations.
    pub fn new(locations: Vec<Location>) -> Self {
        Self { locations }
    }
}

// `GET /_matrix/client/*/thirdparty/location`
//
// Retrieve an array of third party network locations from a Matrix room alias.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartylocation
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/thirdparty/location",
//         1.1 => "/_matrix/client/v3/thirdparty/location",
//     }
// };

/// Request type for the `get_location_for_room_alias` endpoint.

// pub struct LocationReqBody {
//     /// The Matrix room alias to look up.
//     #[salvo(parameter(parameter_in = Query))]
//     pub alias: OwnedRoomAliasId,
// }

/// Response type for the `get_location_for_room_alias` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct LocationResBody {
    /// List of matched third party locations.
    pub locations: Vec<Location>,
}

impl LocationResBody {
    /// Creates a new `Response` with the given locations.
    pub fn new(locations: Vec<Location>) -> Self {
        Self { locations }
    }
}

/// `GET /_matrix/client/*/thirdparty/user/{protocol}`
///
/// Fetches third party users for a protocol.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartyuserprotocol
/// const METADATA: Metadata = metadata! {
///     method: GET,
///     rate_limited: false,
///     authentication: AccessToken,
///     history: {
///         1.0 => "/_matrix/client/r0/thirdparty/user/:protocol",
///         1.1 => "/_matrix/client/v3/thirdparty/user/:protocol",
///     }
/// };

/// Request type for the `get_user_for_protocol` endpoint.

// pub struct UserForProtocolReqBody {
//     /// The protocol used to communicate to the third party network.
//     #[salvo(parameter(parameter_in = Path))]
//     pub protocol: String,

//     /// One or more custom fields that are passed to the AS to help identify the user.
//     // The specification is incorrect for this parameter. See [matrix-spec#560](https://github.com/matrix-org/matrix-spec/issues/560).
//
//     pub fields: BTreeMap<String, String>,
// }

/// Response type for the `get_user_for_protocol` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct UserForProtocolResBody {
    /// List of matched third party users.
    pub users: Vec<User>,
}

impl UserForProtocolResBody {
    /// Creates a new `Response` with the given users.
    pub fn new(users: Vec<User>) -> Self {
        Self { users }
    }
}

/// `GET /_matrix/client/*/thirdparty/user`
///
/// Retrieve an array of third party users from a Matrix User ID.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3thirdpartyuser
/// const METADATA: Metadata = metadata! {
///     method: GET,
///     rate_limited: false,
///     authentication: AccessToken,
///     history: {
///         1.0 => "/_matrix/client/r0/thirdparty/user",
///         1.1 => "/_matrix/client/v3/thirdparty/user",
///     }
/// };

/// Response type for the `get_user_for_user_id` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct UserResBody {
    /// List of matched third party users.
    pub users: Vec<User>,
}

impl UserResBody {
    /// Creates a new `ThirdPartyUserResBody` with the given users.
    pub fn new(users: Vec<User>) -> Self {
        Self { users }
    }
}
