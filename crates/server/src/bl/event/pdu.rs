use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::{json, value::to_raw_value};

use crate::core::events::{
    AnyEphemeralRoomEvent, AnyMessageLikeEvent, AnyStateEvent, AnyStrippedStateEvent, AnySyncStateEvent,
    AnySyncTimelineEvent, AnyTimelineEvent, EventContent, MessageLikeEventType, StateEvent, StateEventType,
    TimelineEventType, room::member::RoomMemberEventContent, space::child::HierarchySpaceChildEvent,
};
use crate::core::identifiers::*;
use crate::core::serde::RawJson;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, RawJsonValue};
use crate::core::{UnixMillis, UserId};
use crate::{AppError, AppResult, JsonValue};

/// Content hashes of a PDU.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventHash {
    /// The SHA-256 hash.
    pub sha256: String,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct PduEvent {
    pub event_id: Arc<EventId>,
    pub event_sn: i64,
    #[serde(rename = "type")]
    pub event_ty: TimelineEventType,
    pub room_id: OwnedRoomId,
    pub sender: OwnedUserId,
    pub origin_server_ts: UnixMillis,
    pub content: Box<RawJsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_key: Option<String>,
    pub prev_events: Vec<Arc<EventId>>,
    pub depth: usize,
    pub auth_events: Vec<Arc<EventId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redacts: Option<Arc<EventId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsigned: Option<Box<RawJsonValue>>,
    pub hashes: EventHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Box<RawJsonValue>>, // BTreeMap<Box<ServerName>, BTreeMap<ServerSigningKeyId, String>>
}

impl PduEvent {
    #[tracing::instrument]
    pub fn redact(&mut self, reason: &PduEvent) -> AppResult<()> {
        self.unsigned = None;

        let allowed: &[&str] = match self.event_ty {
            TimelineEventType::RoomMember => &["join_authorised_via_users_server", "membership"],
            TimelineEventType::RoomCreate => &["creator"],
            TimelineEventType::RoomJoinRules => &["join_rule"],
            TimelineEventType::RoomPowerLevels => &[
                "ban",
                "events",
                "events_default",
                "kick",
                "redact",
                "state_default",
                "users",
                "users_default",
            ],
            TimelineEventType::RoomHistoryVisibility => &["history_visibility"],
            _ => &[],
        };

        let mut old_content: BTreeMap<String, serde_json::Value> = serde_json::from_str(self.content.get())
            .map_err(|_| AppError::internal("PDU in db has invalid content."))?;

        let mut new_content = serde_json::Map::new();

        for key in allowed {
            if let Some(value) = old_content.remove(*key) {
                new_content.insert((*key).to_owned(), value);
            }
        }

        self.unsigned = Some(
            to_raw_value(&json!({
                "redacted_because": serde_json::to_value(reason).expect("to_value(PduEvent) always works")
            }))
            .expect("to string always works"),
        );

        self.content = to_raw_value(&new_content).expect("to string always works");

        Ok(())
    }

    pub fn remove_transaction_id(&mut self) -> AppResult<()> {
        if let Some(unsigned) = &self.unsigned {
            let mut unsigned: BTreeMap<String, Box<RawJsonValue>> = serde_json::from_str(unsigned.get())
                .map_err(|_| AppError::internal("Invalid unsigned in pdu event"))?;
            unsigned.remove("transaction_id");
            self.unsigned = Some(to_raw_value(&unsigned).expect("unsigned is valid"));
        }

        Ok(())
    }

    pub fn add_age(&mut self) -> AppResult<()> {
        let mut unsigned: BTreeMap<String, Box<RawJsonValue>> = self
            .unsigned
            .as_ref()
            .map_or_else(|| Ok(BTreeMap::new()), |u| serde_json::from_str(u.get()))
            .map_err(|_| AppError::internal("Invalid unsigned in pdu event"))?;

        unsigned.insert("age".to_owned(), to_raw_value(&1).unwrap());
        self.unsigned = Some(to_raw_value(&unsigned).expect("unsigned is valid"));

        Ok(())
    }

