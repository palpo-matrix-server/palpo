pub mod handler;
mod pdu;
pub use pdu::*;
pub mod search;

use diesel::prelude::*;
use palpo_core::serde::CanonicalJsonObject;
use serde::Deserialize;

use crate::core::identifiers::*;
use crate::core::serde::default_false;
use crate::core::{JsonValue, RawJsonValue, Seqnum, UnixMillis};
use crate::schema::*;
use crate::{AppError, AppResult, DieselResult, MatrixError, db};

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

pub fn ensure_event_sn(room_id: &RoomId, event_id: &EventId) -> AppResult<Seqnum> {
    if let Some(sn) = event_points::table
        .find(event_id)
        .select(event_points::event_sn)
        .first::<Seqnum>(&mut *db::connect()?)
        .optional()?
    {
        Ok(sn)
    } else {
        diesel::insert_into(event_points::table)
            .values((event_points::event_id.eq(event_id), event_points::room_id.eq(room_id)))
            .on_conflict_do_nothing()
            .returning(event_points::event_sn)
            .get_result::<Seqnum>(&mut *db::connect()?)
            .map_err(Into::into)
    }
}
/// Returns the `count` of this pdu's id.
pub fn get_event_sn(event_id: &EventId) -> AppResult<Seqnum> {
    event_points::table
        .find(event_id)
        .select(event_points::event_sn)
        .first::<Seqnum>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn get_event_id_by_sn(event_sn: Seqnum) -> AppResult<OwnedEventId> {
    event_points::table
        .filter(event_points::event_sn.eq(event_sn))
        .select(event_points::event_id)
        .first::<OwnedEventId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn get_event_sn_and_ty(event_id: &EventId) -> AppResult<(Seqnum, String)> {
    events::table
        .find(event_id)
        .select((events::sn, events::ty))
        .first::<(Seqnum, String)>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn get_db_event(event_id: &EventId) -> AppResult<Option<DbEvent>> {
    events::table
        .find(event_id)
        .first::<DbEvent>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}

pub fn update_frame_id(event_id: &EventId, frame_id: i64) -> AppResult<()> {
    diesel::update(event_points::table.find(event_id))
        .set(event_points::frame_id.eq(frame_id))
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub fn update_frame_id_by_sn(event_sn: Seqnum, frame_id: i64) -> AppResult<()> {
    diesel::update(event_points::table.filter(event_points::event_sn.eq(event_sn)))
        .set(event_points::frame_id.eq(frame_id))
        .execute(&mut db::connect()?)?;
    Ok(())
}

pub type PdusIterItem = (Seqnum, PduEvent);
#[inline]
pub fn ignored_filter(item: PdusIterItem, user_id: &UserId) -> Option<PdusIterItem> {
    let (_, ref pdu) = item;

    is_ignored_pdu(pdu, user_id).eq(&false).then_some(item)
}

#[inline]
pub fn is_ignored_pdu(pdu: &PduEvent, user_id: &UserId) -> bool {
    // exclude Synapse's dummy events from bloating up response bodies. clients
    // don't need to see this.
    if pdu.event_ty.to_string() == "org.matrix.dummy_event" {
        return true;
    }

    // TODO: fixme
    // let ignored_type = IGNORED_MESSAGE_TYPES.binary_search(&pdu.kind).is_ok();

    // let ignored_server = crate::config()
    //     .forbidden_remote_server_names
    //     .contains(pdu.sender().server_name());

    // if ignored_type && (crate::user::user_is_ignored(&pdu.sender, user_id).await) {
    //     return true;
    // }

    false
}
