//! Types for the [`m.room.topic`] event.
//!
//! [`m.room.topic`]: https://spec.matrix.org/latest/client-server-api/#mroomtopic

use palpo_macros::EventContent;
use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::events::EmptyStateKey;

/// The content of an `m.room.topic` event.
///
/// A topic is a short message detailing what is currently being discussed in
/// the room.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug, EventContent)]
#[palpo_event(type = "m.room.topic", kind = State, state_key_type = EmptyStateKey)]
pub struct RoomTopicEventContent {
    /// The topic text.
    pub topic: String,
}

impl RoomTopicEventContent {
    /// Creates a new `RoomTopicEventContent` with the given topic.
    pub fn new(topic: String) -> Self {
        Self { topic }
    }
}
