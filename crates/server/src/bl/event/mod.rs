pub mod handler;
mod pdu;
pub use pdu::*;
pub mod search;

use diesel::prelude::*;
use palpo_core::serde::CanonicalJsonObject;
use serde::Deserialize;

use crate::core::identifiers::*;
use crate::core::serde::default_false;
use crate::core::{JsonValue, RawJsonValue, UnixMillis};
use crate::schema::*;
use crate::{AppError, AppResult, DieselResult, MatrixError, Seqnum, db};

#[derive(Insertable, Identifiable, AsChangeset, Queryable, Debug, Clone)]
#[diesel(table_name = event_datas, primary_key(event_id))]
pub struct DbEventData {
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub internal_metadata: Option<JsonValue>,
    pub json_data: JsonValue,
    pub format_version: Option<i64>,
}

#[derive(Identifiable, Insertable, Queryable, Debug, Clone)]
#[diesel(table_name = events, primary_key(id))]
pub struct DbEvent {
    pub id: OwnedEventId,
    pub sn: i64,
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub unrecognized_keys: Option<String>,
    pub depth: i64,
    pub origin_server_ts: Option<UnixMillis>,
    pub received_at: Option<i64>,
    pub sender_id: Option<OwnedUserId>,
    pub contains_url: bool,
    pub worker_id: Option<String>,
    pub state_key: Option<String>,
    pub is_outlier: bool,
    pub is_redacted: bool,
    pub soft_failed: bool,
    pub rejection_reason: Option<String>,
    // pub topological_ordering: i64,
}
#[derive(Insertable, AsChangeset, Deserialize, Debug, Clone)]
#[diesel(table_name = events, primary_key(id))]
pub struct NewDbEvent {
    pub id: OwnedEventId,
    pub sn: i64,
    #[serde(rename = "type")]
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub unrecognized_keys: Option<String>,
    pub depth: i64,
    pub origin_server_ts: Option<UnixMillis>,
    pub received_at: Option<i64>,
    pub sender_id: Option<OwnedUserId>,
    #[serde(default = "default_false")]
    pub contains_url: bool,
    pub worker_id: Option<String>,
    pub state_key: Option<String>,
    #[serde(default = "default_false")]
    pub is_outlier: bool,
    #[serde(default = "default_false")]
    pub soft_failed: bool,
    pub rejection_reason: Option<String>,
}

impl NewDbEvent {
    pub fn from_canonical_json(id: &EventId, sn: Seqnum, value: &CanonicalJsonObject) -> AppResult<Self> {
        Self::from_json_value(id, sn, serde_json::to_value(value)?)
    }
    pub fn from_json_value(id: &EventId, sn: Seqnum, mut value: JsonValue) -> AppResult<Self> {
        let obj = value.as_object_mut().ok_or(MatrixError::bad_json("Invalid event"))?;
        obj.insert("id".into(), id.as_str().into());
        obj.insert("sn".into(), sn.into());
        Ok(serde_json::from_value(value)?)
    }
}

/// Generates a correct eventId for the incoming pdu.
///
/// Returns a tuple of the new `EventId` and the PDU as a `BTreeMap<String, CanonicalJsonValue>`.
pub fn gen_event_id_canonical_json(
    pdu: &RawJsonValue,
    room_version_id: &RoomVersionId,
) -> AppResult<(OwnedEventId, CanonicalJsonObject)> {
    let value: CanonicalJsonObject = serde_json::from_str(pdu.get()).map_err(|e| {
        warn!("Error parsing incoming event {:?}: {:?}", pdu, e);
        AppError::public("Invalid PDU in server response")
    })?;
    let event_id = gen_event_id(&value, room_version_id)?;
    Ok((event_id, value))
}
/// Generates a correct eventId for the incoming pdu.
pub fn gen_event_id(value: &CanonicalJsonObject, room_version_id: &RoomVersionId) -> AppResult<OwnedEventId> {
    let reference_hash = crate::core::signatures::reference_hash(value, room_version_id)?;
    let event_id: OwnedEventId = format!("${reference_hash}").try_into()?;
    Ok(event_id)
}

pub fn get_event_sn(event_id: &EventId) -> AppResult<Seqnum> {
    if let Some(sn) = event_sns::table
        .find(event_id)
        .select(event_sns::sn)
        .first::<Seqnum>(&mut *db::connect()?)
        .optional()?
    {
        Ok(sn)
    } else {
        diesel::insert_into(event_sns::table)
            .values(event_sns::id.eq(event_id))
            .on_conflict_do_nothing()
            .returning(event_sns::sn)
            .get_result::<Seqnum>(&mut *db::connect()?)
            .map_err(Into::into)
    }
}

pub fn get_event_sn_and_ty(event_id: &EventId) -> AppResult<(i64, String)> {
    events::table
        .find(event_id)
        .select((events::sn, events::ty))
        .first::<(i64, String)>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn get_db_event(event_id: &EventId) -> AppResult<Option<DbEvent>> {
    events::table
        .find(event_id)
        .first::<DbEvent>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}
