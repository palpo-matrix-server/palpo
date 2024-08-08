/// Endpoints to look up Matrix IDs bound to 3PIDs.
use std::collections::BTreeMap;

use crate::lookup::IdentifierHashingAlgorithm;
use crate::serde::StringEnum;
use crate::{OwnedUserId, PrivOwnedStr};

/// The algorithms that can be used to hash the identifiers used for lookup, as defined in the
/// Matrix Spec.
///
/// This type can hold an arbitrary string. To build this with a custom value, convert it from a
/// string with `::from()` / `.into()`. To check for values that are not available as a documented
/// variant here, use its string representation, obtained through [`.as_str()`](Self::as_str()).
#[derive(Clone, PartialEq, Eq, StringEnum)]
#[non_exhaustive]
#[palpo_enum(rename_all = "snake_case")]
pub enum IdentifierHashingAlgorithm {
    /// The SHA-256 hashing algorithm.
    Sha256,

    /// No algorithm is used, and identifier strings are directly used for lookup.
    None,

    #[doc(hidden)]
    _Custom(PrivOwnedStr),
}

/// `POST /_matrix/identity/*/lookup`
///
/// Looks up the set of Matrix User IDs which have bound the 3PIDs given, if bindings are available.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#post_matrixidentityv2lookup
// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/lookup",
//     }
// };

/// Request type for the `lookup_3pid` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct LookupThreepidReqBody {
    /// The algorithm the client is using to encode the `addresses`. This should be one of the
    /// available options from `/hash_details`.
    pub algorithm: IdentifierHashingAlgorithm,

    /// The pepper from `/hash_details`. This is required even when the `algorithm` does not
    /// make use of it.
    pub pepper: String,

    /// The addresses to look up.
    ///
    /// The format of the entries here depend on the `algorithm` used. Note that queries which
    /// have been incorrectly hashed or formatted will lead to no matches.
    pub addresses: Vec<String>,
}

/// Response type for the `lookup_3pid` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct LookupThreepidResBody {
    /// Any applicable mappings of `addresses` to Matrix User IDs.
    ///
    /// Addresses which do not have associations will not be included, which can make this
    /// property be an empty object.
    pub mappings: BTreeMap<String, OwnedUserId>,
}
impl LookupThreepidResBody {
    /// Create a `Response` with the BTreeMap which map addresses from the request which were
    /// found to their corresponding User IDs.
    pub fn new(mappings: BTreeMap<String, OwnedUserId>) -> Self {
        Self { mappings }
    }
}

/// `GET /_matrix/identity/*/hash_details`
///
/// Gets parameters for hashing identifiers from the server. This can include any of the algorithms
/// defined in the spec.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2hash_details

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/hash_details",
//     }
// };

/// Response type for the `get_hash_parameters` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct HashParametersResBody {
    /// The pepper the client MUST use in hashing identifiers, and MUST supply to the /lookup
    /// endpoint when performing lookups.
    ///
    /// Servers SHOULD rotate this string often.
    pub lookup_pepper: String,

    /// The algorithms the server supports.
    ///
    /// Must contain at least `sha256`.
    pub algorithms: Vec<IdentifierHashingAlgorithm>,
}
impl HashParametersResBody {
    /// Create a new `Response` using the given pepper and `Vec` of algorithms.
    pub fn new(lookup_pepper: String, algorithms: Vec<IdentifierHashingAlgorithm>) -> Self {
        Self { lookup_pepper, algorithms }
    }
}

#[cfg(test)]
mod tests {
    use super::IdentifierHashingAlgorithm;

    #[test]
    fn parse_identifier_hashing_algorithm() {
        assert_eq!(IdentifierHashingAlgorithm::from("sha256"), IdentifierHashingAlgorithm::Sha256);
        assert_eq!(IdentifierHashingAlgorithm::from("none"), IdentifierHashingAlgorithm::None);
    }
}
