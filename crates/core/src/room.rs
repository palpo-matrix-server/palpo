//! Common types for rooms.

use salvo::prelude::*;
use serde::Deserialize;

use crate::{OwnedEventId, OwnedRoomId, OwnedUserId, PrivOwnedStr, events::StateEventType, serde::StringEnum};

/// An enum of possible room types.
#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(ToSchema, Clone, PartialEq, Eq, StringEnum)]
#[non_exhaustive]
pub enum RoomType {
    /// Defines the room as a space.
    #[palpo_enum(rename = "m.space")]
    Space,

    /// Defines the room as a custom type.
    #[doc(hidden)]
    #[salvo(schema(value_type = String))]
    _Custom(PrivOwnedStr),
}

#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(ToSchema, Clone, Default, PartialEq, Eq, StringEnum)]
#[palpo_enum(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Visibility {
    /// Indicates that the room will be shown in the published room list.
    Public,

    /// Indicates that the room will not be shown in the published room list.
    #[default]
    Private,

    #[doc(hidden)]
    #[salvo(schema(value_type = String))]
    _Custom(PrivOwnedStr),
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomEventReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}
#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomTypingReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomUserReqArgs {
    /// Room in which the event to be reported is located.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// Event to report.
    #[salvo(parameter(parameter_in = Path))]
    pub event_id: OwnedEventId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct RoomEventTypeReqArgs {
    /// The ID of the room the event is in.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The ID of the event.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: StateEventType,
}
