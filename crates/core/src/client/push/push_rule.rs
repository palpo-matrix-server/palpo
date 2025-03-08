use salvo::prelude::*;
/// `GET /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}`
///
/// Retrieve a single specified push rule.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushrulesscopekindruleid
use serde::{Deserialize, Serialize};

use crate::push::{Action, PushCondition, PushRule, RuleKind, RuleScope, Ruleset};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id",
//     }
// };

/// Response type for the `get_pushrule` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct RuleResBody {
    /// The specific push rule.
    #[serde(flatten)]
    pub rule: PushRule,
}
impl RuleResBody {
    /// Creates a new `Response` with the given rule.
    pub fn new(rule: PushRule) -> Self {
        Self { rule }
    }
}

/// `GET /_matrix/client/*/pushrules/`
///
/// Retrieve all push rulesets for this user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushrules

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/",
//         1.1 => "/_matrix/client/v3/pushrules/",
//     }
// };
/// Response type for the `get_pushrules_all` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct RulesResBody {
    /// The global ruleset.
    pub global: Ruleset,
}

impl RulesResBody {
    /// Creates a new `Response` with the given global ruleset.
    pub fn new(global: Ruleset) -> Self {
        Self { global }
    }
}

/// `PUT /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}`
///
/// This endpoint allows the creation and modification of push rules for this user ID.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3pushrulesscopekindruleid

// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id",
//     }
// };

// /// Request type for the `set_pushrule` endpoint.
// #[derive(ToSchema, Deserialize, Debug)]
// pub struct SetRuleReqBody {
//     /// The scope to set the rule in.
//     pub scope: RuleScope,

//     /// The rule.
//     pub rule: NewPushRule,

//     /// Use 'before' with a rule_id as its value to make the new rule the
//     /// next-most important rule with respect to the given user defined rule.
//     #[serde(default)]
//     pub before: Option<String>,

//     /// This makes the new rule the next-less important rule relative to the
//     /// given user defined rule.
//     #[serde(default)]
//     pub after: Option<String>,
// }
#[derive(ToParameters, Deserialize, Serialize, Debug)]
pub struct SetRuleReqArgs {
    #[salvo(parameter(parameter_in = Path))]
    pub scope: RuleScope,
    #[salvo(parameter(parameter_in = Path))]
    pub kind: RuleKind,
    #[salvo(parameter(parameter_in = Path))]
    pub rule_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[salvo(parameter(parameter_in = Query))]
    pub before: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[salvo(parameter(parameter_in = Query))]
    pub after: Option<String>,
}

// #[derive(ToSchema, Deserialize, Debug)]
// pub enum SetRuleReqBody {
//     Simple(SimpleReqBody),
//     Patterned(PatternedReqBody),
//     Conditional(ConditionalReqBody),
// }

#[derive(ToSchema, Deserialize, Debug)]
pub struct SimpleReqBody {
    pub actions: Vec<Action>,
}

#[derive(ToSchema, Deserialize, Debug)]
pub struct PatternedReqBody {
    pub actions: Vec<Action>,
    pub pattern: String,
}

#[derive(ToSchema, Deserialize, Debug)]
pub struct ConditionalReqBody {
    pub actions: Vec<Action>,
    pub conditions: Vec<PushCondition>,
}

// impl From<NewPushRule> for SetRuleReqBody {
//     fn from(rule: NewPushRule) -> Self {
//         match rule {
//             NewPushRule::Override(r) => SetRuleReqBody::Conditional(ConditionalReqBody {
//                 actions: r.actions,
//                 conditions: r.conditions,
//             }),
//             NewPushRule::Content(r) => SetRuleReqBody::Patterned(PatternedReqBody {
//                 actions: r.actions,
//                 pattern: r.pattern,
//             }),
//             NewPushRule::Room(r) => SetRuleReqBody::Simple(SimpleReqBody { actions: r.actions }),
//             NewPushRule::Sender(r) => SetRuleReqBody::Simple(SimpleReqBody { actions: r.actions }),
//             NewPushRule::Underride(r) => SetRuleReqBody::Conditional(ConditionalReqBody {
//                 actions: r.actions,
//                 conditions: r.conditions,
//             }),
//             _ => unreachable!("variant added to NewPushRule not covered by SetRuleReqBody"),
//         }
//     }
// }

/// `PUT /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}/enabled`
///
/// This endpoint allows clients to enable or disable the specified push rule.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3pushrulesscopekindruleidenabled
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id/enabled",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id/enabled",
//     }
// };

