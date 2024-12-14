use std::borrow::Borrow;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::StreamExt;
use serde_json::value::RawValue as RawJsonValue;

use super::{extract_key, get_event_keys, PubKeyMap, PubKeys};
use crate::core::canonical_json::{CanonicalJsonObject, CanonicalJsonValue};
use crate::core::identifiers::*;
use crate::core::signatures::Verified;
use crate::core::{
    federation::discovery::{ServerSigningKeys, VerifyKey},
    serde::RawJson,
    signatures::{self, Ed25519KeyPair, PublicKeyMap, PublicKeySet},
    OwnedServerSigningKeyId, RoomVersionId, ServerName, ServerSigningKeyId, UnixMillis,
};
use crate::event::gen_event_id_canonical_json;
use crate::server_key::required_keys_exist;
use crate::{
    utils::{timepoint_from_now, IterStream},
    AppError, AppResult, Server,
};

pub async fn validate_and_add_event_id(
    pdu: &RawJsonValue,
    room_version: &RoomVersionId,
) -> AppResult<(OwnedEventId, CanonicalJsonObject)> {
    let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version)?;
    if let Err(e) = verify_event(&value, Some(room_version)).await {
        return Err(AppError::public(format!("Event {event_id} failed verification: {e:?}")));
    }

    value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

    Ok((event_id, value))
}

pub async fn validate_and_add_event_id_no_fetch(
    pdu: &RawJsonValue,
    room_version: &RoomVersionId,
) -> AppResult<(OwnedEventId, CanonicalJsonObject)> {
    let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version)?;
    if !required_keys_exist(&value, room_version) {
        return Err(AppError::public(format!(
            "Event {event_id} cannot be verified: missing keys."
        )));
    }

    if let Err(e) = verify_event(&value, Some(room_version)).await {
        return Err(AppError::public(format!("Event {event_id} failed verification: {e:?}")));
    }

    value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

    Ok((event_id, value))
}

pub async fn verify_event(event: &CanonicalJsonObject, room_version: Option<&RoomVersionId>) -> AppResult<Verified> {
    let room_version = room_version.unwrap_or(&RoomVersionId::V11);
    let keys = get_event_keys(event, room_version).await?;
    signatures::verify_event(&keys, event, room_version).map_err(Into::into)
}

pub async fn verify_json(event: &CanonicalJsonObject, room_version: Option<&RoomVersionId>) -> AppResult<()> {
    let room_version = room_version.unwrap_or(&RoomVersionId::V11);
    let keys = get_event_keys(event, room_version).await?;
    signatures::verify_json(&keys, event).map_err(Into::into)
}
