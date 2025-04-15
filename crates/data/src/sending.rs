use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use diesel::prelude::*;
use serde::Deserialize;
use serde_json::value::to_raw_value;
use tokio::sync::{mpsc, Mutex, Semaphore};

use crate::connect;
use crate::core::appservice::event::{push_events_request, PushEventsReqBody};
use crate::core::appservice::Registration;
use crate::core::device::DeviceListUpdateContent;
use crate::core::events::push_rules::PushRulesEventContent;
use crate::core::events::receipt::{ReceiptContent, ReceiptData, ReceiptMap, ReceiptType};
use crate::core::events::GlobalAccountDataEventType;
use crate::core::federation::transaction::{send_messages_request, Edu, SendMessageReqBody, SendMessageResBody};
use crate::core::identifiers::*;
use crate::core::presence::{PresenceContent, PresenceUpdate};
pub use crate::core::sending::*;
use crate::core::serde::{CanonicalJsonObject, RawJsonValue};
use crate::core::{device_id, push, Seqnum, UnixMillis};
use crate::schema::*;

#[derive(Identifiable, Queryable, Insertable, Debug, Clone)]
#[diesel(table_name = outgoing_requests)]
pub struct DbOutgoingRequest {
    pub id: i64,
    pub kind: String,
    pub appservice_id: Option<String>,
    pub user_id: Option<OwnedUserId>,
    pub pushkey: Option<String>,
    pub server_id: Option<OwnedServerName>,
    pub pdu_id: Option<OwnedEventId>,
    pub edu_json: Option<Vec<u8>>,
    pub state: String,
    pub data: Option<Vec<u8>>,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = outgoing_requests)]
pub struct NewDbOutgoingRequest {
    pub kind: String,
    pub appservice_id: Option<String>,
    pub user_id: Option<OwnedUserId>,
    pub pushkey: Option<String>,
    pub server_id: Option<OwnedServerName>,
    pub pdu_id: Option<OwnedEventId>,
    pub edu_json: Option<Vec<u8>>,
}
