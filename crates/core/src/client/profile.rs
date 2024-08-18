/// `GET /_matrix/client/*/profile/{user_id}/avatar_url`
///
/// Get the avatar URL of a user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3profileuser_idavatar_url
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{OwnedMxcUri, OwnedUserId};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/client/r0/profile/:user_id/avatar_url",
//         1.1 => "/_matrix/client/v3/profile/:user_id/avatar_url",
//     }
// };

// /// Request type for the `get_avatar_url` endpoint.

// pub struct Requexst {
//     /// The user whose avatar URL will be retrieved.
//     #[salvo(parameter(parameter_in = Path))]
//     pub user_id: OwnedUserId,
// }

/// Response type for the `get_avatar_url` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct AvatarUrlResBody {
    /// The user's avatar URL, if set.
    #[serde(skip_serializing_if = "Option::is_none",default, deserialize_with = "crate::serde::empty_string_as_none")
    ]
    pub avatar_url: Option<OwnedMxcUri>,

    /// The [BlurHash](https://blurha.sh) for the avatar pointed to by `avatar_url`.
    ///
    /// This uses the unstable prefix in
    /// [MSC2448](https://github.com/matrix-org/matrix-spec-proposals/pull/2448).
    #[serde(rename = "xyz.amorgan.blurhash", skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}
impl AvatarUrlResBody {
    /// Creates a new `Response` with the given avatar URL.
    pub fn new(avatar_url: Option<OwnedMxcUri>) -> Self {
        Self {
            avatar_url,
            blurhash: None,
        }
    }
}

/// `GET /_matrix/client/*/profile/{user_id}/display_name`
///
/// Get the display name of a user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3profileuser_iddisplay_name
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/client/r0/profile/:user_id/display_name",
//         1.1 => "/_matrix/client/v3/profile/:user_id/display_name",
//     }
// };
/// Response type for the `get_display_name` endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct DisplayNameResBody {
    /// The user's display name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl DisplayNameResBody {
    /// Creates a new `Response` with the given display name.
    pub fn new(display_name: Option<String>) -> Self {
        Self { display_name }
    }
}


/// `PUT /_matrix/client/*/profile/{user_id}/avatar_url`
///
/// Set the avatar URL of the user.
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3profileuser_idavatar_url
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/profile/:user_id/avatar_url",
//         1.1 => "/_matrix/client/v3/profile/:user_id/avatar_url",
//     }
// };

/// Request type for the `set_avatar_url` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SetAvatarUrlReqBody {
    /// The new avatar URL for the user.
    ///
    /// `None` is used to unset the avatar.
    #[serde(default, deserialize_with = "crate::serde::empty_string_as_none")]
    pub avatar_url: Option<OwnedMxcUri>,

    /// The [BlurHash](https://blurha.sh) for the avatar pointed to by `avatar_url`.
    ///
    /// This uses the unstable prefix in
    /// [MSC2448](https://github.com/matrix-org/matrix-spec-proposals/pull/2448).
    /// #[cfg(feature = "unstable-msc2448")]
    #[serde(rename = "xyz.amorgan.blurhash", skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}

/// `PUT /_matrix/client/*/profile/{user_id}/display_name`
///
/// Set the display name of the user.

/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#put_matrixclientv3profileuser_iddisplay_name
// const METADATA: Metadata = metadata! {
//     method: PUT,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/profile/:user_id/display_name",
//         1.1 => "/_matrix/client/v3/profile/:user_id/display_name",
//     }
// };

/// Request type for the `set_display_name` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SetDisplayNameReqBody {
    /// The user whose display name will be set.
    #[salvo(parameter(parameter_in = Path))]
    pub user_id: OwnedUserId,

    /// The new display name for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "displayname")]
    pub display_name: Option<String>,
}