    #[tracing::instrument]
    pub fn to_sync_room_event(&self) -> RawJson<AnySyncTimelineEvent> {
        let mut json = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
        });

        if let Some(unsigned) = &self.unsigned {
            json["unsigned"] = json!(unsigned);
        }
        if let Some(state_key) = &self.state_key {
            json["state_key"] = json!(state_key);
        }
        if let Some(redacts) = &self.redacts {
            json["redacts"] = json!(redacts);
        }

        serde_json::from_value(json).expect("RawJson::from_value always works")
    }

    /// This only works for events that are also AnyRoomEvents.
    #[tracing::instrument]
    pub fn to_any_event(&self) -> RawJson<AnyEphemeralRoomEvent> {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "room_id": self.room_id,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }
        if let Some(state_key) = &self.state_key {
            data["state_key"] = json!(state_key);
        }
        if let Some(redacts) = &self.redacts {
            data["redacts"] = json!(redacts);
        }

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_room_event(&self) -> RawJson<AnyTimelineEvent> {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "room_id": self.room_id,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }
        if let Some(state_key) = &self.state_key {
            data["state_key"] = json!(state_key);
        }
        if let Some(redacts) = &self.redacts {
            data["redacts"] = json!(redacts);
        }

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_message_like_event(&self) -> RawJson<AnyMessageLikeEvent> {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "room_id": self.room_id,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }
        if let Some(state_key) = &self.state_key {
            data["state_key"] = json!(state_key);
        }
        if let Some(redacts) = &self.redacts {
            data["redacts"] = json!(redacts);
        }

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_state_event(&self) -> RawJson<AnyStateEvent> {
        serde_json::from_value(self.to_state_event_value()).expect("RawJson::from_value always works")
    }
    #[tracing::instrument]
    pub fn to_state_event_value(&self) -> JsonValue {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "room_id": self.room_id,
            "state_key": self.state_key,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }

        data
    }

    #[tracing::instrument]
    pub fn to_sync_state_event(&self) -> RawJson<AnySyncStateEvent> {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "state_key": self.state_key,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_stripped_state_event(&self) -> RawJson<AnyStrippedStateEvent> {
        let data = json!({
            "content": self.content,
            "type": self.event_ty,
            "sender": self.sender,
            "state_key": self.state_key,
        });

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_stripped_spacechild_state_event(&self) -> RawJson<HierarchySpaceChildEvent> {
        let data = json!({
            "content": self.content,
            "type": self.event_ty,
            "sender": self.sender,
            "state_key": self.state_key,
            "origin_server_ts": self.origin_server_ts,
        });

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    #[tracing::instrument]
    pub fn to_member_event(&self) -> RawJson<StateEvent<RoomMemberEventContent>> {
        let mut data = json!({
            "content": self.content,
            "type": self.event_ty,
            "event_id": *self.event_id,
            "sender": self.sender,
            "origin_server_ts": self.origin_server_ts,
            "redacts": self.redacts,
            "room_id": self.room_id,
            "state_key": self.state_key,
        });

        if let Some(unsigned) = &self.unsigned {
            data["unsigned"] = json!(unsigned);
        }

        serde_json::from_value(data).expect("RawJson::from_value always works")
    }

    /// This does not return a full `Pdu` it is only to satisfy palpo's types.
    #[tracing::instrument]
    pub fn convert_to_outgoing_federation_event(mut pdu_json: CanonicalJsonObject) -> Box<RawJsonValue> {
        if let Some(unsigned) = pdu_json.get_mut("unsigned").and_then(|val| val.as_object_mut()) {
            unsigned.remove("transaction_id");
        }

        pdu_json.remove("event_id");

        // TODO: another option would be to convert it to a canonical string to validate size
        // and return a Result<RawJson<...>>
        // serde_json::from_str::<RawJson<_>>(
        //     crate::core::serde::to_canonical_json_string(pdu_json).expect("CanonicalJson is valid serde_json::Value"),
        // )
        // .expect("RawJson::from_value always works")

        to_raw_value(&pdu_json).expect("CanonicalJson is valid serde_json::Value")
    }

    pub fn from_id_val(event_id: &EventId, mut json: CanonicalJsonObject) -> Result<Self, serde_json::Error> {
        json.insert(
            "event_id".to_owned(),
            CanonicalJsonValue::String(event_id.as_str().to_owned()),
        );

        serde_json::from_value(serde_json::to_value(json).expect("valid JSON"))
    }

    pub fn get_content<T>(&self) -> Result<T, serde_json::Error>
    where
        T: for<'de> Deserialize<'de>,
    {
        serde_json::from_str(self.content.get())
    }
}

impl crate::core::state::Event for PduEvent {
    type Id = Arc<EventId>;

    fn event_id(&self) -> &Self::Id {
        &self.event_id
    }

    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn sender(&self) -> &UserId {
        &self.sender
    }

    fn event_type(&self) -> &TimelineEventType {
        &self.event_ty
    }

    fn content(&self) -> &RawJsonValue {
        &self.content
    }

    fn origin_server_ts(&self) -> UnixMillis {
        self.origin_server_ts
    }

    fn state_key(&self) -> Option<&str> {
        self.state_key.as_deref()
    }

    fn prev_events(&self) -> Box<dyn DoubleEndedIterator<Item = &Self::Id> + '_> {
        Box::new(self.prev_events.iter())
    }

    fn auth_events(&self) -> Box<dyn DoubleEndedIterator<Item = &Self::Id> + '_> {
        Box::new(self.auth_events.iter())
    }

    fn redacts(&self) -> Option<&Self::Id> {
        self.redacts.as_ref()
    }
}

// These impl's allow us to dedup state snapshots when resolving state
// for incoming events (federation/send/{txn}).
impl Eq for PduEvent {}
impl PartialEq for PduEvent {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id
    }
}
impl PartialOrd for PduEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.event_id.partial_cmp(&other.event_id)
    }
}
impl Ord for PduEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.event_id.cmp(&other.event_id)
    }
}

/// Build the start of a PDU in order to add it to the Database.
#[derive(Debug, Deserialize)]
pub struct PduBuilder {
    #[serde(rename = "type")]
    pub event_type: TimelineEventType,
    pub content: Box<RawJsonValue>,
    pub unsigned: Option<BTreeMap<String, serde_json::Value>>,
    pub state_key: Option<String>,
    pub redacts: Option<Arc<EventId>>,
    pub timestamp: Option<UnixMillis>,
}

impl PduBuilder {
    pub fn state<T>(state_key: String, content: &T) -> Self
    where
        T: EventContent<EventType = StateEventType>,
    {
        Self {
            event_type: content.event_type().into(),
            content: to_raw_value(content).expect("Builder failed to serialize state event content to RawValue"),
            state_key: Some(state_key),
            ..Self::default()
        }
    }

    pub fn timeline<T>(content: &T) -> Self
    where
        T: EventContent<EventType = MessageLikeEventType>,
    {
        Self {
            event_type: content.event_type().into(),
            content: to_raw_value(content).expect("Builder failed to serialize timeline event content to RawValue"),
            ..Self::default()
        }
    }
}

impl Default for PduBuilder {
    fn default() -> Self {
        Self {
            event_type: "m.room.message".into(),
            content: Box::<RawJsonValue>::default(),
            unsigned: None,
            state_key: None,
            redacts: None,
            timestamp: None,
        }
    }
}
