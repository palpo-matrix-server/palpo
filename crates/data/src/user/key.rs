use std::collections::BTreeMap;

use diesel::prelude::*;

use crate::core::client::key::UploadSigningKeysReqBody;
use crate::core::encryption::{CrossSigningKey, DeviceKeys, OneTimeKey};
use crate::core::identifiers::*;
use crate::core::{DeviceKeyAlgorithm, JsonValue, MatrixError, Seqnum, UnixMillis};
use crate::schema::*;
use crate::user::clean_signatures;
use crate::{ DataResult, connect};

#[derive(Identifiable, Insertable, Queryable, Debug, Clone)]
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
    pub origin_key_id: OwnedDeviceKeyId,
    pub target_user_id: OwnedUserId,
    pub target_device_id: OwnedDeviceId,
    pub signature: String,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_cross_signing_sigs)]
pub struct NewDbCrossSignature {
    pub origin_user_id: OwnedUserId,
    pub origin_key_id: OwnedDeviceKeyId,
    pub target_user_id: OwnedUserId,
    pub target_device_id: OwnedDeviceId,
    pub signature: String,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_fallback_keys)]
pub struct DbFallbackKey {
    pub id: String,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: OwnedDeviceKeyId,
    pub key_data: JsonValue,
    pub used_at: Option<i64>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_fallback_keys)]
pub struct NewDbFallbackKey {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: OwnedDeviceKeyId,
    pub key_data: JsonValue,
    pub used_at: Option<i64>,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_one_time_keys)]
pub struct DbOneTimeKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: OwnedDeviceKeyId,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = e2e_one_time_keys)]
pub struct NewDbOneTimeKey {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub key_id: OwnedDeviceKeyId,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = e2e_device_keys)]
pub struct DbDeviceKey {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub algorithm: String,
    pub stream_id: i64,
    pub display_name: Option<String>,
    pub key_data: JsonValue,
    pub created_at: UnixMillis,
}
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = e2e_device_keys)]
pub struct NewDbDeviceKey {
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
    pub occur_sn: i64,
    pub changed_at: UnixMillis,
}
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = e2e_key_changes)]
pub struct NewDbKeyChange {
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub occur_sn: i64,
    pub changed_at: UnixMillis,
}

