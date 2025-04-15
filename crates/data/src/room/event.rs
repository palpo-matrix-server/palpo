mod alias;
pub use alias::*;
pub mod auth_chain;
mod current;
pub mod directory;
pub mod lazy_loading;
pub mod pdu_metadata;
pub mod receipt;
pub mod space;
pub mod state;
pub mod timeline;
pub mod typing;
pub mod user;
pub use current::*;
pub use user::*;
pub mod thread;

use std::collections::HashMap;

use diesel::prelude::*;
use rand::seq::SliceRandom;

use crate::appservice::RegistrationInfo;
use crate::core::directory::RoomTypeFilter;
use crate::core::events::direct::DirectEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::{GuestAccess, RoomGuestAccessEventContent};
use crate::core::events::room::member::MembershipState;
use crate::core::events::{
    AnyStrippedStateEvent, AnySyncStateEvent, GlobalAccountDataEventType, RoomAccountDataEventType, StateEventType,
};
use crate::core::identifiers::*;
use crate::core::room::RoomType;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{Seqnum, UnixMillis};
use crate::schema::*;
use crate::{db, diesel_exists, DataError, DataResult, IsRemoteOrLocal, APPSERVICE_IN_ROOM_CACHE};

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
    pub depth: i64,
    pub topological_ordering: i64,
    pub stream_ordering: i64,
    pub unrecognized_keys: Option<String>,
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
}
#[derive(Insertable, AsChangeset, Deserialize, Debug, Clone)]
#[diesel(table_name = events, primary_key(id))]
pub struct NewDbEvent {
    pub id: OwnedEventId,
    pub sn: i64,
    #[serde(rename = "type")]
    pub ty: String,
    pub room_id: OwnedRoomId,
    pub depth: i64,
    pub topological_ordering: i64,
    pub stream_ordering: i64,
    pub unrecognized_keys: Option<String>,
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
    pub fn from_canonical_json(id: &EventId, sn: Seqnum, value: &CanonicalJsonObject) -> DataResult<Self> {
        Self::from_json_value(id, sn, serde_json::to_value(value)?)
    }
    pub fn from_json_value(id: &EventId, sn: Seqnum, mut value: JsonValue) -> DataResult<Self> {
        let depth = value.get("depth").cloned().unwrap_or(0.into());
        let obj = value.as_object_mut().ok_or(MatrixError::bad_json("Invalid event"))?;
        obj.insert("id".into(), id.as_str().into());
        obj.insert("sn".into(), sn.into());
        obj.insert("topological_ordering".into(), depth);
        obj.insert("stream_ordering".into(), 0.into());
        Ok(serde_json::from_value(value).map_err(|e| MatrixError::bad_json("invalid json for event"))?)
    }
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_txn_ids, primary_key(event_id))]
pub struct DbEventTxnId {
    pub id: i64,
    pub txn_id: OwnedTransactionId,
    pub room_id: OwnedRoomId,
    pub user_id: OwnedUserId,
    pub device_id: Option<OwnedDeviceId>,
    pub event_id: Option<OwnedEventId>,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = event_txn_ids, primary_key(event_id))]
pub struct NewDbEventTxnId {
    pub txn_id: OwnedTransactionId,
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub device_id: Option<OwnedDeviceId>,
    pub event_id: Option<OwnedEventId>,
    pub created_at: UnixMillis,
}
