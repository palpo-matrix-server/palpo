//! Modules and types for events in the `m.policy.rule` namespace.

pub mod room;
pub mod server;
pub mod user;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::{PrivOwnedStr, serde::StringEnum};

/// The payload for policy rule events.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct PolicyRuleEventContent {
    /// The entity affected by this rule.
    ///
    /// Glob characters `*` and `?` can be used to match zero or more characters
    /// or exactly one character respectively.
    pub entity: String,

    /// The suggested action to take.
    pub recommendation: Recommendation,

    /// The human-readable description for the recommendation.
    pub reason: String,
}

impl PolicyRuleEventContent {
    /// Creates a new `PolicyRuleEventContent` with the given entity,
    /// recommendation and reason.
    pub fn new(entity: String, recommendation: Recommendation, reason: String) -> Self {
        Self {
            entity,
            recommendation,
            reason,
        }
    }
}

/// The possibly redacted form of [`PolicyRuleEventContent`].
///
/// This type is used when it's not obvious whether the content is redacted or
/// not.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct PossiblyRedactedPolicyRuleEventContent {
    /// The entity affected by this rule.
    ///
    /// Glob characters `*` and `?` can be used to match zero or more characters
    /// or exactly one character respectively.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,

    /// The suggested action to take.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<Recommendation>,

    /// The human-readable description for the recommendation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The possible actions that can be taken.
#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(ToSchema, Clone, PartialEq, Eq, StringEnum)]
#[non_exhaustive]
pub enum Recommendation {
    /// Entities affected by the rule should be banned from participation where
    /// possible.
    #[palpo_enum(rename = "m.ban")]
    Ban,

    #[doc(hidden)]
    _Custom(PrivOwnedStr),
}
