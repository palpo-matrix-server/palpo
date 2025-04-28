//! OpenID endpoints.

//! `GET /_matrix/federation/*/openid/userinfo`
//!
//! Exchange an OpenID access token for information about the user who generated
//! the token. `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1openiduserinfo
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::OwnedUserId;

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/federation/v1/openid/userinfo",
//     }
// };

/// Request type for the `get_openid_userinfo` endpoint.

#[derive(ToParameters, Deserialize, Debug)]
pub struct UserInfoReqArgs {
    /// The OpenID access token to get information about the owner for.
    #[salvo(parameter(parameter_in = Query))]
    pub access_token: String,
}

/// Response type for the `get_openid_userinfo` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct UserInfoResBody {
    /// The Matrix User ID who generated the token.
    pub sub: OwnedUserId,
}

impl UserInfoResBody {
    /// Creates a new `Response` with the given user id.
    pub fn new(sub: OwnedUserId) -> Self {
        Self { sub }
    }
}
