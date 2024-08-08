use core::panic;
use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use diesel::prelude::*;
use futures_util::stream::FuturesUnordered;
use palpo_core::UnixMillis;
use serde_json::json;

use crate::core::client;
use crate::core::client::device::Device;
use crate::core::client::key::ClaimKeysResBody;
use crate::core::encryption::{CrossSigningKey, DeviceKeys, OneTimeKey};
use crate::core::events::StateEventType;
use crate::core::federation;
use crate::core::identifiers::*;
use crate::core::{serde::RawJson, DeviceKeyAlgorithm, OwnedDeviceId, OwnedUserId, UserId};
use crate::schema::events::sender;
use crate::schema::*;
use crate::user::{clean_signatures, DbUserDevice};
use crate::{db, diesel_exists, utils, AppError, AppResult, JsonValue, MatrixError, BAD_QUERY_RATE_LIMITER};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_cross_signing_keys)]
pub struct DbCrossSigningKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub key_type: String,
    pub key_data: JsonValue,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_cross_signing_keys)]
pub struct NewDbCrossSigningKey {
    pub user_id: OwnedUserId,
    pub key_type: String,
    pub key_data: JsonValue,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_cross_signing_sigs)]
pub struct DbCrossSignature {
    pub id: i64,

    pub origin_user_id: OwnedUserId,
    pub origin_key_id: String,
    pub target_user_id: OwnedUserId,
    pub target_key_id: String,
    pub signature: JsonValue,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_cross_signing_sigs)]
pub struct NewDbCrossSignature {
    pub origin_user_id: OwnedUserId,
    pub origin_key_id: String,
    pub target_user_id: OwnedUserId,
    pub target_key_id: String,
    pub signature: JsonValue,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_fallback_keys)]
pub struct DbCrossFallbackKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: String,
    pub key_data: String,
    pub used_at: Option<i64>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_fallback_keys)]
pub struct NewDbFallbackKey {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: String,
    pub key_data: String,
    pub used_at: Option<i64>,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_one_time_keys)]
pub struct DbOneTimeKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub key_id: OwnedDeviceKeyId,
    pub algorithm: String,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_one_time_keys)]
pub struct NewDbOneTimeKey {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub key_id: OwnedDeviceKeyId,
    pub algorithm: String,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_device_keys)]
pub struct DbUserDeviceKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub stream_id: i64,
    pub display_name: Option<String>,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_device_keys)]
pub struct NewDbUserDeviceKey {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub stream_id: i64,
    pub display_name: Option<String>,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_key_changes)]
pub struct DbKeyChange {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub changed_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_key_changes)]
pub struct NewDbKeyChange {
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub changed_at: UnixMillis,
}

