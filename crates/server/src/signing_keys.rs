use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::ops::Deref;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};
use std::{future, iter};

use diesel::prelude::*;
use futures_util::{stream::FuturesUnordered, FutureExt, StreamExt};
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name as HyperName};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch::Receiver, Semaphore};

use crate::core::client::sync_events;
use crate::core::federation::discovery::{OldVerifyKey, ServerSigningKeys, VerifyKey};
use crate::core::identifiers::*;
use crate::core::serde::{Base64, CanonicalJsonObject, JsonValue, RawJsonValue};
use crate::core::signatures::Ed25519KeyPair;
use crate::core::{Seqnum, UnixMillis};
use crate::data::connect;
use crate::data::schema::*;
use crate::{AppResult, MatrixError, ServerConfig};

/// Similar to ServerSigningKeys, but drops a few unnecessary fields we don't require post-validation
#[derive(Deserialize, Debug, Clone)]
pub struct SigningKeys {
    pub verify_keys: BTreeMap<String, VerifyKey>,
    pub old_verify_keys: BTreeMap<String, OldVerifyKey>,
    pub valid_until_ts: UnixMillis,
}

impl SigningKeys {
    /// Creates the SigningKeys struct, using the keys of the current server
    pub fn load_own_keys() -> Self {
        let mut keys = Self {
            verify_keys: BTreeMap::new(),
            old_verify_keys: BTreeMap::new(),
            valid_until_ts: UnixMillis::from_system_time(SystemTime::now() + Duration::from_secs(7 * 86400))
                .expect("Should be valid until year 500,000,000"),
        };

        keys.verify_keys.insert(
            format!("ed25519:{}", crate::keypair().version()),
            VerifyKey {
                key: Base64::new(crate::keypair().public_key().to_vec()),
            },
        );

        keys
    }
}

impl From<ServerSigningKeys> for SigningKeys {
    fn from(value: ServerSigningKeys) -> Self {
        let ServerSigningKeys {
            verify_keys,
            old_verify_keys,
            valid_until_ts,
            ..
        } = value;

        Self {
            verify_keys: verify_keys.into_iter().map(|(id, key)| (id.to_string(), key)).collect(),
            old_verify_keys: old_verify_keys
                .into_iter()
                .map(|(id, key)| (id.to_string(), key))
                .collect(),
            valid_until_ts,
        }
    }
}
