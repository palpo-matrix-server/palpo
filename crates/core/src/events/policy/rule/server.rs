//! Types for the [`m.policy.rule.server`] event.
//!
//! [`m.policy.rule.server`]: https://spec.matrix.org/latest/client-server-api/#mpolicyruleserver

use crate::macros::EventContent;
use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use super::{PolicyRuleEventContent, PossiblyRedactedPolicyRuleEventContent};
use crate::{
    events::{
        EventContent, EventContentFromType, PossiblyRedactedStateEventContent, StateEventType,
    },
    serde::RawJsonValue,
};

/// The content of an `m.policy.rule.server` event.
///
/// This event type is used to apply rules to server entities.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug, EventContent)]
#[allow(clippy::exhaustive_structs)]
#[palpo_event(type = "m.policy.rule.server", kind = State, state_key_type = String, custom_possibly_redacted)]
pub struct PolicyRuleServerEventContent(pub PolicyRuleEventContent);

/// The possibly redacted form of [`PolicyRuleServerEventContent`].
///
/// This type is used when it's not obvious whether the content is redacted or
/// not.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
#[allow(clippy::exhaustive_structs)]
pub struct PossiblyRedactedPolicyRuleServerEventContent(pub PossiblyRedactedPolicyRuleEventContent);

impl EventContent for PossiblyRedactedPolicyRuleServerEventContent {
    type EventType = StateEventType;

    fn event_type(&self) -> Self::EventType {
        StateEventType::PolicyRuleServer
    }
}

impl PossiblyRedactedStateEventContent for PossiblyRedactedPolicyRuleServerEventContent {
    type StateKey = String;

    fn event_type(&self) -> StateEventType {
        StateEventType::PolicyRuleServer
    }
}

impl EventContentFromType for PossiblyRedactedPolicyRuleServerEventContent {
    fn from_parts(_ev_type: &str, content: &RawJsonValue) -> serde_json::Result<Self> {
        serde_json::from_str(content.get())
    }
}