pub async fn get_keys<F: Fn(&UserId) -> bool>(
    sender_id: Option<&UserId>,
    device_keys_input: &BTreeMap<OwnedUserId, Vec<OwnedDeviceId>>,
    allowed_signatures: F,
) -> AppResult<client::key::KeysResBody> {
    let mut master_keys = BTreeMap::new();
    let mut self_signing_keys = BTreeMap::new();
    let mut user_signing_keys = BTreeMap::new();
    let mut device_keys = BTreeMap::new();
    let mut get_over_federation = HashMap::new();

    for (user_id, device_ids) in device_keys_input {
        if user_id.server_name() != crate::server_name() {
            get_over_federation
                .entry(user_id.server_name())
                .or_insert_with(Vec::new)
                .push((user_id, device_ids));
            continue;
        }

        if device_ids.is_empty() {
            let mut container = BTreeMap::new();
            for device_id in crate::user::all_device_ids(user_id)? {
                if let Some(mut keys) = crate::user::get_device_keys(user_id, &device_id)? {
                    let device = crate::user::get_device(user_id, &device_id)?;
                    add_unsigned_device_display_name(&mut keys, &device)?;
                    container.insert(device_id, keys);
                }
            }
            device_keys.insert(user_id.to_owned(), container);
        } else {
            for device_id in device_ids {
                let mut container = BTreeMap::new();
                if let Some(mut keys) = crate::user::get_device_keys(user_id, device_id)? {
                    container.insert(device_id.to_owned(), keys);
                }
                device_keys.insert(user_id.to_owned(), container);
            }
        }

        if let Some(master_key) = crate::user::get_master_key(sender_id, user_id, &allowed_signatures)? {
            master_keys.insert(user_id.to_owned(), master_key);
        }
        if let Some(self_signing_key) = crate::user::get_self_signing_key(sender_id, user_id, &allowed_signatures)? {
            self_signing_keys.insert(user_id.to_owned(), self_signing_key);
        }
        if Some(&**user_id) == sender_id {
            if let Some(user_signing_key) = crate::user::get_user_signing_key(user_id)? {
                user_signing_keys.insert(user_id.to_owned(), user_signing_key);
            }
        }
    }

    let failures = BTreeMap::new();

    let back_off = |id| match BAD_QUERY_RATE_LIMITER.write().unwrap().entry(id) {
        hash_map::Entry::Vacant(e) => {
            e.insert((Instant::now(), 1));
        }
        hash_map::Entry::Occupied(mut e) => *e.get_mut() = (Instant::now(), e.get().1 + 1),
    };

    // TODO: fixme
    // let mut futures: FuturesUnordered<_> = get_over_federation
    //     .into_iter()
    //     .map(|(server, vec)| async move {
    //         if let Some((time, tries)) = BAD_QUERY_RATE_LIMITER.read().unwrap().get(&*server) {
    //             // Exponential backoff
    //             let mut min_elapsed_duration = Duration::from_secs(30) * (*tries) * (*tries);
    //             if min_elapsed_duration > Duration::from_secs(60 * 60 * 24) {
    //                 min_elapsed_duration = Duration::from_secs(60 * 60 * 24);
    //             }

    //             if time.elapsed() < min_elapsed_duration {
    //                 debug!("Backing off query from {:?}", server);
    //                 return (server, Err(AppError::public("bad query, still backing off")));
    //             }
    //         }

    //         let mut device_keys_input_fed = BTreeMap::new();
    //         for (user_id, keys) in vec {
    //             device_keys_input_fed.insert(user_id.to_owned(), keys.clone());
    //         }

    //         let mut request = reqwest::Request::new(Method::POST, reqwest::Url::parse(&server.to_string()).unwrap());
    //         let body = serde_json::to_vec(&federation::key::KeysReqBody {
    //             device_keys: device_keys_input_fed,
    //         })
    //         .unwrap();
    //         (*request.body_mut()) = Some(body.into());
    //         (
    //             server,
    //             tokio::time::timeout(
    //                 Duration::from_secs(25),
    //                 crate::sending::send_federation_request(request),
    //             )
    //             .await
    //             .map_err(|e| AppError::public("Query took too long")),
    //         )
    //     })
    //     .collect();

    // while let Some((server, response)) = futures.next().await {
    //     match response {
    //         Ok(Ok(response)) => {
    //             for (user, masterkey) in response.master_keys {
    //                 let (master_key_id, mut master_key) = crate::user::parse_master_key(&user, &masterkey)?;

    //                 if let Some(our_master_key) = crate::user::get_key(&master_key_id, user_id, &user, &allowed_signatures)? {
    //                     let (_, our_master_key) = crate::user::parse_master_key(&user, &our_master_key)?;
    //                     master_key.signatures.extend(our_master_key.signatures);
    //                 }
    //                 let json = serde_json::to_value(master_key).expect("to_value always works");
    //                 let raw = serde_json::from_value(json).expect("RawJson::from_value always works");
    //                 crate::user::add_cross_signing_keys(
    //                     &user, &raw, &None, &None,
    //                     false, // Dont notify. A notification would trigger another key request resulting in an endless loop
    //                 )?;
    //                 master_keys.insert(user, raw);
    //             }

    //             self_signing_keys.extend(response.self_signing_keys);
    //             device_keys.extend(response.device_keys);
    //         }
    //         _ => {
    //             back_off(server.to_owned());
    //             failures.insert(server.to_string(), json!({}));
    //         }
    //     }
    // }

    Ok(client::key::KeysResBody {
        master_keys,
        self_signing_keys,
        user_signing_keys,
        device_keys,
        failures,
    })
}
fn add_unsigned_device_display_name(keys: &mut RawJson<DeviceKeys>, device: &DbUserDevice) -> serde_json::Result<()> {
    if let Some(display_name) = &device.display_name {
        let mut object = keys.deserialize_as::<serde_json::Map<String, serde_json::Value>>()?;

        let unsigned = object.entry("unsigned").or_insert_with(|| json!({}));
        if let serde_json::Value::Object(unsigned_object) = unsigned {
            unsigned_object.insert("device_display_name".to_owned(), display_name.to_owned().into());
        }

        *keys = RawJson::from_raw_value(serde_json::value::to_raw_value(&object)?);
    }

    Ok(())
}

