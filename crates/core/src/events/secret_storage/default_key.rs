//! Types for the [`m.secret_storage.default_key`] event.
//!
//! [`m.secret_storage.default_key`]: https://spec.matrix.org/latest/client-server-api/#key-storage

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::macros::EventContent;

/// The payload for `DefaultKeyEvent`.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug, EventContent)]
#[palpo_event(type = "m.secret_storage.default_key", kind = GlobalAccountData)]
pub struct SecretStorageDefaultKeyEventContent {
    /// The ID of the default key.
    #[serde(rename = "key")]
    pub key_id: String,
}

impl SecretStorageDefaultKeyEventContent {
    /// Create a new [`SecretStorageDefaultKeyEventContent`] with the given key
    /// ID.
    ///
    /// Uploading this to the account data will mark the secret storage key with
    /// the given key ID as the default key.
    pub fn new(key_id: String) -> Self {
        Self { key_id }
    }
}
