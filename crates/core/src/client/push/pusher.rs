//! `GET /_matrix/client/*/pushers`
//!
//! Gets all currently active pushers for the authenticated user.

use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::push::{Pusher, PusherIds};

// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3pushers
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushers",
//         1.1 => "/_matrix/client/v3/pushers",
//     }
// };
/// Response type for the `get_pushers` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct PushersResBody {
    /// An array containing the current pushers for the user.
    pub pushers: Vec<Pusher>,
}

impl PushersResBody {
    /// Creates a new `Response` with the given pushers.
    pub fn new(pushers: Vec<Pusher>) -> Self {
        Self { pushers }
    }
}

// `POST /_matrix/client/*/pushers/set`
//
// This endpoint allows the creation, modification and deletion of pushers for this user ID.
// `/v3/` ([spec])
//
// [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3pushersset

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/pushers/set",
//         1.1 => "/_matrix/client/v3/pushers/set",
//     }
// };

/// Request type for the `set_pusher` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SetPusherReqBody(pub PusherAction);

// impl Request {
//     /// Creates a new `Request` for the given action.
//     pub fn new(action: PusherAction) -> Self {
//         Self { action }
//     }

//     /// Creates a new `Request` to create or update the given pusher.
//     pub fn post(pusher: Pusher) -> Self {
//         Self::new(PusherAction::Post(PusherPostData { pusher, append: false }))
//     }

//     /// Creates a new `Request` to delete the pusher identified by the given IDs.
//     pub fn delete(ids: PusherIds) -> Self {
//         Self::new(PusherAction::Delete(ids))
//     }
// }

/// The action to take for the pusher.
#[derive(ToSchema, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum PusherAction {
    /// Create or update the given pusher.
    Post(PusherPostData),

    /// Delete the pusher identified by the given IDs.
    Delete(PusherIds),
}

/// Data necessary to create or update a pusher.
#[derive(ToSchema, Deserialize, Clone, Debug, Serialize)]
pub struct PusherPostData {
    /// The pusher to configure.
    #[serde(flatten)]
    pub pusher: Pusher,

    /// Controls if another pusher with the same pushkey and app id should be created, if there
    /// are already others for other users.
    ///
    /// Defaults to `false`. See the spec for more details.
    #[serde(skip_serializing_if = "crate::serde::is_default")]
    pub append: bool,
}
