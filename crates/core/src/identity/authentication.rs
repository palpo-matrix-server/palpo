use std::time::Duration;

use crate::{authentication::TokenType, OwnedServerName, OwnedUserId};

/// `GET /_matrix/identity/*/account`
///
/// Get information about what user owns the access token used in the request.

/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#get_matrixidentityv2account
// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/account",
//     }
// };

/// Response type for the `get_account_information` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct AccountInfoResBody {
    /// The user ID which registered the token.
    pub user_id: OwnedUserId,
}
impl AccountInfoResBody {
    /// Creates a new `Response` with the given `UserId`.
    pub fn new(user_id: OwnedUserId) -> Self {
        Self { user_id }
    }
}

/// `POST /_matrix/identity/*/account/register`
///
/// Exchanges an OpenID token from the homeserver for an access token to access the identity server.

/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#post_matrixidentityv2accountregister

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/identity/v2/account/register",
//     }
// };

/// Request type for the `register_account` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct RegisterAccountReqBody {
    /// An access token the consumer may use to verify the identity of the person who generated
    /// the token.
    ///
    /// This is given to the federation API `GET /openid/userinfo` to verify the user's
    /// identity.
    pub access_token: String,

    /// The string `Bearer`.
    pub token_type: TokenType,

    /// The homeserver domain the consumer should use when attempting to verify the user's
    /// identity.
    pub matrix_server_name: OwnedServerName,

    /// The number of seconds before this token expires and a new one must be generated.
    #[serde(with = "crate::serde::duration::secs")]
    pub expires_in: Duration,
}

/// Response type for the `register_account` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct RegisterAccountResBody {
    /// An opaque string representing the token to authenticate future requests to the identity
    /// server with.
    pub token: String,
}
impl RegisterAccountResBody {
    /// Creates an empty `Response`.
    pub fn new(token: String) -> Self {
        Self { token }
    }
}
