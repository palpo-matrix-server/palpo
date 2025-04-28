//! Endpoint for sending events.

//! `PUT /_matrix/app/*/transactions/{txn_id}`
//!
//! Endpoint to push an event (or batch of events) to the application service.
//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/application-service-api/#put_matrixappv1transactionstxnid

use reqwest::Url;
use salvo::oapi::ToSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    OwnedRoomId, OwnedUserId,
    events::{AnyTimelineEvent, receipt::ReceiptContent},
    presence::PresenceContent,
    sending::{SendRequest, SendResult},
    serde::{JsonValue, RawJson, RawJsonValue, from_raw_json_value},
};

/// `PUT /_matrix/app/*/transactions/{txn_id}`
///
/// Endpoint to push an event (or batch of events) to the application service.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/application-service-api/#put_matrixappv1transactionstxnid

// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/app/v1/transactions/:txn_id",
//     }
// };

pub fn push_events_request(origin: &str, txn_id: &str, body: PushEventsReqBody) -> SendResult<SendRequest> {
    let url = Url::parse(&format!("{origin}/_matrix/app/v1/transactions/{}", txn_id))?;
    crate::sending::post(url).stuff(body)
}
/// Request type for the `push_events` endpoint.

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct PushEventsReqBody {
    /// The transaction ID for this set of events.
    ///
    /// HomeServers generate these IDs and they are used to ensure idempotency
    /// of results.
    // #[salvo(parameter(parameter_in = Path))]
    // pub txn_id: OwnedTransactionId,

    /// A list of events.
    pub events: Vec<RawJson<AnyTimelineEvent>>,
    // /// Information on E2E device updates.
    // #[serde(
    //     default,
    //     skip_serializing_if = "DeviceLists::is_empty",
    //     rename = "org.matrix.msc3202.device_lists"
    // )]
    // pub device_lists: DeviceLists,

    // /// The number of unclaimed one-time keys currently held on the server for this device, for
    // /// each algorithm.
    // #[serde(
    //     default,
    //     skip_serializing_if = "BTreeMap::is_empty",
    //     rename = "org.matrix.msc3202.device_one_time_keys_count"
    // )]
    // pub device_one_time_keys_count: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, BTreeMap<DeviceKeyAlgorithm,
    // u64>>>,

    // /// A list of key algorithms for which the server has an unused fallback key for the
    // /// device.
    // #[serde(
    //     default,
    //     skip_serializing_if = "BTreeMap::is_empty",
    //     rename = "org.matrix.msc3202.device_unused_fallback_key_types"
    // )]
    // pub device_unused_fallback_key_types: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, Vec<DeviceKeyAlgorithm>>>,

    // /// A list of EDUs.
    // #[serde(
    //     default,
    //     skip_serializing_if = "<[_]>::is_empty",
    //     rename = "de.sorunome.msc2409.ephemeral"
    // )]
    // pub ephemeral: Vec<Edu>,

    // /// A list of to-device messages.

    // #[serde(
    //     default,
    //     skip_serializing_if = "<[_]>::is_empty",
    //     rename = "de.sorunome.msc2409.to_device"
    // )]
    // pub to_device: Vec<RawJson<AnyToDeviceEvent>>,
}
crate::json_body_modifier!(PushEventsReqBody);

/// Type for passing ephemeral data to homeservers.

#[derive(ToSchema, Clone, Debug, Serialize)]
#[non_exhaustive]
pub enum Edu {
    /// An EDU representing presence updates for users of the sending
    /// homeserver.
    Presence(PresenceContent),

    /// An EDU representing receipt updates for users of the sending homeserver.
    #[salvo(schema(value_type = Object))]
    Receipt(ReceiptContent),

    /// A typing notification EDU for a user in a room.
    Typing(TypingContent),

    #[doc(hidden)]
    #[salvo(schema(skip))]
    _Custom(JsonValue),
}

#[derive(Debug, Deserialize)]

struct EduDeHelper {
    /// The message type field
    r#type: String,
    content: Box<RawJsonValue>,
}

impl<'de> Deserialize<'de> for Edu {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawJsonValue>::deserialize(deserializer)?;
        let EduDeHelper { r#type, content } = from_raw_json_value(&json)?;

        Ok(match r#type.as_ref() {
            "m.presence" => Self::Presence(from_raw_json_value(&content)?),
            "m.receipt" => Self::Receipt(from_raw_json_value(&content)?),
            "m.typing" => Self::Typing(from_raw_json_value(&content)?),
            _ => Self::_Custom(from_raw_json_value(&content)?),
        })
    }
}

/// The content for "m.typing" Edu.

#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct TypingContent {
    /// The room where the user's typing status has been updated.
    pub room_id: OwnedRoomId,

    /// The user ID that has had their typing status changed.
    pub user_id: OwnedUserId,

    /// Whether the user is typing in the room or not.
    pub typing: bool,
}

impl TypingContent {
    /// Creates a new `TypingContent`.
    pub fn new(room_id: OwnedRoomId, user_id: OwnedUserId, typing: bool) -> Self {
        Self {
            room_id,
            user_id,
            typing,
        }
    }
}
