use salvo::prelude::*;
use serde::Deserialize;

use crate::events::{GlobalAccountDataEventType, StateEventType};
use crate::push::{RuleKind, RuleScope};
use crate::{OwnedEventId, OwnedRoomId, OwnedUserId};

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
pub struct UserRoomReqArgs {
    /// The user whose tags will be retrieved.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The room from which tags will be retrieved.
    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserEventTypeReqArgs {
    /// The ID of the user to set account_data for.
    ///
    /// The access token must be authorized to make requests for this user ID.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The event type of the account_data to set.
    ///
    /// Custom types should be namespaced to avoid clashes.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: GlobalAccountDataEventType,
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

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserRoomEventTypeReqArgs {
    /// The ID of the user to set account_data for.
    ///
    /// The access token must be authorized to make requests for this user ID.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    #[salvo(parameter(parameter_in = Path))]
    pub room_id: OwnedRoomId,

    /// The event type of the account_data to set.
    ///
    /// Custom types should be namespaced to avoid clashes.
    #[salvo(parameter(parameter_in = Path))]
    pub event_type: GlobalAccountDataEventType,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserFilterReqArgs {
    /// The user ID to download a filter for.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The ID of the filter to download.
    #[salvo(parameter(parameter_in = Path))]
    pub filter_id: String,
}

#[derive(ToParameters, Deserialize, Debug)]
pub struct ScopeKindRuleReqArgs {
    /// The scope to fetch rules from.
    #[salvo(parameter(parameter_in = Path))]
    pub scope: RuleScope,

    /// The kind of rule.
    #[salvo(parameter(parameter_in = Path))]
    pub kind: RuleKind,

    /// The identifier for the rule.
    #[salvo(parameter(parameter_in = Path))]
    pub rule_id: String,
}
