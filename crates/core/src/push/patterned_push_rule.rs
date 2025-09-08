//! Common types for the [push notifications module][push].
//!
//! [push]: https://spec.matrix.org/latest/client-server-api/#push-notifications
//!
//! ## Understanding the types of this module
//!
//! Push rules are grouped in `RuleSet`s, and are grouped in five kinds (for
//! more details about the different kind of rules, see the `Ruleset`
//! documentation, or the specification). These five kinds are, by order of
//! priority:
//!
//! - override rules
//! - content rules
//! - room rules
//! - sender rules
//! - underride rules

use std::hash::{Hash, Hasher};

use indexmap::Equivalent;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::push::{
    Action, FlattenedJson, MissingPatternError, PredefinedContentRuleId, PushConditionRoomCtx,
    PushRule, condition,
};

/// Like `SimplePushRule`, but with an additional `pattern` field.
///
/// Only applicable to content rules.
///
/// To create an instance of this type.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct PatternedPushRule {
    /// Actions to determine if and how a notification is delivered for events
    /// matching this rule.
    pub actions: Vec<Action>,

    /// Whether this is a default rule, or has been set explicitly.
    pub default: bool,

    /// Whether the push rule is enabled or not.
    pub enabled: bool,

    /// The ID of this rule.
    pub rule_id: String,

    /// The glob-style pattern to match against.
    pub pattern: String,
}

impl PatternedPushRule {
    /// Check if the push rule applies to the event.
    ///
    /// # Arguments
    ///
    /// * `event` - The flattened JSON representation of a room message event.
    /// * `context` - The context of the room at the time of the event.
    pub fn applies_to(
        &self,
        key: &str,
        event: &FlattenedJson,
        context: &PushConditionRoomCtx,
    ) -> bool {
        // The old mention rules are disabled when an m.mentions field is present.
        #[allow(deprecated)]
        if self.rule_id == PredefinedContentRuleId::ContainsUserName.as_ref()
            && event.contains_mentions()
        {
            return false;
        }

        if event
            .get_str("sender")
            .is_some_and(|sender| sender == context.user_id)
        {
            return false;
        }

        self.enabled && condition::check_event_match(event, key, &self.pattern, context)
    }
}

// The following trait are needed to be able to make
// an IndexSet of the type
impl Hash for PatternedPushRule {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rule_id.hash(state);
    }
}

impl PartialEq for PatternedPushRule {
    fn eq(&self, other: &Self) -> bool {
        self.rule_id == other.rule_id
    }
}

impl Eq for PatternedPushRule {}

impl Equivalent<PatternedPushRule> for str {
    fn equivalent(&self, key: &PatternedPushRule) -> bool {
        self == key.rule_id
    }
}

/// A patterned push rule to update or create.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct NewPatternedPushRule {
    /// The ID of this rule.
    pub rule_id: String,

    /// The glob-style pattern to match against.
    pub pattern: String,

    /// Actions to determine if and how a notification is delivered for events
    /// matching this rule.
    pub actions: Vec<Action>,
}

impl NewPatternedPushRule {
    /// Creates a `NewPatternedPushRule` with the given ID, pattern and actions.
    pub fn new(rule_id: String, pattern: String, actions: Vec<Action>) -> Self {
        Self {
            rule_id,
            pattern,
            actions,
        }
    }
}

impl From<NewPatternedPushRule> for PatternedPushRule {
    fn from(new_rule: NewPatternedPushRule) -> Self {
        let NewPatternedPushRule {
            rule_id,
            pattern,
            actions,
        } = new_rule;
        Self {
            actions,
            default: false,
            enabled: true,
            rule_id,
            pattern,
        }
    }
}

impl From<PatternedPushRule> for PushRule {
    fn from(push_rule: PatternedPushRule) -> Self {
        let PatternedPushRule {
            actions,
            default,
            enabled,
            rule_id,
            pattern,
            ..
        } = push_rule;
        Self {
            actions,
            default,
            enabled,
            rule_id,
            conditions: None,
            pattern: Some(pattern),
        }
    }
}

impl TryFrom<PushRule> for PatternedPushRule {
    type Error = MissingPatternError;

    fn try_from(push_rule: PushRule) -> Result<Self, Self::Error> {
        if let PushRule {
            actions,
            default,
            enabled,
            rule_id,
            pattern: Some(pattern),
            ..
        } = push_rule
        {
            Ok(PatternedPushRule {
                actions,
                default,
                enabled,
                rule_id,
                pattern,
            }
            .into())
        } else {
            Err(MissingPatternError)
        }
    }
}
