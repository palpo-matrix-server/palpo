pub mod handler;
mod pdu;
use palpo_core::Direction;
pub use pdu::*;
pub mod search;

use diesel::prelude::*;
use palpo_core::serde::CanonicalJsonObject;

use crate::core::identifiers::*;
use crate::core::serde::RawJsonValue;
use crate::core::{Seqnum, UnixMillis};
use crate::data::connect;
use crate::data::room::DbEvent;
use crate::data::schema::*;
use crate::{AppError, AppResult, MatrixError};

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
        .first::<Seqnum>(&mut connect()?)
        .optional()?
    {
        Ok(sn)
    } else {
        diesel::insert_into(event_points::table)
            .values((event_points::event_id.eq(event_id), event_points::room_id.eq(room_id)))
            .on_conflict_do_nothing()
            .returning(event_points::event_sn)
            .get_result::<Seqnum>(&mut connect()?)
            .map_err(Into::into)
    }
}
/// Returns the `count` of this pdu's id.
pub fn get_event_sn(event_id: &EventId) -> AppResult<Seqnum> {
    event_points::table
        .find(event_id)
        .select(event_points::event_sn)
        .first::<Seqnum>(&mut connect()?)
        .map_err(Into::into)
}

pub fn get_event_id_by_sn(event_sn: Seqnum) -> AppResult<OwnedEventId> {
    event_points::table
        .filter(event_points::event_sn.eq(event_sn))
        .select(event_points::event_id)
        .first::<OwnedEventId>(&mut connect()?)
        .map_err(Into::into)
}

pub fn get_event_for_timestamp(
    room_id: &RoomId,
    timestamp: UnixMillis,
    dir: Direction,
) -> AppResult<(OwnedEventId, UnixMillis)> {
    match dir {
        Direction::Forward => {
            let (local_event_id, origin_server_ts) = events::table
                .filter(events::room_id.eq(room_id))
                .filter(events::origin_server_ts.is_not_null())
                .filter(events::origin_server_ts.ge(timestamp))
                .order_by((events::origin_server_ts.asc(), events::sn.asc()))
                .select((events::id, events::origin_server_ts))
                .first::<(OwnedEventId, UnixMillis)>(&mut connect()?)?;
            Ok((local_event_id, origin_server_ts))
        }
        Direction::Backward => {
            let (local_event_id, origin_server_ts) = events::table
                .filter(events::room_id.eq(room_id))
                .filter(events::origin_server_ts.is_not_null())
                .filter(events::origin_server_ts.le(timestamp))
                .order_by((events::origin_server_ts.desc(), events::sn.desc()))
                .select((events::id, events::origin_server_ts))
                .first::<(OwnedEventId, UnixMillis)>(&mut connect()?)?;

            println!(
                "LLLLLLLLLLbackward event found: {:#?}",
                events::table
                    .filter(events::room_id.eq(room_id))
                    .filter(events::origin_server_ts.is_not_null())
                    .filter(events::origin_server_ts.le(timestamp))
                    .order_by((events::origin_server_ts.desc(), events::sn.desc()))
                    .load::<DbEvent>(&mut connect()?)?
            );
            println!(
                "LLLLLLLLLLbackward event 22222222: {:#?}",
                events::table
                    .filter(events::room_id.eq(room_id))
                    .order_by((events::origin_server_ts.desc(), events::sn.desc()))
                    .load::<DbEvent>(&mut connect()?)?
            );
            Ok((local_event_id, origin_server_ts))
        }
    }
    // TODO: implement this function to find the event for a given timestamp
    // Check for gaps in the history where events could be hiding in between
    // the timestamp given and the event we were able to find locally
    // let is_event_next_to_backward_gap = false;
    // let is_event_next_to_forward_gap = false;
    // let local_event = None;
}

pub fn get_event_sn_and_ty(event_id: &EventId) -> AppResult<(Seqnum, String)> {
    events::table
        .find(event_id)
        .select((events::sn, events::ty))
        .first::<(Seqnum, String)>(&mut connect()?)
        .map_err(Into::into)
}

pub fn get_db_event(event_id: &EventId) -> AppResult<DbEvent> {
    events::table
        .find(event_id)
        .first::<DbEvent>(&mut connect()?)
        .map_err(Into::into)
}

pub fn get_frame_id(room_id: &RoomId, event_sn: i64) -> AppResult<i64> {
    event_points::table
        .filter(event_points::room_id.eq(room_id))
        .filter(event_points::event_sn.eq(event_sn))
        .select(event_points::frame_id)
        .first::<Option<i64>>(&mut connect()?)?
        .ok_or(MatrixError::not_found("room frame id is not found").into())
}
pub fn get_last_frame_id(room_id: &RoomId, before_sn: i64) -> AppResult<i64> {
    event_points::table
        .filter(event_points::room_id.eq(room_id))
        .filter(event_points::event_sn.le(before_sn))
        .filter(event_points::frame_id.is_not_null())
        .select(event_points::frame_id)
        .order_by(event_points::event_sn.desc())
        .first::<Option<i64>>(&mut connect()?)?
        .ok_or(MatrixError::not_found("room last frame id is not found").into())
}
pub fn update_frame_id(event_id: &EventId, frame_id: i64) -> AppResult<()> {
    diesel::update(event_points::table.find(event_id))
        .set(event_points::frame_id.eq(frame_id))
        .execute(&mut connect()?)?;
    diesel::update(events::table.find(event_id))
        .set(events::stream_ordering.eq(frame_id))
        .execute(&mut connect()?)?;
    Ok(())
}

pub fn update_frame_id_by_sn(event_sn: Seqnum, frame_id: i64) -> AppResult<()> {
    diesel::update(event_points::table.filter(event_points::event_sn.eq(event_sn)))
        .set(event_points::frame_id.eq(frame_id))
        .execute(&mut connect()?)?;
    diesel::update(events::table.filter(events::sn.eq(event_sn)))
        .set(events::stream_ordering.eq(frame_id))
        .execute(&mut connect()?)?;
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
