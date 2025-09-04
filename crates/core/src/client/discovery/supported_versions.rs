//! `GET /_matrix/client/*/capabilities`
//!
//! Get information about the server's supported feature set and other relevant
//! capabilities ([spec]).
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#capabilities-negotiation

use std::{
    borrow::Cow,
    collections::{BTreeMap, btree_map},
};

use maplit::btreemap;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, from_value as from_json_value, to_value as to_json_value};

use crate::{MatrixVersion, PrivOwnedStr, RoomVersionId, SupportedVersions, serde::StringEnum};

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessTokenOptional,
//     history: {
//         1.0 => "/_matrix/client/versions",
//     }
// };

/// Response type for the `api_versions` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct VersionsResBody {
    /// A list of Matrix client API protocol versions supported by the
    /// homeserver.
    pub versions: Vec<String>,

    /// Experimental features supported by the server.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unstable_features: BTreeMap<String, bool>,
}

impl VersionsResBody {
    /// Creates a new `Response` with the given `versions`.
    pub fn new(versions: Vec<String>) -> Self {
        Self {
            versions,
            unstable_features: BTreeMap::new(),
        }
    }

    /// Convert this `Response` into a [`SupportedVersions`] that can be used with
    /// `OutgoingRequest::try_into_http_request()`.
    ///
    /// Matrix versions that can't be parsed to a `MatrixVersion`, and features with the boolean
    /// value set to `false` are discarded.
    pub fn as_supported_versions(&self) -> SupportedVersions {
        SupportedVersions::from_parts(&self.versions, &self.unstable_features)
    }
}