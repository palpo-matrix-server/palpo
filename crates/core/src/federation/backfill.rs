//! Endpoints to request more history from another homeserver.
//! `GET /_matrix/federation/*/backfill/{room_id}`
//!
//! Get more history from another homeserver.
//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1backfillroomid

use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::sending::{SendRequest, SendResult};
use crate::serde::RawJsonValue;
use crate::{EventId, OwnedEventId, OwnedRoomId, OwnedServerName, RoomId, ServerName, UnixMillis};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/backfill/:room_id",
//     }
// };

pub fn backfill_request(
    server: &ServerName,
    room_id: &RoomId,
    event_id: &EventId,
    limit: usize,
) -> SendResult<SendRequest> {
    Ok(crate::sending::get(server.build_url(&format!(
        "/federation/v1/backfill/{room_id}&limit={limit}&v={event_id}"
    ))?))
}

/// Request type for the `get_backfill` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct BackfillReqArgs {
    /// The room ID to backfill.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event IDs to backfill from.
    #[salvo(parameter(parameter_in = Query))]
    pub v: Vec<OwnedEventId>,

    /// The maximum number of PDUs to retrieve, including the given events.
    #[salvo(parameter(parameter_in = Query))]
    pub limit: usize,
}

/// Response type for the `get_backfill` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]

pub struct BackfillResBody {
    /// The `server_name` of the homeserver sending this transaction.
    pub origin: OwnedServerName,

    /// POSIX timestamp in milliseconds on originating homeserver when this transaction
    /// started.
    pub origin_server_ts: UnixMillis,

    /// List of persistent updates to rooms.
    #[salvo(schema(value_type = Vec<Object>))]
    pub pdus: Vec<Box<RawJsonValue>>,
}
impl BackfillResBody {
    /// Creates a new `Response` with:
    /// * the `server_name` of the homeserver.
    /// * the timestamp in milliseconds of when this transaction started.
    /// * the list of persistent updates to rooms.
    pub fn new(origin: OwnedServerName, origin_server_ts: UnixMillis, pdus: Vec<Box<RawJsonValue>>) -> Self {
        Self {
            origin,
            origin_server_ts,
            pdus,
        }
    }
}