pub fn get_master_key(
    sender_id: Option<&UserId>,
    user_id: &UserId,
    allowed_signatures: &dyn Fn(&UserId) -> bool,
) -> DataResult<Option<CrossSigningKey>> {
    let key_data = e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("master"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .optional()?;
    if let Some(mut key_data) = key_data {
        clean_signatures(&mut key_data, sender_id, user_id, allowed_signatures)?;
        Ok(serde_json::from_value(key_data).ok())
    } else {
        Ok(None)
    }
}

pub fn get_self_signing_key(
    sender_id: Option<&UserId>,
    user_id: &UserId,
    allowed_signatures: &dyn Fn(&UserId) -> bool,
) -> DataResult<Option<CrossSigningKey>> {
    let key_data = e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("self_signing"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .optional()?;
    if let Some(mut key_data) = key_data {
        clean_signatures(&mut key_data, sender_id, user_id, allowed_signatures)?;
        Ok(serde_json::from_value(key_data).ok())
    } else {
        Ok(None)
    }
}
pub fn get_user_signing_key(user_id: &OwnedUserId) -> DataResult<Option<CrossSigningKey>> {
    e2e_cross_signing_keys::table
        .filter(e2e_cross_signing_keys::user_id.eq(user_id))
        .filter(e2e_cross_signing_keys::key_type.eq("user_signing"))
        .select(e2e_cross_signing_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .map(|data| serde_json::from_value(data).ok())
        .optional()
        .map(|v| v.flatten())
        .map_err(Into::into)
}

pub fn add_one_time_key(
    user_id: &OwnedUserId,
    device_id: &DeviceId,
    key_id: &DeviceKeyId,
    one_time_key: &OneTimeKey,
) -> DataResult<()> {
    diesel::insert_into(e2e_one_time_keys::table)
        .values(&NewDbOneTimeKey {
            user_id: user_id.to_owned(),
            device_id: device_id.to_owned(),
            algorithm: key_id.algorithm().to_string(),
            key_id: key_id.to_owned(),
            key_data: serde_json::to_value(one_time_key).unwrap(),
            created_at: UnixMillis::now(),
        })
        .on_conflict((
            e2e_one_time_keys::user_id,
            e2e_one_time_keys::device_id,
            e2e_one_time_keys::algorithm,
            e2e_one_time_keys::key_id,
        ))
        .do_update()
        .set(e2e_one_time_keys::key_data.eq(serde_json::to_value(one_time_key).unwrap()))
        .execute(&mut connect()?)?;
    Ok(())
}

pub fn claim_one_time_key(
    user_id: &OwnedUserId,
    device_id: &DeviceId,
    key_algorithm: &DeviceKeyAlgorithm,
) -> DataResult<Option<(OwnedDeviceKeyId, OneTimeKey)>> {
    let one_time_key = e2e_one_time_keys::table
        .filter(e2e_one_time_keys::user_id.eq(user_id))
        .filter(e2e_one_time_keys::device_id.eq(device_id))
        .filter(e2e_one_time_keys::algorithm.eq(key_algorithm.as_ref()))
        .order(e2e_one_time_keys::id.desc())
        .first::<DbOneTimeKey>(&mut connect()?)
        .optional()?;
    if let Some(DbOneTimeKey {
        id, key_id, key_data, ..
    }) = one_time_key
    {
        diesel::delete(e2e_one_time_keys::table.find(id)).execute(&mut connect()?)?;
        Ok(Some((key_id, serde_json::from_value::<OneTimeKey>(key_data)?)))
    } else {
        Ok(None)
    }
}

pub fn count_one_time_keys(user_id: &UserId, device_id: &DeviceId) -> DataResult<BTreeMap<DeviceKeyAlgorithm, u64>> {
    let list = e2e_one_time_keys::table
        .filter(e2e_one_time_keys::user_id.eq(user_id))
        .filter(e2e_one_time_keys::device_id.eq(device_id))
        .group_by(e2e_one_time_keys::algorithm)
        .select((e2e_one_time_keys::algorithm, diesel::dsl::count_star()))
        .load::<(String, i64)>(&mut connect()?)?;
    Ok(BTreeMap::from_iter(
        list.into_iter().map(|(k, v)| (DeviceKeyAlgorithm::from(k), v as u64)),
    ))
}

pub fn add_device_keys(user_id: &UserId, device_id: &DeviceId, device_keys: &DeviceKeys) -> DataResult<()> {
    println!(
        ">>>>>>>>>>>>>>>>>>add add_device_keys user_id: {:?} device_id: {device_id} device_keys:{device_keys:?}",
        user_id
    );
    let new_device_key = NewDbDeviceKey {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        stream_id: 0,
        display_name: device_keys.unsigned.device_display_name.clone(),
        key_data: serde_json::to_value(device_keys).unwrap(),
        created_at: UnixMillis::now(),
    };
    diesel::insert_into(e2e_device_keys::table)
        .values(&new_device_key)
        .on_conflict((e2e_device_keys::user_id, e2e_device_keys::device_id))
        .do_update()
        .set(&new_device_key)
        .execute(&mut connect()?)?;
    mark_device_key_update(user_id)?;
    Ok(())
}

pub fn add_cross_signing_keys(
    user_id: &UserId,
    master_key: &CrossSigningKey,
    self_signing_key: &Option<CrossSigningKey>,
    user_signing_key: &Option<CrossSigningKey>,
    notify: bool,
) -> DataResult<()> {
    // TODO: Check signatures
    diesel::insert_into(e2e_cross_signing_keys::table)
        .values(NewDbCrossSigningKey {
            user_id: user_id.to_owned(),
            key_type: "master".to_owned(),
            key_data: serde_json::to_value(master_key)?,
        })
        .execute(&mut connect()?)?;

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
            .execute(&mut connect()?)?;
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
            .execute(&mut connect()?)?;
    }

    if notify {
        mark_device_key_update(user_id)?;
    }

    Ok(())
}

pub fn sign_key(
    target_user_id: &UserId,
    target_device_id: &str,
    signature: (String, String),
    sender_id: &UserId,
) -> DataResult<()> {
    // let cross_signing_key = e2e_cross_signing_keys::table
    //     .filter(e2e_cross_signing_keys::user_id.eq(target_id))
    //     .filter(e2e_cross_signing_keys::key_type.eq("master"))
    //     .order_by(e2e_cross_signing_keys::id.desc())
    //     .first::<DbCrossSigningKey>(&mut connect()?)?;
    // let mut cross_signing_key: CrossSigningKey = serde_json::from_value(cross_signing_key.key_data.clone())?;
    let origin_key_id = DeviceKeyId::parse(&signature.0)?.to_owned();

    // cross_signing_key
    //     .signatures
    //     .entry(sender_id.to_owned())
    //     .or_defaut()
    //     .insert(key_id.clone(), signature.1);

    diesel::insert_into(e2e_cross_signing_sigs::table)
        .values(NewDbCrossSignature {
            origin_user_id: sender_id.to_owned(),
            origin_key_id,
            target_user_id: target_user_id.to_owned(),
            target_device_id: OwnedDeviceId::from(target_device_id),
            signature: signature.1,
        })
        .execute(&mut connect()?)?;
    mark_device_key_update(target_user_id)
}

pub fn mark_device_key_update(user_id: &UserId) -> DataResult<()> {
    println!(">>>>>>>>>>>>>>mark_device_key_update, user_id: {:?}", user_id);
    let changed_at = UnixMillis::now();
    for room_id in crate::user::joined_rooms(user_id, 0)? {
        // comment for testing
        // // Don't send key updates to unencrypted rooms
        // if crate::room::state::get_state(&room_id, &StateEventType::RoomEncryption, "")?.is_none() {
        //     continue;
        // }

        let change = NewDbKeyChange {
            user_id: user_id.to_owned(),
            room_id: Some(room_id.to_owned()),
            changed_at,
            occur_sn: crate::next_sn()?,
        };

        diesel::delete(
            e2e_key_changes::table
                .filter(e2e_key_changes::user_id.eq(user_id))
                .filter(e2e_key_changes::room_id.eq(room_id)),
        )
        .execute(&mut connect()?)?;
        diesel::insert_into(e2e_key_changes::table)
            .values(&change)
            .execute(&mut connect()?)?;
    }

    let change = NewDbKeyChange {
        user_id: user_id.to_owned(),
        room_id: None,
        changed_at,
        occur_sn: crate::next_sn()?,
    };

    diesel::delete(
        e2e_key_changes::table
            .filter(e2e_key_changes::user_id.eq(user_id))
            .filter(e2e_key_changes::room_id.is_null()),
    )
    .execute(&mut connect()?)?;
    diesel::insert_into(e2e_key_changes::table)
        .values(&change)
        .execute(&mut connect()?)?;

    Ok(())
}

pub fn get_device_keys(user_id: &UserId, device_id: &DeviceId) -> DataResult<Option<DeviceKeys>> {
    e2e_device_keys::table
        .filter(e2e_device_keys::user_id.eq(user_id))
        .filter(e2e_device_keys::device_id.eq(device_id))
        .select(e2e_device_keys::key_data)
        .first::<JsonValue>(&mut connect()?)
        .optional()?
        .map(|v| serde_json::from_value(v).map_err(Into::into))
        .transpose()
}

pub fn get_device_keys_and_sigs(user_id: &UserId, device_id: &DeviceId) -> DataResult<Option<DeviceKeys>> {
    let Some(mut device_keys) = get_device_keys(user_id, device_id)? else {
        return Ok(None);
    };
    let signatures = e2e_cross_signing_sigs::table
        .filter(e2e_cross_signing_sigs::origin_user_id.eq(user_id))
        .filter(e2e_cross_signing_sigs::target_user_id.eq(user_id))
        .filter(e2e_cross_signing_sigs::target_device_id.eq(device_id))
        .load::<DbCrossSignature>(&mut connect()?)?;
    for DbCrossSignature {
        origin_key_id,
        signature,
        ..
    } in signatures
    {
        device_keys
            .signatures
            .entry(user_id.to_owned())
            .or_default()
            .insert(origin_key_id, signature);
    }
    Ok(Some(device_keys))
}

pub fn keys_changed_users(user_id: &UserId, since_sn: i64, until_sn: Option<i64>) -> DataResult<Vec<OwnedUserId>> {
    let room_ids = crate::user::joined_rooms(user_id, 0)?;
    if let Some(until_sn) = until_sn {
        e2e_key_changes::table
            .filter(
                e2e_key_changes::room_id
                    .eq_any(&room_ids)
                    .or(e2e_key_changes::room_id.is_null()),
            )
            .filter(e2e_key_changes::occur_sn.ge(since_sn))
            .filter(e2e_key_changes::occur_sn.le(until_sn))
            .select(e2e_key_changes::user_id)
            .load::<OwnedUserId>(&mut connect()?)
            .map_err(Into::into)
    } else {
        e2e_key_changes::table
            .filter(
                e2e_key_changes::room_id
                    .eq_any(&room_ids)
                    .or(e2e_key_changes::room_id.is_null()),
            )
            .filter(e2e_key_changes::occur_sn.ge(since_sn))
            .select(e2e_key_changes::user_id)
            .load::<OwnedUserId>(&mut connect()?)
            .map_err(Into::into)
    }
}

pub fn room_keys_changed(
    room_id: &RoomId,
    since_sn: i64,
    until_sn: Option<i64>,
) -> DataResult<Vec<(OwnedUserId, Seqnum)>> {
    if let Some(until_sn) = until_sn {
        e2e_key_changes::table
            .filter(e2e_key_changes::room_id.eq(room_id))
            .filter(e2e_key_changes::occur_sn.ge(since_sn))
            .filter(e2e_key_changes::occur_sn.le(until_sn))
            .select((e2e_key_changes::user_id, e2e_key_changes::occur_sn))
            .load::<(OwnedUserId, i64)>(&mut connect()?)
            .map_err(Into::into)
    } else {
        e2e_key_changes::table
            .filter(e2e_key_changes::room_id.eq(room_id))
            .filter(e2e_key_changes::occur_sn.ge(since_sn))
            .select((e2e_key_changes::user_id, e2e_key_changes::occur_sn))
            .load::<(OwnedUserId, i64)>(&mut connect()?)
            .map_err(Into::into)
    }
}

// Check if a key provided in `body` differs from the same key stored in the DB. Returns
// true on the first difference. If a key exists in `body` but does not exist in the DB,
// returns True. If `body` has no keys, this always returns False.
// Note by 'key' we mean Matrix key rather than JSON key.

// The purpose of this function is to detect whether or not we need to apply UIA checks.
// We must apply UIA checks if any key in the database is being overwritten. If a key is
// being inserted for the first time, or if the key exactly matches what is in the database,
// then no UIA check needs to be performed.

// Args:
//     user_id: The user who sent the `body`.
//     body: The JSON request body from POST /keys/device_signing/upload
// Returns:
//     true if any key in `body` has a different value in the database.
pub fn has_different_keys(user_id: &UserId, body: &UploadSigningKeysReqBody) -> DataResult<bool> {
    //TODO: NOW
    Ok(true)
}
