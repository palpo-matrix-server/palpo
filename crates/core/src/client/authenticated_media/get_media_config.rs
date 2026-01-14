//! `GET /_matrix/client/*/media/config`
//!
//! Gets the config for the media repository.

//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv1mediaconfig


use salvo::oapi::{ToParameters, ToSchema};
use serde::{Deserialize, Serialize};

use crate::auth_scheme::AccessToken;

// metadata! {
//     method: GET,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         unstable("org.matrix.msc3916") => "/_matrix/client/unstable/org.matrix.msc3916/media/config",
//         1.11 | stable("org.matrix.msc3916.stable") => "/_matrix/client/v1/media/config",
//     }
// }

// /// Request type for the `get_media_config` endpoint.
// #[request(error = crate::Error)]
// #[derive(Default)]
// pub struct Request {}

/// Response type for the `get_media_config` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct MediaConfigResBody {
    /// Maximum size of upload in bytes.
    #[serde(rename = "m.upload.size")]
    pub upload_size: usize,
}

impl MediaConfigResBody {
    /// Creates a new `Response` with the given maximum upload size.
    pub fn new(upload_size: usize) -> Self {
        Self { upload_size }
    }
}