pub async fn claim_keys(
    one_time_keys_input: &BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, DeviceKeyAlgorithm>>,
) -> AppResult<ClaimKeysResBody> {
    let mut one_time_keys = BTreeMap::new();

    let mut get_over_federation = BTreeMap::new();

    for (user_id, map) in one_time_keys_input {
        if user_id.server_name() != crate::server_name() {
            get_over_federation
                .entry(user_id.server_name())
                .or_insert_with(Vec::new)
                .push((user_id, map));
        }

        let mut container = BTreeMap::new();
        for (device_id, key_algorithm) in map {
            if let Some(one_time_keys) = crate::user::take_one_time_key(user_id, device_id, key_algorithm)? {
                let mut c = BTreeMap::new();
                c.insert(one_time_keys.0, one_time_keys.1);
                container.insert(device_id.clone(), c);
            }
        }
        one_time_keys.insert(user_id.clone(), container);
    }

    let mut failures = BTreeMap::new();

    // let mut futures: FuturesUnordered<_> = get_over_federation
    //     .into_iter()
    //     .map(|(server, vec)| async move {
    //         let mut one_time_keys_input_fed = BTreeMap::new();
    //         for (user_id, keys) in vec {
    //             one_time_keys_input_fed.insert(user_id.clone(), keys.clone());
    //         }
    //         (
    //             server,
    //             crate::sending
    //                 .send_federation_request(
    //                     server,
    //                     federation::key::ClaimKeysReqBody {
    //                         one_time_keys: one_time_keys_input_fed,
    //                     },
    //                 )
    //                 .await,
    //         )
    //     })
    //     .collect();

    // while let Some((server, response)) = futures.next().await {
    //     match response {
    //         Ok(keys) => {
    //             one_time_keys.extend(keys.one_time_keys);
    //         }
    //         Err(_e) => {
    //             failures.insert(server.to_string(), json!({}));
    //         }
    //     }
    // }

    Ok(ClaimKeysResBody {
        failures,
        one_time_keys,
    })
}

pub fn get_master_key(
    sender_id: Option<&UserId>,
    user_id: &UserId,
    allowed_signatures: &dyn Fn(&UserId) -> bool,
) -> AppResult<Option<RawJson<CrossSigningKey>>> {
    let key_data = e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("master"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut *db::connect()?)
        .optional()?;
    if let Some(mut key_data) = key_data {
        clean_signatures(&mut key_data, sender_id, user_id, allowed_signatures)?;
        Ok(RawJson::from_value(key_data).ok())
    } else {
        Ok(None)
    }
}

