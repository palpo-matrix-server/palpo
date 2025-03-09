//! Endpoints for third party lookups

// `GET /_matrix/app/*/thirdparty/location/{protocol}`
//
// Retrieve a list of Matrix portal rooms that lead to the matched third party location.
// `/v1/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/application-service-api/#get_matrixappv1thirdpartylocationprotocol
use std::collections::BTreeMap;

use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::third_party::{Location, Protocol, User};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/app/v1/thirdparty/location/:protocol",
//     }
// };

/// Request type for the `get_location_for_protocol` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct ForProtocolReqArgs {
    /// The protocol used to communicate to the third party network.
    #[salvo(parameter(parameter_in = Path))]
    pub protocol: String,

    /// One or more custom fields to help identify the third party location.
    // The specification is incorrect for this parameter. See [matrix-spec#560](https://github.com/matrix-org/matrix-spec/issues/560).
    #[salvo(parameter(parameter_in = Query))]
    pub fields: BTreeMap<String, String>,
}
impl ForProtocolReqArgs {
    /// Creates a new `Request` with the given protocol.
    pub fn new(protocol: String) -> Self {
        Self {
            protocol,
            fields: BTreeMap::new(),
        }
    }
}
// /// Request type for the `get_location_for_room_alias` endpoint.
// #[request]
// pub struct ForRoomAliasReqArgs {
//     /// The Matrix room alias to look up.
//     pub alias: OwnedRoomAliasId,
// }

// impl ForRoomAliasReqBody {
//     /// Creates a new `Request` with the given room alias id.
//     pub fn new(alias: OwnedRoomAliasId) -> Self {
//         Self { alias }
//     }
// }

/// Response type for the `get_location_for_protocol` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct LocationsResBody(
    /// List of matched third party locations.
    pub Vec<Location>,
);

impl LocationsResBody {
    /// Creates a new `Response` with the given locations.
    pub fn new(locations: Vec<Location>) -> Self {
        Self(locations)
    }
}

/// Response type for the `get_user_for_protocol` endpoint.
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

/// Response type for the `get_location_for_protocol` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct UsersResBody {
    /// List of matched third party users.
    pub users: Vec<User>,
}

impl UsersResBody {
    /// Creates a new `Response` with the given users.
    pub fn new(users: Vec<User>) -> Self {
        Self { users }
    }
}
