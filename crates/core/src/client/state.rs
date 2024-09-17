use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::events::{AnyStateEvent, AnyStateEventContent, StateEventType};
use crate::{serde::RawJson, OwnedEventId, OwnedRoomId, UnixMillis};

/// `GET /_matrix/client/*/rooms/{room_id}/state/{eventType}/{stateKey}`
///
/// Get state events associated with a given key.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3roomsroomidstateeventtypestatekey

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/rooms/:room_id/state/:event_type/:state_key",
//         1.1 => "/_matrix/client/v3/rooms/:room_id/state/:event_type/:state_key",
//     }
// };

/// Request type for the `get_state_events_for_key` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct StateEventsForKeyReqArgs {
    /// The room to look up the state for.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The type of state to look up.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: StateEventType,

    /// The key of the state to look up.
    #[salvo(parameter(parameter_in = Path))]
    pub state_key: String,
}

/// Response type for the `get_state_events_for_key` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct StateEventsForKeyResBody(
    /// The content of the state event.
    ///
    /// Since the inner type of the `Raw` does not implement `Deserialize`, you need to use
    /// [`RawJson::deserialize_as`] to deserialize it.
    pub RawJson<AnyStateEventContent>,
);
impl StateEventsForKeyResBody {
    /// Creates a new `Response` with the given content.
    pub fn new(content: RawJson<AnyStateEventContent>) -> Self {
        Self(content)
    }
}

/// `GET /_matrix/client/*/rooms/{room_id}/state`
///
/// Get state events for a room.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3roomsroomidstate

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/rooms/:room_id/state",
//         1.1 => "/_matrix/client/v3/rooms/:room_id/state",
//     }
// };

// /// Request type for the `get_state_events` endpoint.

// pub struct StateEventsReqBody {
//     /// The room to look up the state for.
//     #[salvo(parameter(parameter_in = Path))]
//     pub room_id: OwnedRoomId,
// }

/// Response type for the `get_state_events` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct StateEventsResBody {
    /// If the user is a member of the room this will be the current state of the room as a
    /// list of events.
    ///
    /// If the user has left the room then this will be the state of the room when they left as
    /// a list of events.
    #[salvo(schema(value_type = Vec<Object>, additional_properties = true))]
    pub room_state: Vec<RawJson<AnyStateEvent>>,
}
impl StateEventsResBody {
    /// Creates a new `Response` with the given room state.
    pub fn new(room_state: Vec<RawJson<AnyStateEvent>>) -> Self {
        Self { room_state }
    }
}

/// `PUT /_matrix/client/*/rooms/{room_id}/state/{eventType}/{stateKey}`
///
/// Send a state event to a room associated with a given state key.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/rooms/:room_id/state/:event_type/:state_key",
//         1.1 => "/_matrix/client/v3/rooms/:room_id/state/:event_type/:state_key",
//     }
// };

/// Request type for the `send_state_event` endpoint.
// #[derive(ToSchema, Deserialize, Debug)]
// pub struct SendStateEventReqBody {
//     // /// The room to set the state in.
//     // pub room_id: OwnedRoomId,

//     // /// The type of event to send.
//     // pub event_type: StateEventType,

//     /// The state_key for the state to send.
//     pub state_key: Option<String>,

//     /// The event content to send.
//     #[salvo(schema(value_type = Object, additional_properties = true))]
//     pub body: RawJson<AnyStateEventContent>,

//     /// Timestamp to use for the `origin_server_ts` of the event.
//     ///
//     /// This is called [timestamp massaging] and can only be used by Appservices.
//     ///
//     /// Note that this does not change the position of the event in the timeline.
//     ///
//     /// [timestamp massaging]: https://spec.matrix.org/latest/application-service-api/#timestamp-massaging
//     pub timestamp: Option<UnixMillis>,
// }
#[derive(ToSchema, Deserialize, Debug)]
pub struct SendStateEventReqBody (
    /// The event content to send.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub RawJson<AnyStateEventContent>
);

/// Response type for the `send_state_event` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct SendStateEventResBody {
    /// A unique identifier for the event.
    pub event_id: OwnedEventId,
}
impl SendStateEventResBody {
    /// Creates a new `Response` with the given event id.
    pub fn new(event_id: OwnedEventId) -> Self {
        Self { event_id }
    }
}

/// Data in the request's query string.
#[derive(Serialize, Deserialize, Debug)]
struct RequestQuery {
    /// Timestamp to use for the `origin_server_ts` of the event.
    #[serde(default, rename = "ts", skip_serializing_if = "Option::is_none")]
    timestamp: Option<UnixMillis>,
}