pub fn get_self_signing_key(
    sender_id: Option<&UserId>,
    user_id: &UserId,
    allowed_signatures: &dyn Fn(&UserId) -> bool,
) -> AppResult<Option<RawJson<CrossSigningKey>>> {
    let key_data = e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("self_signing"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut *db::connect()?)
        .optional()?;
    if let Some(mut key_data) = key_data {
        clean_signatures(&mut key_data, sender_id, user_id, allowed_signatures)?;
        Ok(RawJson::from_value(key_data).ok())
    } else {
        Ok(None)
    }
}
pub fn get_user_signing_key(user_id: &OwnedUserId) -> AppResult<Option<RawJson<CrossSigningKey>>> {
    e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("user_signing"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut *db::connect()?)
        .map(|data| RawJson::from_value(data).ok())
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

pub fn add_one_time_key(
    user_id: &OwnedUserId,
    device_id: &DeviceId,
    key_id: &DeviceKeyId,
    one_time_key: &OneTimeKey,
) -> AppResult<()> {
    diesel::insert_into(e2e_one_time_keys::table)
        .values(&NewDbOneTimeKey {
            user_id: user_id.to_owned(),
            device_id: device_id.to_owned(),
            key_id: key_id.to_owned(),
            algorithm: key_id.algorithm().to_string(),
            key_data: serde_json::to_value(one_time_key).unwrap(),
            created_at: UnixMillis::now(),
        })
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn take_one_time_key(
    user_id: &OwnedUserId,
    device_id: &DeviceId,
    key_algorithm: &DeviceKeyAlgorithm,
) -> AppResult<Option<(OwnedDeviceKeyId, RawJson<OneTimeKey>)>> {
    let one_time_key = e2e_one_time_keys::table
        .filter(e2e_one_time_keys::user_id.eq(user_id))
        .filter(e2e_one_time_keys::device_id.eq(device_id))
        .filter(e2e_one_time_keys::algorithm.eq(key_algorithm.as_ref()))
        .order(e2e_one_time_keys::id.desc())
        .first::<DbOneTimeKey>(&mut *db::connect()?)
        .optional()?;
    if let Some(one_time_key) = &one_time_key {
        diesel::delete(one_time_key).execute(&mut db::connect()?)?;
    }
    Ok(one_time_key.map(|k| (k.key_id, RawJson::from_value(k.key_data).unwrap())))
}

pub fn count_one_time_keys(
    user_id: &OwnedUserId,
    device_id: &OwnedDeviceId,
) -> AppResult<BTreeMap<DeviceKeyAlgorithm, u64>> {
    let list = e2e_one_time_keys::table
        .filter(e2e_one_time_keys::user_id.eq(user_id))
        .filter(e2e_one_time_keys::device_id.eq(device_id))
        .group_by(e2e_one_time_keys::algorithm)
        .select((e2e_one_time_keys::algorithm, diesel::dsl::count_star()))
        .load::<(String, i64)>(&mut *db::connect()?)?;
    Ok(BTreeMap::from_iter(
        list.into_iter().map(|(k, v)| (DeviceKeyAlgorithm::from(k), v as u64)),
    ))
}

pub fn add_device_keys(user_id: &OwnedUserId, device_id: &OwnedDeviceId, device_keys: &DeviceKeys) -> AppResult<()> {
    diesel::insert_into(e2e_device_keys::table)
        .values(&NewDbUserDeviceKey {
            user_id: user_id.to_owned(),
            device_id: device_id.to_owned(),
            stream_id: 0,
            display_name: device_keys.unsigned.device_display_name.clone(),
            key_data: serde_json::to_value(device_keys).unwrap(),
            created_at: UnixMillis::now(),
        })
        .execute(&mut db::connect()?)?;
    mark_device_key_update(user_id)?;
    Ok(())
}

pub fn add_cross_signing_keys(
    user_id: &UserId,
    master_key: &CrossSigningKey,
    self_signing_key: &Option<CrossSigningKey>,
    user_signing_key: &Option<CrossSigningKey>,
    notify: bool,
) -> AppResult<()> {
    // TODO: Check signatures
    diesel::insert_into(e2e_cross_signing_keys::table)
        .values(NewDbCrossSigningKey {
            user_id: user_id.to_owned(),
            key_type: "master".to_owned(),
            key_data: serde_json::to_value(master_key)?,
        })
        .execute(&mut db::connect()?)?;

    // Self-signing key
    if let Some(self_signing_key) = self_signing_key {
        let mut self_signing_key_ids = self_signing_key.keys.values();

        let self_signing_key_id = self_signing_key_ids
            .next()
            .ok_or(MatrixError::invalid_param("Self signing key contained no key."))?;

        if self_signing_key_ids.next().is_some() {
            return Err(MatrixError::invalid_param("Self signing key contained more than one key.").into());
        }

        diesel::insert_into(e2e_cross_signing_keys::table)
            .values(NewDbCrossSigningKey {
                user_id: user_id.to_owned(),
                key_type: "self_signing".to_owned(),
                key_data: serde_json::to_value(self_signing_key)?,
            })
            .execute(&mut db::connect()?)?;
    }

    // User-signing key
    if let Some(user_signing_key) = user_signing_key {
        let mut user_signing_key_ids = user_signing_key.keys.values();

        let user_signing_key_id = user_signing_key_ids
            .next()
            .ok_or(MatrixError::invalid_param("User signing key contained no key."))?;

        if user_signing_key_ids.next().is_some() {
            return Err(MatrixError::invalid_param("User signing key contained more than one key.").into());
        }

        diesel::insert_into(e2e_cross_signing_keys::table)
            .values(NewDbCrossSigningKey {
                user_id: user_id.to_owned(),
                key_type: "user_signing".to_owned(),
                key_data: serde_json::to_value(user_signing_key)?,
            })
            .execute(&mut db::connect()?)?;
    }

    if notify {
        mark_device_key_update(user_id)?;
    }

    Ok(())
}

pub fn sign_key(
    target_id: &UserId,
    target_key_id: &str,
    signature: (String, String),
    sender_id: &UserId,
) -> AppResult<()> {
    let cross_signing_key = e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(target_id))
        .order_by(e2e_cross_signing_keys::id.desc())
        .first::<DbCrossSigningKey>(&mut *db::connect()?)?;
    let mut cross_signing_key: CrossSigningKey = serde_json::from_value(cross_signing_key.key_data.clone())?;
    let key_id = DeviceKeyId::parse(&signature.0)?.to_owned();

    diesel::insert_into(e2e_cross_signing_sigs::table)
        .values(NewDbCrossSignature {
            origin_user_id: sender_id.to_owned(),
            origin_key_id: key_id.to_string(),
            target_user_id: target_id.to_owned(),
            target_key_id: target_key_id.to_string(),
            signature: serde_json::to_value(&signature)?,
        })
        .execute(&mut db::connect()?)?;

    cross_signing_key
        .signatures
        .entry(sender_id.to_owned())
        .or_insert_with(BTreeMap::new)
        .insert(key_id.clone(), signature.1);
    mark_device_key_update(target_id)
}

pub fn mark_device_key_update(user_id: &UserId) -> AppResult<()> {
    for room_id in crate::user::joined_rooms(user_id, 0)? {
        // Don't send key updates to unencrypted rooms
        if crate::room::state::get_state(&room_id, &StateEventType::RoomEncryption, "")?.is_none() {
            continue;
        }

        diesel::insert_into(e2e_key_changes::table)
            .values(&NewDbKeyChange {
                user_id: user_id.to_owned(),
                room_id: Some(room_id.to_owned()),
                changed_at: UnixMillis::now(),
            })
            .on_conflict_do_nothing()
            .execute(&mut db::connect()?)?;
    }

    diesel::insert_into(e2e_key_changes::table)
        .values(&NewDbKeyChange {
            user_id: user_id.to_owned(),
            room_id: None,
            changed_at: UnixMillis::now(),
        })
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn get_device_keys(user_id: &UserId, device_id: &DeviceId) -> AppResult<Option<RawJson<DeviceKeys>>> {
    e2e_device_keys::table
        .filter(e2e_device_keys::user_id.eq(user_id))
        .filter(e2e_device_keys::device_id.eq(device_id))
        .select(e2e_device_keys::key_data)
        .first::<JsonValue>(&mut *db::connect()?)
        .optional()?
        .map(|v| RawJson::from_value(v).map_err(Into::into))
        .transpose()
}

pub fn get_keys_changed_users(user_id: &UserId, from_sn: i64, to_sn: Option<i64>) -> AppResult<bool> {
    if let Some(to_sn) = to_sn {
        diesel_exists!(
            e2e_key_changes::table
                .filter(e2e_key_changes::user_id.eq(user_id))
                .filter(e2e_key_changes::occur_sn.ge(from_sn))
                .filter(e2e_key_changes::occur_sn.le(to_sn))
                .select(e2e_key_changes::user_id),
            &mut db::connect()?
        )
        .map_err(Into::into)
    } else {
        diesel_exists!(
            e2e_key_changes::table
                .filter(e2e_key_changes::user_id.eq(user_id))
                .filter(e2e_key_changes::occur_sn.ge(from_sn))
                .select(e2e_key_changes::user_id),
            &mut db::connect()?
        )
        .map_err(Into::into)
    }
}
