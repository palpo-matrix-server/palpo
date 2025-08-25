//! Types to deserialize `m.room.member` events.
use std::collections::BTreeMap;
use std::ops::Deref;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::events::room::member::{ThirdPartyInvite, MembershipState};
use crate::macros::EventContent;
use crate::serde::JsonValue;
use crate::state::Event;
use crate::{
    PrivOwnedStr,
    events::{
        AnyStrippedStateEvent, BundledStateRelations, EventContent, EventContentFromType,
        PossiblyRedactedStateEventContent, RedactContent, RedactedStateEventContent,
        StateEventType,
    },
    identifiers::*,
    serde::{CanBeEmpty, RawJson, RawJsonValue, StringEnum},
};

/// A helper type for an [`Event`] of type `m.room.member`.
///
/// This is a type that deserializes each field lazily, as requested.
#[derive(Debug, Clone)]
pub struct RoomMemberEvent<E: Event>(E);

impl<E: Event> RoomMemberEvent<E> {
    /// Construct a new `RoomMemberEvent` around the given event.
    pub fn new(event: E) -> Self {
        Self(event)
    }

    /// The membership of the user.
    pub fn membership(&self) -> Result<MembershipState, String> {
        RoomMemberEventContent(self.content()).membership()
    }

    /// If this is a `join` event, the ID of a user on the homeserver that authorized it.
    pub fn join_authorised_via_users_server(&self) -> Result<Option<OwnedUserId>, String> {
        RoomMemberEventContent(self.content()).join_authorised_via_users_server()
    }

    /// If this is an `invite` event, details about the third-party invite that resulted in this
    /// event.
    pub(crate) fn third_party_invite(&self) -> Result<Option<ThirdPartyInvite>, String> {
        RoomMemberEventContent(self.content()).third_party_invite()
    }
}

impl<E: Event> Deref for RoomMemberEvent<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Helper trait for `Option<RoomMemberEvent<E>>`.
pub(crate) trait RoomMemberEventOptionExt {
    /// The membership of the user.
    ///
    /// Defaults to `leave` if there is no `m.room.member` event.
    fn membership(&self) -> Result<MembershipState, String>;
}

impl<E: Event> RoomMemberEventOptionExt for Option<RoomMemberEvent<E>> {
    fn membership(&self) -> Result<MembershipState, String> {
        match self {
            Some(event) => event.membership(),
            None => Ok(MembershipState::Leave),
        }
    }
}