/// Request type for the `set_pushrule_enabled` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct SetRuleEnabledReqBody {
    // /// The scope to fetch a rule from.
    // #[salvo(parameter(parameter_in = Path))]
    // pub scope: RuleScope,

    // /// The kind of rule
    // #[salvo(parameter(parameter_in = Path))]
    // pub kind: RuleKind,

    // /// The identifier for the rule.
    // #[salvo(parameter(parameter_in = Path))]
    // pub rule_id: String,
    /// Whether the push rule is enabled or not.
    pub enabled: bool,
}

/// `GET /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}/enabled`
///
/// This endpoint gets whether the specified push rule is enabled.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushrulesscopekindruleidenabled

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id/enabled",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id/enabled",
//     }
// };

/// Request type for the `get_pushrule_enabled` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct RuleEnabledReqBody {
    /// The scope to fetch a rule from.
    #[salvo(parameter(parameter_in = Path))]
    pub scope: RuleScope,

    /// The kind of rule
    #[salvo(parameter(parameter_in = Path))]
    pub kind: RuleKind,

    /// The identifier for the rule.
    #[salvo(parameter(parameter_in = Path))]
    pub rule_id: String,
}

/// Response type for the `get_pushrule_enabled` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct RuleEnabledResBody {
    /// Whether the push rule is enabled or not.
    pub enabled: bool,
}
impl RuleEnabledResBody {
    /// Creates a new `Response` with the given enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

/// `DELETE /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}`
///
/// This endpoint removes the push rule defined in the path.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#delete_matrixclientv3pushrulesscopekindruleid
// const METADATA: Metadata = metadata! {
//     method: DELETE,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id",
//     }
// };

// /// Request type for the `delete_pushrule` endpoint.

// pub struct DeleteRuleReqBody {
//     /// The scope to delete from.
//     #[salvo(parameter(parameter_in = Path))]
//     pub scope: RuleScope,

//     /// The kind of rule
//     #[salvo(parameter(parameter_in = Path))]
//     pub kind: RuleKind,

//     /// The identifier for the rule.
//     #[salvo(parameter(parameter_in = Path))]
//     pub rule_id: String,
// }

/// `GET /_matrix/client/*/pushrules/global/`
///
/// Retrieve all push rulesets in the global scope for this user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushrules
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/global/",
//         1.1 => "/_matrix/client/v3/pushrules/global/",
//     }
// };

/// Response type for the `get_pushrules_global_scope` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct DeleteRuleResBody {
    /// The global ruleset.
    #[serde(flatten)]
    pub global: Ruleset,
}

impl DeleteRuleResBody {
    /// Creates a new `Response` with the given global ruleset.
    pub fn new(global: Ruleset) -> Self {
        Self { global }
    }
}

/// `GET /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}/actions`
///
/// This endpoint get the actions for the specified push rule.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushrulesscopekindruleidactions
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id/actions",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id/actions",
//     }
// };

// /// Request type for the `get_pushrule_actions` endpoint.

// pub struct RuleActionsReqBody {
//     /// The scope to fetch a rule from.
//     #[salvo(parameter(parameter_in = Path))]
//     pub scope: RuleScope,

//     /// The kind of rule
//     #[salvo(parameter(parameter_in = Path))]
//     pub kind: RuleKind,

//     /// The identifier for the rule.
//     #[salvo(parameter(parameter_in = Path))]
//     pub rule_id: String,
// }

/// Response type for the `get_pushrule_actions` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct RuleActionsResBody {
    /// The actions to perform for this rule.
    pub actions: Vec<Action>,
}
impl RuleActionsResBody {
    /// Creates a new `Response` with the given actions.
    pub fn new(actions: Vec<Action>) -> Self {
        Self { actions }
    }
}

/// `PUT /_matrix/client/*/pushrules/{scope}/{kind}/{rule_id}/actions`
///
/// This endpoint allows clients to change the actions of a push rule. This can be used to change
/// the actions of builtin rules.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3pushrulesscopekindruleidactions
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushrules/:scope/:kind/:rule_id/actions",
//         1.1 => "/_matrix/client/v3/pushrules/:scope/:kind/:rule_id/actions",
//     }
// };

/// Request type for the `set_pushrule_actions` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct SetRuleActionsReqBody {
    // /// The scope to fetch a rule from.
    // #[salvo(parameter(parameter_in = Path))]
    // pub scope: RuleScope,

    // /// The kind of rule
    // #[salvo(parameter(parameter_in = Path))]
    // pub kind: RuleKind,

    // /// The identifier for the rule.
    // #[salvo(parameter(parameter_in = Path))]
    // pub rule_id: String,
    /// The actions to perform for this rule
    pub actions: Vec<Action>,
}
