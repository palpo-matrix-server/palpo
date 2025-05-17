use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
use std::time::Instant;

use diesel::prelude::*;
use hickory_resolver::Resolver as HickoryResolver;
use hickory_resolver::config::*;
use hickory_resolver::name_server::TokioConnectionProvider;
use salvo::oapi::ToSchema;
use serde::Serialize;
use tokio::sync::{Semaphore, broadcast};

use crate::core::UnixMillis;
use crate::core::federation::discovery::{OldVerifyKey, ServerSigningKeys};
use crate::core::identifiers::*;
use crate::core::serde::{Base64, CanonicalJsonObject, JsonValue, RawJsonValue};
use crate::data::connect;
use crate::data::misc::DbServerSigningKeys;
use crate::data::schema::*;
use crate::room::state;
use crate::utils::{MutexMap, MutexMapGuard};
use crate::{AppResult, MatrixError, SigningKeys};

pub const MXC_LENGTH: usize = 32;
pub const DEVICE_ID_LENGTH: usize = 10;
pub const TOKEN_LENGTH: usize = 32;
pub const SESSION_ID_LENGTH: usize = 32;
pub const AUTO_GEN_PASSWORD_LENGTH: usize = 15;
pub const RANDOM_USER_ID_LENGTH: usize = 10;

pub type TlsNameMap = HashMap<String, (Vec<IpAddr>, u16)>;
type RateLimitState = (Instant, u32); // Time if last failed try, number of failed tries

pub type RoomMutexMap = MutexMap<OwnedRoomId, ()>;
pub type RoomMutexGuard = MutexMapGuard<OwnedRoomId, ()>;

pub type LazyRwLock<T> = LazyLock<RwLock<T>>;
pub static TLS_NAME_OVERRIDE: LazyRwLock<TlsNameMap> = LazyLock::new(Default::default);
pub static BAD_EVENT_RATE_LIMITER: LazyRwLock<HashMap<OwnedEventId, RateLimitState>> = LazyLock::new(Default::default);
pub static BAD_SIGNATURE_RATE_LIMITER: LazyRwLock<HashMap<Vec<String>, RateLimitState>> =
    LazyLock::new(Default::default);
pub static BAD_QUERY_RATE_LIMITER: LazyRwLock<HashMap<OwnedServerName, RateLimitState>> =
    LazyLock::new(Default::default);
pub static SERVER_NAME_RATE_LIMITER: LazyRwLock<HashMap<OwnedServerName, Arc<Semaphore>>> =
    LazyLock::new(Default::default);
pub static ROOM_ID_FEDERATION_HANDLE_TIME: LazyRwLock<HashMap<OwnedRoomId, (OwnedEventId, Instant)>> =
    LazyLock::new(Default::default);
pub static STATERES_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(Default::default);
pub static APPSERVICE_IN_ROOM_CACHE: LazyRwLock<HashMap<OwnedRoomId, HashMap<String, bool>>> =
    LazyRwLock::new(Default::default);
pub static ROTATE: LazyLock<RotationHandler> = LazyLock::new(Default::default);
pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Handles "rotation" of long-polling requests. "Rotation" in this context is similar to "rotation" of log files and the like.
///
/// This is utilized to have sync workers return early and release read locks on the database.
pub struct RotationHandler(broadcast::Sender<()>, broadcast::Receiver<()>);

#[derive(Serialize, ToSchema, Clone, Copy, Debug)]
pub struct EmptyObject {}

impl RotationHandler {
    pub fn new() -> Self {
        let (s, r) = broadcast::channel(1);
        Self(s, r)
    }

    pub fn watch(&self) -> impl Future<Output = ()> {
        let mut r = self.0.subscribe();

        async move {
            let _ = r.recv().await;
        }
    }

    pub fn fire(&self) {
        let _ = self.0.send(());
    }
}

impl Default for RotationHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn config() -> &'static crate::config::ServerConfig {
    crate::config::get()
}

pub fn dns_resolver() -> &'static HickoryResolver<TokioConnectionProvider> {
    static DNS_RESOLVER: OnceLock<HickoryResolver<TokioConnectionProvider>> = OnceLock::new();
    DNS_RESOLVER.get_or_init(|| {
        HickoryResolver::builder_with_config(ResolverConfig::default(), TokioConnectionProvider::default()).build()
    })
}

