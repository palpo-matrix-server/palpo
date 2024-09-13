//! `PUT /_matrix/client/*/rooms/{room_id}/redact/{event_id}/{txn_id}`
//!
//! Redact an event, stripping all information not critical to the event graph integrity.

//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3roomsroomidredacteventidtxnid

use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{OwnedEventId, OwnedRoomId, OwnedTransactionId};

// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/rooms/:room_id/redact/:event_id/:txn_id",
//         1.1 => "/_matrix/client/v3/rooms/:room_id/redact/:event_id/:txn_id",
//     }
// };

#[derive(ToParameters, Deserialize, Debug)]
pub struct RedactEventReqArgs {
    /// The ID of the room of the event to redact.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The ID of the event to redact.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,

    /// The transaction ID for this event.
    ///
    /// Clients should generate a unique ID across requests within the
    /// same session. A session is identified by an access token, and
    /// persists when the [access token is refreshed].
    ///
    /// It will be used by the server to ensure idempotency of requests.
    ///
    /// [access token is refreshed]: https://spec.matrix.org/latest/client-server-api/#refreshing-access-tokens
    #[salvo(parameter(parameter_in = Path))]
    pub txn_id: OwnedTransactionId,
}

/// Request type for the `redact_event` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct RedactEventReqBody {
    /// The reason for the redaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Response type for the `redact_event` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct RedactEventResBody {
    /// The ID of the redacted event.
    pub event_id: OwnedEventId,
}
