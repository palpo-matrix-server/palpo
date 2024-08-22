/// Endpoints for handling keys for end-to-end encryption

/// `POST /_matrix/federation/*/user/keys/claim`
///
/// Claim one-time keys for use in pre-key messages.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#post_matrixfederationv1userkeysclaim
use std::collections::BTreeMap;
use std::time::Duration;

use crate::{
    encryption::OneTimeKey,
    encryption::{CrossSigningKey, DeviceKeys},
    serde::{Base64, RawJson},
    DeviceKeyAlgorithm, OwnedDeviceId, OwnedDeviceKeyId, OwnedUserId,
};
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/user/keys/claim",
//     }
// };

/// Request type for the `claim_keys` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct ClaimKeysReqBody {
    #[serde(
        with = "crate::serde::duration::opt_ms",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout: Option<Duration>,

    /// The keys to be claimed.
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub one_time_keys: OneTimeKeyClaims,
}

/// Response type for the `claim_keys` endpoint.
#[derive(ToSchema, Serialize, Debug)]

pub struct ClaimKeysResBody {
    /// One-time keys for the queried devices
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub one_time_keys: OneTimeKeys,
}
impl ClaimKeysResBody {
    /// Creates a new `Response` with the given one time keys.
    pub fn new(one_time_keys: OneTimeKeys) -> Self {
        Self { one_time_keys }
    }
}

/// A claim for one time keys
pub type OneTimeKeyClaims = BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, DeviceKeyAlgorithm>>;

/// One time keys for use in pre-key messages
pub type OneTimeKeys = BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, BTreeMap<OwnedDeviceKeyId, OneTimeKey>>>;

/// A key and its signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyObject {
    /// The key, encoded using unpadded base64.
    pub key: Base64,

    /// Signature of the key object.
    pub signatures: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceKeyId, String>>,
}

impl KeyObject {
    /// Creates a new `KeyObject` with the given key and signatures.
    pub fn new(key: Base64, signatures: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceKeyId, String>>) -> Self {
        Self { key, signatures }
    }
}

/// `POST /_matrix/federation/*/user/keys/query`
///
/// Get the current devices and identity keys for the given users.
/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#post_matrixfederationv1userkeysquery
// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: ServerSignatures,
//     history: {
//         1.0 => "/_matrix/federation/v1/user/keys/query",
//     }
// };

/// Request type for the `get_keys` endpoint.

#[derive(ToSchema, Deserialize, Serialize, Debug)]
pub struct KeysReqBody {
    /// The keys to be downloaded.
    ///
    /// Gives all keys for a given user if the list of device ids is empty.
    pub device_keys: BTreeMap<OwnedUserId, Vec<OwnedDeviceId>>,
}

/// Response type for the `get_keys` endpoint.
#[derive(ToSchema, Serialize, Default, Debug)]

pub struct KeysResBody {
    /// Keys from the queried devices.
    pub device_keys: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, DeviceKeys>>,

    /// Information on the master cross-signing keys of the queried users.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub master_keys: BTreeMap<OwnedUserId, CrossSigningKey>,

    /// Information on the self-signing keys of the queried users.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub self_signing_keys: BTreeMap<OwnedUserId, CrossSigningKey>,
}
impl KeysResBody {
    /// Creates a new `Response` with the given device keys.
    pub fn new(device_keys: BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, DeviceKeys>>) -> Self {
        Self {
            device_keys,
            ..Default::default()
        }
    }
}
