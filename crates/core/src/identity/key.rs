/// `GET /_matrix/identity/*/pubkey/isvalid`
///
/// Check whether a long-term public key is valid. The response should always be the same, provided
/// the key exists.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2pubkeyisvalid
use crate::{serde::Base64, OwnedServerSigningKeyId};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/identity/v2/pubkey/isvalid",
//     }
// };

// /// Request type for the `check_public_key_validity` endpoint.

// pub struct Requexst {
//     /// Base64-encoded (no padding) public key to check for validity.
//     #[salvo(parameter(parameter_in = Query))]
//     pub public_key: Base64,
// }

/// Response type for the `check_public_key_validity` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct ValidityResBody {
    /// Whether the public key is recognised and is currently valid.
    pub valid: bool,
}
impl ValidityResBody {
    /// Create a `Response` with the given bool indicating the validity of the public key.
    pub fn new(valid: bool) -> Self {
        Self { valid }
    }
}

/// `GET /_matrix/identity/*/pubkey/{keyId}`
///
/// Get the public key for the given key ID.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2pubkeykeyid

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/identity/v2/pubkey/:key_id",
//     }
// };

// /// Request type for the `get_public_key` endpoint.

// pub struct Requxest {
//     /// The ID of the key.
//     #[salvo(parameter(parameter_in = Path))]
//     pub key_id: OwnedServerSigningKeyId,
// }

/// Response type for the `get_public_key` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct PublicKeyResBody {
    /// Unpadded base64-encoded public key.
    pub public_key: Base64,
}
impl PublicKeyResBody {
    /// Create a `Response` with the given base64-encoded (unpadded) public key.
    pub fn new(public_key: Base64) -> Self {
        Self { public_key }
    }
}

/// `GET /_matrix/identity/*/pubkey/ephemeral/isvalid`
///
/// Check whether a short-term public key is valid.
/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2pubkeyephemeralisvalid

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/identity/v2/pubkey/ephemeral/isvalid",
//     }
// };

// /// Request type for the `validate_ephemeral_key` endpoint.

// pub struct Rexquest {
//     /// The unpadded base64-encoded short-term public key to check.
//     #[salvo(parameter(parameter_in = Query))]
//     pub public_key: Base64,
// }

/// Response type for the `validate_ephemeral_key` endpoint.

// pub struct ValidateEphemeralKeyResBody {
//     /// Whether the short-term public key is recognised and is currently valid.
//     pub valid: bool,
// }

// impl ValidateEphemeralKeyResBody {
//     /// Create a `Response` with the given bool indicating the validity of the short-term public
//     /// key.
//     pub fn new(valid: bool) -> Self {
//         Self { valid }
//     }
// }
