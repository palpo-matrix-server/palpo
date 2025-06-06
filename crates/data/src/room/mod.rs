use diesel::prelude::*;
use serde::Deserialize;

use crate::core::events::StateEventType;
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonObject;
use crate::core::serde::{JsonValue, default_false};
use crate::core::{MatrixError, Seqnum, UnixMillis};
use crate::schema::*;
use crate::{DataResult, connect};

pub mod receipt;

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = rooms)]
pub struct DbRoom {
    pub id: OwnedRoomId,
    pub sn: Seqnum,
    pub version: String,
    pub is_public: bool,
    pub min_depth: i64,
    pub state_frame_id: Option<i64>,
    pub has_auth_chain_index: bool,
    pub disabled: bool,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = rooms)]
pub struct NewDbRoom {
    pub id: OwnedRoomId,
    pub version: String,
    pub is_public: bool,
    pub min_depth: i64,
    pub has_auth_chain_index: bool,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, AsChangeset, Debug, Clone)]
#[diesel(table_name = stats_room_currents, primary_key(room_id))]
pub struct DbRoomCurrent {
    pub room_id: OwnedRoomId,
    pub state_events: i64,
    pub joined_members: i64,
    pub invited_members: i64,
    pub left_members: i64,
    pub banned_members: i64,
    pub knocked_members: i64,
    pub local_users_in_room: i64,
    pub completed_delta_stream_id: i64,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_relations)]
pub struct DbEventRelation {
    pub id: i64,

    pub room_id: OwnedRoomId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub event_ty: String,
    pub child_id: OwnedEventId,
    pub child_sn: i64,
    pub child_ty: String,
    pub rel_type: Option<String>,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = event_relations)]
pub struct NewDbEventRelation {
    pub room_id: OwnedRoomId,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub event_ty: String,
    pub child_id: OwnedEventId,
    pub child_sn: i64,
    pub child_ty: String,
    pub rel_type: Option<String>,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_aliases, primary_key(alias_id))]
pub struct DbRoomAlias {
    pub alias_id: OwnedRoomAliasId,
    pub room_id: OwnedRoomId,
    pub created_by: OwnedUserId,
    pub created_at: UnixMillis,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_state_fields)]
pub struct DbRoomStateField {
    pub id: i64,
    pub event_ty: StateEventType,
    pub state_key: String,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_state_deltas, primary_key(frame_id))]
pub struct DbRoomStateDelta {
    pub frame_id: i64,
    pub room_id: OwnedRoomId,
    pub parent_id: Option<i64>,
    pub appended: Vec<u8>,
    pub disposed: Vec<u8>,
}

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_receipts)]
pub struct DbReceipt {
    pub id: i64,
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub event_id: OwnedEventId,
    pub occur_sn: Seqnum,
    pub json_data: JsonValue,
    pub receipt_at: UnixMillis,
}

#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = event_receipts)]
pub struct NewDbReceipt {
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub event_id: OwnedEventId,
    pub occur_sn: i64,
    pub json_data: JsonValue,
    pub receipt_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_users)]
pub struct DbRoomUser {
    pub id: i64,
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub room_server_id: OwnedServerName,
    pub user_id: OwnedUserId,
    pub user_server_id: OwnedServerName,
    pub sender_id: OwnedUserId,
    pub membership: String,
    pub forgotten: bool,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub state_data: Option<JsonValue>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = room_users)]
pub struct NewDbRoomUser {
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub room_server_id: OwnedServerName,
    pub user_id: OwnedUserId,
    pub user_server_id: OwnedServerName,
    pub sender_id: OwnedUserId,
    pub membership: String,
    pub forgotten: bool,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub state_data: Option<JsonValue>,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = threads, primary_key(event_id))]
pub struct DbThread {
    pub event_id: OwnedEventId,
    pub event_sn: Seqnum,
    pub room_id: OwnedRoomId,
    pub last_id: OwnedEventId,
    pub last_sn: i64,
}

