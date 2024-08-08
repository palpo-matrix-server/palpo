/// `POST /_matrix/identity/*/terms`
///
/// Send acceptance of the terms of service of an identity server.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#post_matrixidentityv2terms
// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/terms",
//     }
// };
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
/// Request type for the `accept_terms_of_service` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct AcceptTosReqBody {
    /// The URLs the user is accepting in this request.
    ///
    /// An example is `https://example.org/somewhere/terms-2.0-en.html`.
    pub user_accepts: Vec<String>,
}

/// `GET /_matrix/identity/*/terms`
///
/// Get the terms of service of an identity server.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2terms

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/identity/v2/terms",
//     }
// };

/// Response type for the `get_terms_of_service` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct TosResBody {
    /// The policies the server offers.
    ///
    /// Mapped from arbitrary ID (unused in this version of the specification) to a Policy
    /// Object.
    pub policies: BTreeMap<String, Policies>,
}

impl TosResBody {
    /// Creates a new `Response` with the given `Policies`.
    pub fn new(policies: BTreeMap<String, Policies>) -> Self {
        Self { policies }
    }
}

/// Collection of localized policies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policies {
    /// The version for the policy.
    ///
    /// There are no requirements on what this might be and could be
    /// "alpha", semantically versioned, or arbitrary.
    pub version: String,

    /// Available languages for the policy.
    ///
    /// The keys could be the language code corresponding to
    /// the given `LocalizedPolicy`, for example "en" or "fr".
    #[serde(flatten)]
    pub localized: BTreeMap<String, LocalizedPolicy>,
}

impl Policies {
    /// Create a new `Policies` with the given version and localized map.
    pub fn new(version: String, localized: BTreeMap<String, LocalizedPolicy>) -> Self {
        Self { version, localized }
    }
}

/// A localized policy offered by a server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalizedPolicy {
    /// The localized name of the policy.
    ///
    /// Examples are "Terms of Service", "Conditions d'utilisation".
    pub name: String,

    /// The URL at which the policy is available.
    ///
    /// Examples are `https://example.org/somewhere/terms-2.0-en.html
    /// and `https://example.org/somewhere/terms-2.0-fr.html`.
    pub url: String,
}

impl LocalizedPolicy {
    /// Create a new `LocalizedPolicy` with the given name and url.
    pub fn new(name: String, url: String) -> Self {
        Self { name, url }
    }
}
