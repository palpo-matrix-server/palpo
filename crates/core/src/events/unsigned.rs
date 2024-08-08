use salvo::oapi::ToSchema;
use serde::{de::DeserializeOwned, Deserialize};

use super::{
    relation::{BundledMessageLikeRelations, BundledStateRelations},
    room::redaction::RoomRedactionEventContent,
    MessageLikeEventContent, OriginalSyncMessageLikeEvent, PossiblyRedactedStateEventContent,
};
use crate::{serde::CanBeEmpty, OwnedEventId, OwnedTransactionId, OwnedUserId, UnixMillis};

/// Extra information about a message event that is not incorporated into the event's hash.
#[derive(ToSchema, Clone, Debug, Deserialize)]
#[serde(bound = "OriginalSyncMessageLikeEvent<C>: DeserializeOwned")]
pub struct MessageLikeUnsigned<C: MessageLikeEventContent> {
    /// The time in milliseconds that has elapsed since the event was sent.
    ///
    /// This field is generated by the local homeserver, and may be incorrect if the local time on
    /// at least one of the two servers is out of sync, which can cause the age to either be
    /// negative or greater than it actually is.
    pub age: Option<i64>,

    /// The client-supplied transaction ID, if the client being given the event is the same one
    /// which sent it.
    pub transaction_id: Option<OwnedTransactionId>,

    /// [Bundled aggregations] of related child events.
    ///
    /// [Bundled aggregations]: https://spec.matrix.org/latest/client-server-api/#aggregations-of-child-events
    #[serde(rename = "m.relations", default)]
    pub relations: BundledMessageLikeRelations<OriginalSyncMessageLikeEvent<C>>,
}

impl<C: MessageLikeEventContent> MessageLikeUnsigned<C> {
    /// Create a new `Unsigned` with fields set to `None`.
    pub fn new() -> Self {
        Self {
            age: None,
            transaction_id: None,
            relations: BundledMessageLikeRelations::default(),
        }
    }
}

impl<C: MessageLikeEventContent> Default for MessageLikeUnsigned<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: MessageLikeEventContent> CanBeEmpty for MessageLikeUnsigned<C> {
    /// Whether this unsigned data is empty (all fields are `None`).
    ///
    /// This method is used to determine whether to skip serializing the `unsigned` field in room
    /// events. Do not use it to determine whether an incoming `unsigned` field was present - it
    /// could still have been present but contained none of the known fields.
    fn is_empty(&self) -> bool {
        self.age.is_none() && self.transaction_id.is_none() && self.relations.is_empty()
    }
}

/// Extra information about a state event that is not incorporated into the event's hash.
#[derive(ToSchema, Clone, Debug, Deserialize)]
pub struct StateUnsigned<C: PossiblyRedactedStateEventContent> {
    /// The time in milliseconds that has elapsed since the event was sent.
    ///
    /// This field is generated by the local homeserver, and may be incorrect if the local time on
    /// at least one of the two servers is out of sync, which can cause the age to either be
    /// negative or greater than it actually is.
    pub age: Option<i64>,

    /// The client-supplied transaction ID, if the client being given the event is the same one
    /// which sent it.
    pub transaction_id: Option<OwnedTransactionId>,

    /// Optional previous content of the event.
    pub prev_content: Option<C>,

    /// [Bundled aggregations] of related child events.
    ///
    /// [Bundled aggregations]: https://spec.matrix.org/latest/client-server-api/#aggregations-of-child-events
    #[serde(rename = "m.relations", default)]
    pub relations: BundledStateRelations,
}

impl<C: PossiblyRedactedStateEventContent> StateUnsigned<C> {
    /// Create a new `Unsigned` with fields set to `None`.
    pub fn new() -> Self {
        Self {
            age: None,
            transaction_id: None,
            prev_content: None,
            relations: Default::default(),
        }
    }
}

impl<C: PossiblyRedactedStateEventContent> CanBeEmpty for StateUnsigned<C> {
    /// Whether this unsigned data is empty (all fields are `None`).
    ///
    /// This method is used to determine whether to skip serializing the `unsigned` field in room
    /// events. Do not use it to determine whether an incoming `unsigned` field was present - it
    /// could still have been present but contained none of the known fields.
    fn is_empty(&self) -> bool {
        self.age.is_none() && self.transaction_id.is_none() && self.prev_content.is_none() && self.relations.is_empty()
    }
}

impl<C: PossiblyRedactedStateEventContent> Default for StateUnsigned<C> {
    fn default() -> Self {
        Self::new()
    }
}

/// Extra information about a redacted event that is not incorporated into the event's hash.
#[derive(ToSchema, Clone, Debug, Deserialize)]
pub struct RedactedUnsigned {
    /// The event that redacted this event, if any.
    pub redacted_because: UnsignedRoomRedactionEvent,
}

impl RedactedUnsigned {
    /// Create a new `RedactedUnsigned` with the given redaction event.
    pub fn new(redacted_because: UnsignedRoomRedactionEvent) -> Self {
        Self { redacted_because }
    }
}

/// A redaction event as found in `unsigned.redacted_because`.
///
/// While servers usually send this with the `redacts` field (unless nested), the ID of the event
/// being redacted is known from context wherever this type is used, so it's not reflected as a
/// field here.
///
/// It is intentionally not possible to create an instance of this type other than through `Clone`
/// or `Deserialize`.
#[derive(ToSchema, Clone, Debug, Deserialize)]
#[non_exhaustive]
pub struct UnsignedRoomRedactionEvent {
    /// Data specific to the event type.
    pub content: RoomRedactionEventContent,

    /// The globally unique event identifier for the user who sent the event.
    pub event_id: OwnedEventId,

    /// The fully-qualified ID of the user who sent this event.
    pub sender: OwnedUserId,

    /// Timestamp in milliseconds on originating homeserver when this event was sent.
    pub origin_server_ts: UnixMillis,

    /// Additional key-value pairs not signed by the homeserver.
    #[serde(default)]
    pub unsigned: MessageLikeUnsigned<RoomRedactionEventContent>,
}