pub fn add_signing_key_from_trusted_server(
    from_server: &ServerName,
    new_keys: ServerSigningKeys,
) -> AppResult<SigningKeys> {
    let key_data = server_signing_keys::table
        .filter(server_signing_keys::server_id.eq(from_server))
        .select(server_signing_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .optional()?;

    let prev_keys: Option<ServerSigningKeys> = key_data.map(|key_data| serde_json::from_value(key_data)).transpose()?;

    if let Some(mut prev_keys) = prev_keys {
        let ServerSigningKeys {
            verify_keys,
            old_verify_keys,
            ..
        } = new_keys;

        prev_keys.verify_keys.extend(verify_keys);
        prev_keys.old_verify_keys.extend(old_verify_keys);
        prev_keys.valid_until_ts = new_keys.valid_until_ts;

        diesel::insert_into(server_signing_keys::table)
            .values(DbServerSigningKeys {
                server_id: from_server.to_owned(),
                key_data: serde_json::to_value(&prev_keys)?,
                updated_at: UnixMillis::now(),
                created_at: UnixMillis::now(),
            })
            .on_conflict(server_signing_keys::server_id)
            .do_update()
            .set((
                server_signing_keys::key_data.eq(serde_json::to_value(&prev_keys)?),
                server_signing_keys::updated_at.eq(UnixMillis::now()),
            ))
            .execute(&mut connect()?)?;
        Ok(prev_keys.into())
    } else {
        diesel::insert_into(server_signing_keys::table)
            .values(DbServerSigningKeys {
                server_id: from_server.to_owned(),
                key_data: serde_json::to_value(&new_keys)?,
                updated_at: UnixMillis::now(),
                created_at: UnixMillis::now(),
            })
            .on_conflict(server_signing_keys::server_id)
            .do_update()
            .set((
                server_signing_keys::key_data.eq(serde_json::to_value(&new_keys)?),
                server_signing_keys::updated_at.eq(UnixMillis::now()),
            ))
            .execute(&mut connect()?)?;
        Ok(new_keys.into())
    }
}
pub fn add_signing_key_from_origin(origin: &ServerName, new_keys: ServerSigningKeys) -> AppResult<SigningKeys> {
    let key_data = server_signing_keys::table
        .filter(server_signing_keys::server_id.eq(origin))
        .select(server_signing_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .optional()?;

    let prev_keys: Option<ServerSigningKeys> = key_data.map(|key_data| serde_json::from_value(key_data)).transpose()?;

    if let Some(mut prev_keys) = prev_keys {
        let ServerSigningKeys {
            verify_keys,
            old_verify_keys,
            ..
        } = new_keys;

        // Moving `verify_keys` no longer present to `old_verify_keys`
        for (key_id, key) in prev_keys.verify_keys {
            if !verify_keys.contains_key(&key_id) {
                prev_keys
                    .old_verify_keys
                    .insert(key_id, OldVerifyKey::new(prev_keys.valid_until_ts, key.key));
            }
        }

        prev_keys.verify_keys = verify_keys;
        prev_keys.old_verify_keys.extend(old_verify_keys);
        prev_keys.valid_until_ts = new_keys.valid_until_ts;

        diesel::insert_into(server_signing_keys::table)
            .values(DbServerSigningKeys {
                server_id: origin.to_owned(),
                key_data: serde_json::to_value(&prev_keys)?,
                updated_at: UnixMillis::now(),
                created_at: UnixMillis::now(),
            })
            .on_conflict(server_signing_keys::server_id)
            .do_update()
            .set((
                server_signing_keys::key_data.eq(serde_json::to_value(&prev_keys)?),
                server_signing_keys::updated_at.eq(UnixMillis::now()),
            ))
            .execute(&mut connect()?)?;
        Ok(prev_keys.into())
    } else {
        diesel::insert_into(server_signing_keys::table)
            .values(DbServerSigningKeys {
                server_id: origin.to_owned(),
                key_data: serde_json::to_value(&new_keys)?,
                updated_at: UnixMillis::now(),
                created_at: UnixMillis::now(),
            })
            .on_conflict(server_signing_keys::server_id)
            .do_update()
            .set((
                server_signing_keys::key_data.eq(serde_json::to_value(&new_keys)?),
                server_signing_keys::updated_at.eq(UnixMillis::now()),
            ))
            .execute(&mut connect()?)?;
        Ok(new_keys.into())
    }
}

// /// This returns an empty `Ok(None)` when there are no keys found for the server.
// pub fn signing_keys_for(origin: &ServerName) -> AppResult<Option<SigningKeys>> {
//     let key_data = server_signing_keys::table
//         .filter(server_signing_keys::server_id.eq(origin))
//         .select(server_signing_keys::key_data)
//         .first::<JsonValue>(&mut connect()?)
//         .optional()?;
//     if let Some(key_data) = key_data {
//         Ok(serde_json::from_value(key_data).map(Option::Some)?)
//     } else {
//         Ok(None)
//     }
// }

/// Filters the key map of multiple servers down to keys that should be accepted given the expiry time,
/// room version, and timestamp of the paramters
pub fn filter_keys_server_map(
    keys: BTreeMap<String, SigningKeys>,
    timestamp: UnixMillis,
    room_version_id: &RoomVersionId,
) -> BTreeMap<String, BTreeMap<String, Base64>> {
    keys.into_iter()
        .filter_map(|(server, keys)| {
            filter_keys_single_server(keys, timestamp, room_version_id).map(|keys| (server, keys))
        })
        .collect()
}

/// Filters the keys of a single server down to keys that should be accepted given the expiry time,
/// room version, and timestamp of the paramters
pub fn filter_keys_single_server(
    keys: SigningKeys,
    timestamp: UnixMillis,
    room_version_id: &RoomVersionId,
) -> Option<BTreeMap<String, Base64>> {
    if keys.valid_until_ts > timestamp
        // valid_until_ts MUST be ignored in room versions 1, 2, 3, and 4.
        // https://spec.matrix.org/v1.10/server-server-api/#get_matrixkeyv2server
        || matches!(room_version_id, RoomVersionId::V1
                    | RoomVersionId::V2
                    | RoomVersionId::V4
                    | RoomVersionId::V3)
    {
        // Given that either the room version allows stale keys, or the valid_until_ts is
        // in the future, all verify_keys are valid
        let mut map: BTreeMap<_, _> = keys.verify_keys.into_iter().map(|(id, key)| (id, key.key)).collect();

        map.extend(keys.old_verify_keys.into_iter().filter_map(|(id, key)| {
            // Even on old room versions, we don't allow old keys if they are expired
            if key.expired_ts > timestamp {
                Some((id, key.key))
            } else {
                None
            }
        }));

        Some(map)
    } else {
        None
    }
}

pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    // On shutdown
    info!(target: "shutdown-sync", "Received shutdown notification, notifying sync helpers...");
    ROTATE.fire();
}

pub fn parse_incoming_pdu(pdu: &RawJsonValue) -> AppResult<(OwnedEventId, CanonicalJsonObject, OwnedRoomId)> {
    let value: CanonicalJsonObject = serde_json::from_str(pdu.get()).map_err(|e| {
        tracing::warn!("Error parsing incoming event {:?}: {:?}", pdu, e);
        MatrixError::bad_json("Invalid PDU in server response")
    })?;
    let room_id: OwnedRoomId = value
        .get("room_id")
        .and_then(|id| RoomId::parse(id.as_str()?).ok())
        .ok_or(MatrixError::invalid_param("Invalid room id in pdu"))?;

    let room_version_id = crate::room::get_version(&room_id)
        .map_err(|_| MatrixError::invalid_param(format!("Server is not in room {room_id}")))?;

    let (event_id, value) = match crate::event::gen_event_id_canonical_json(pdu, &room_version_id) {
        Ok(t) => t,
        Err(_) => {
            // Event could not be converted to canonical json
            return Err(MatrixError::invalid_param("Could not convert event to canonical json.").into());
        }
    };
    Ok((event_id, value, room_id))
}