#[derive(Insertable, Identifiable, AsChangeset, Queryable, Debug, Clone)]
#[diesel(table_name = event_datas, primary_key(event_id))]
pub struct DbEventData {
    pub event_id: OwnedEventId,
    pub event_sn: Seqnum,
    pub room_id: OwnedRoomId,
    pub internal_metadata: Option<JsonValue>,
    pub json_data: JsonValue,
    pub format_version: Option<i64>,
}

impl DbEventData {
    pub fn save(&self) -> DataResult<()> {
        diesel::insert_into(event_datas::table)
            .values(self)
            .on_conflict(event_datas::event_id)
            .do_update()
            .set(self)
            .execute(&mut connect()?)?;
        Ok(())
    }
}

#[derive(Identifiable, Insertable, Queryable, Debug, Clone)]
#[diesel(table_name = events, primary_key(id))]
pub struct DbEvent {
    pub id: OwnedEventId,
    pub sn: Seqnum,
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub depth: i64,
    pub topological_ordering: i64,
    pub stream_ordering: i64,
    pub unrecognized_keys: Option<String>,
    pub origin_server_ts: UnixMillis,
    pub received_at: Option<i64>,
    pub sender_id: Option<OwnedUserId>,
    pub contains_url: bool,
    pub worker_id: Option<String>,
    pub state_key: Option<String>,
    pub is_outlier: bool,
    pub is_redacted: bool,
    pub soft_failed: bool,
    pub rejection_reason: Option<String>,
}
#[derive(Insertable, AsChangeset, Deserialize, Debug, Clone)]
#[diesel(table_name = events, primary_key(id))]
pub struct NewDbEvent {
    pub id: OwnedEventId,
    pub sn: Seqnum,
    #[serde(rename = "type")]
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub depth: i64,
    pub topological_ordering: i64,
    pub stream_ordering: i64,
    pub unrecognized_keys: Option<String>,
    pub origin_server_ts: UnixMillis,
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
    pub fn from_canonical_json(id: &EventId, sn: Seqnum, value: &CanonicalJsonObject) -> DataResult<Self> {
        Self::from_json_value(id, sn, serde_json::to_value(value)?)
    }
    pub fn from_json_value(id: &EventId, sn: Seqnum, mut value: JsonValue) -> DataResult<Self> {
        let depth = value.get("depth").cloned().unwrap_or(0.into());
        let ty = value.get("type").cloned().unwrap_or_else(|| "m.room.message".into());
        let obj = value.as_object_mut().ok_or(MatrixError::bad_json("Invalid event"))?;
        obj.insert("id".into(), id.as_str().into());
        obj.insert("sn".into(), sn.into());
        obj.insert("type".into(), ty);
        obj.insert("topological_ordering".into(), depth);
        obj.insert("stream_ordering".into(), 0.into());
        Ok(serde_json::from_value(value).map_err(|_e| MatrixError::bad_json("invalid json for event"))?)
    }

    pub fn save(&self) -> DataResult<()> {
        diesel::insert_into(events::table)
            .values(self)
            .on_conflict(events::id)
            .do_update()
            .set(self)
            .execute(&mut connect()?)?;
        Ok(())
    }
}

#[derive(Insertable, Queryable, Debug, Clone)]
#[diesel(table_name = event_idempotents)]
pub struct NewDbEventIdempotent {
    pub txn_id: OwnedTransactionId,
    pub user_id: OwnedUserId,
    pub device_id: Option<OwnedDeviceId>,
    pub room_id: Option<OwnedRoomId>,
    pub event_id: Option<OwnedEventId>,
    pub created_at: UnixMillis,
}

pub fn is_disabled(room_id: &RoomId) -> DataResult<bool> {
    let query = rooms::table.filter(rooms::disabled.eq(true));
    Ok(diesel_exists!(query, &mut connect()?)?)
}

pub fn add_joined_server(room_id: &RoomId, server_name: &ServerName) -> DataResult<()> {
    diesel::insert_into(room_joined_servers::table)
        .values((
            room_joined_servers::room_id.eq(room_id),
            room_joined_servers::server_id.eq(server_name),
            room_joined_servers::occur_sn.eq(crate::next_sn()?),
        ))
        .on_conflict_do_nothing()
        .execute(&mut connect()?)?;
    Ok(())
}
