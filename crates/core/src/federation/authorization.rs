//! Endpoints to retrieve the complete auth chain for a given event.
//! `GET /_matrix/federation/*/event_auth/{room_id}/{event_id}`
//!
//! Get the complete auth chain for a given event.
//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1event_authroomideventid

use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::identifiers::*;
use crate::sending::{SendRequest, SendResult};
use crate::serde::RawJsonValue;

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/event_auth/:room_id/:event_id",
//     }
// };

pub fn event_authorization_request(origin: &str, room_id: &RoomId, event_id: &EventId) -> SendResult<SendRequest> {
    let url = Url::parse(&format!(
        "{origin}/_matrix/federation/v1/event_auth/{}/{}",
        room_id, event_id
    ))?;
    Ok(crate::sending::get(url))
}

/// Request type for the `get_event_authorization` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct EventAuthorizationReqArgs {
    /// The room ID to get the auth chain for.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event ID to get the auth chain for.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}

/// Response type for the `get_event_authorization` endpoint.
#[derive(ToSchema, Deserialize, Serialize, Debug)]

pub struct EventAuthorizationResBody {
    /// The full set of authorization events that make up the state of the room,
    /// and their authorization events, recursively.
    #[salvo(schema(value_type = Vec<Object>))]
    pub auth_chain: Vec<Box<RawJsonValue>>,
}
impl EventAuthorizationResBody {
    /// Creates a new `Response` with the given auth chain.
    pub fn new(auth_chain: Vec<Box<RawJsonValue>>) -> Self {
        Self { auth_chain }
    }
}
