//! `POST /_matrix/client/unstable/org.matrix.msc3575/sync` ([MSC])
//!
//! Get all new events in a sliding window of rooms since the last sync or a given point in time.
//!
//! [MSC]: https://github.com/matrix-org/matrix-doc/blob/kegan/sync-v3/proposals/3575-sync.md

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use salvo::prelude::*;
use serde::{Deserialize, Serialize, de::Error as _};

use crate::events::{
    AnyStrippedStateEvent, AnySyncStateEvent, AnySyncTimelineEvent, StateEventType, TimelineEventType,
};
use crate::identifiers::*;
use crate::serde::{RawJson, deserialize_cow_str, duration::opt_ms};
use crate::state::TypeStateKey;
use crate::{OwnedMxcUri, Seqnum, UnixMillis};

pub use super::v4::{
    AccountData, AccountDataConfig, E2ee, E2eeConfig, Extensions, ExtensionsConfig, Receipts, ReceiptsConfig,
    SyncRoomHero, ToDevice, ToDeviceConfig, Typing, TypingConfig,
};
use super::{UnreadNotificationsCount, v4};

pub type SyncInfo<'a> = (&'a UserId, &'a DeviceId, Seqnum, &'a SyncEventsReqBody);

pub type KnownRooms = BTreeMap<String, BTreeMap<OwnedRoomId, Seqnum>>;
pub type TodoRooms = BTreeMap<OwnedRoomId, (BTreeSet<TypeStateKey>, usize, Seqnum)>;

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//        unstable => "/_matrix/client/unstable/org.matrix.simplified_msc3575/sync",
//        // 1.4 => "/_matrix/client/v5/sync",
//     }
// };

#[derive(ToParameters, Deserialize, Debug)]
pub struct SyncEventsReqArgs {
    /// A point in time to continue a sync from.
    ///
    /// Should be a token from the `pos` field of a previous `/sync`
    /// response.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<String>,

    /// The maximum time to poll before responding to this request.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(with = "opt_ms", default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Duration>,
}
/// Request type for the `sync` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SyncEventsReqBody {
    /// A unique string identifier for this connection to the server.
    ///
    /// Optional. If this is missing, only one sliding sync connection can be made to the server at
    /// any one time. Clients need to set this to allow more than one connection concurrently,
    /// so the server can distinguish between connections. This is NOT STICKY and must be
    /// provided with every request, if your client needs more than one concurrent connection.
    ///
    /// Limitation: it must not contain more than 16 chars, due to it being required with every
    /// request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conn_id: Option<String>,

    /// Allows clients to know what request params reached the server,
    /// functionally similar to txn IDs on /send for events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_id: Option<String>,

    /// The list configurations of rooms we are interested in mapped by
    /// name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lists: BTreeMap<String, RequestList>,

    /// Specific rooms and event types that we want to receive events from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub room_subscriptions: BTreeMap<OwnedRoomId, RoomSubscription>,

    /// Specific rooms we no longer want to receive events from.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub unsubscribe_rooms: Vec<OwnedRoomId>,

    /// Extensions API.
    #[serde(default, skip_serializing_if = "ExtensionsConfig::is_empty")]
    pub extensions: ExtensionsConfig,
}

/// Response type for the `sync` endpoint.
#[derive(ToSchema, Serialize, Default, Debug)]
pub struct SyncEventsResBody {
    /// Matches the `txn_id` sent by the request. Please see [`Request::txn_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txn_id: Option<String>,

    /// The token to supply in the `pos` param of the next `/sync` request.
    pub pos: String,

    /// Updates on the order of rooms, mapped by the names we asked for.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lists: BTreeMap<String, SyncList>,

    /// The updates on rooms.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rooms: BTreeMap<OwnedRoomId, SyncRoom>,

    /// Extensions API.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}
impl SyncEventsResBody {
    /// Creates a new `Response` with the given pos.
    pub fn new(pos: String) -> Self {
        Self {
            pos,
            ..Default::default()
        }
    }
}
/// A sliding sync response updates to joiend rooms (see
/// [`super::Response::lists`]).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SyncList {
    /// The total number of rooms found for this list.
    pub count: usize,
}

/// A slising sync response updated room (see [`super::Response::rooms`]).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SyncRoom {
    /// The name as calculated by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<OwnedMxcUri>,

    /// Whether it is an initial response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial: Option<bool>,

    /// Whether it is a direct room.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_dm: Option<bool>,

    /// If this is `Some(_)`, this is a not-yet-accepted invite containing
    /// the given stripped state events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_state: Option<Vec<RawJson<AnyStrippedStateEvent>>>,

    /// Number of unread notifications.
    #[serde(flatten, default, skip_serializing_if = "UnreadNotificationsCount::is_empty")]
    pub unread_notifications: UnreadNotificationsCount,

    /// Message-like events and live state events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<RawJson<AnySyncTimelineEvent>>,

    /// State events as configured by the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_state: Vec<RawJson<AnySyncStateEvent>>,

    /// The `prev_batch` allowing you to paginate through the messages
    /// before the given ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_batch: Option<String>,

    /// True if the number of events returned was limited by the limit on
    /// the filter.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub limited: bool,

    /// The number of users with membership of `join`, including the
    /// client’s own user ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_count: Option<i64>,

    /// The number of users with membership of `invite`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_count: Option<i64>,

    /// The number of timeline events which have just occurred and are not
    /// historical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_live: Option<i64>,

    /// The bump stamp of the room.
    ///
    /// It can be interpreted as a “recency stamp” or “streaming order
    /// index”. For example, consider `roomA` with `bump_stamp = 2`, `roomB`
    /// with `bump_stamp = 1` and `roomC` with `bump_stamp = 0`. If `roomC`
    /// receives an update, its `bump_stamp` will be 3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bump_stamp: Option<i64>,

    /// Heroes of the room, if requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heroes: Option<Vec<SyncRoomHero>>,
}

/// Filter for a sliding sync list, set at request.
///
/// All fields are applied with AND operators, hence if `is_dm`  is `true` and `is_encrypted` is
/// `true` then only encrypted DM rooms will be returned. The absence of fields implies no filter
/// on that criteria: it does NOT imply `false`.
///
/// Filters are considered _sticky_, meaning that the filter only has to be provided once and their
/// parameters 'sticks' for future requests until a new filter overwrites them.
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct RequestListFilters {
    /// Whether to return DMs, non-DM rooms or both.
    ///
    /// Flag which only returns rooms present (or not) in the DM section of account data.
    /// If unset, both DM rooms and non-DM rooms are returned. If false, only non-DM rooms
    /// are returned. If true, only DM rooms are returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_dm: Option<bool>,

    /// Only list rooms that are spaces of these or all.
    ///
    /// A list of spaces which target rooms must be a part of. For every invited/joined
    /// room for this user, ensure that there is a parent space event which is in this list. If
    /// unset, all rooms are included. Servers MUST NOT navigate subspaces. It is up to the
    /// client to give a complete list of spaces to navigate. Only rooms directly in these
    /// spaces will be returned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spaces: Vec<String>,

    /// Whether to return encrypted, non-encrypted rooms or both.
    ///
    /// Flag which only returns rooms which have an `m.room.encryption` state event. If
    /// unset, both encrypted and unencrypted rooms are returned. If false, only unencrypted
    /// rooms are returned. If true, only encrypted rooms are returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_encrypted: Option<bool>,

    /// Whether to return invited Rooms, only joined rooms or both.
    ///
    /// Flag which only returns rooms the user is currently invited to. If unset, both
    /// invited and joined rooms are returned. If false, no invited rooms are returned. If
    /// true, only invited rooms are returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_invite: Option<bool>,

    /// Whether to return Rooms with tombstones, only rooms without tombstones or both.
    ///
    /// Flag which only returns rooms which have an `m.room.tombstone` state event. If unset,
    /// both tombstoned and un-tombstoned rooms are returned. If false, only un-tombstoned rooms
    /// are returned. If true, only tombstoned rooms are returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_tombstoned: Option<bool>,

    /// Only list rooms of given create-types or all.
    ///
    /// If specified, only rooms where the `m.room.create` event has a `type` matching one
    /// of the strings in this array will be returned. If this field is unset, all rooms are
    /// returned regardless of type. This can be used to get the initial set of spaces for an
    /// account.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub room_types: Vec<String>,

    /// Only list rooms that are not of these create-types, or all.
    ///
    /// Same as "room_types" but inverted. This can be used to filter out spaces from the room
    /// list.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub not_room_types: Vec<String>,

    /// Only list rooms matching the given string, or all.
    ///
    /// Filter the room name. Case-insensitive partial matching e.g 'foo' matches 'abFooab'.
    /// The term 'like' is inspired by SQL 'LIKE', and the text here is similar to '%foo%'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_name_like: Option<String>,

    /// Filter the room based on its room tags.
    ///
    /// If multiple tags are present, a room can have
    /// any one of the listed tags (OR'd).
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub tags: Vec<String>,

    /// Filter the room based on its room tags.
    ///
    /// Takes priority over `tags`. For example, a room
    /// with tags A and B with filters `tags:[A]` `not_tags:[B]` would NOT be included because
    /// `not_tags` takes priority over `tags`. This filter is useful if your Rooms list does
    /// NOT include the list of favourite rooms again.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub not_tags: Vec<String>,

    /// Extensions may add further fields to the filters.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    #[salvo(schema(value_type = Object))]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

/// Sliding Sync Request for each list.
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct RequestList {
    /// Put this list into the all-rooms-mode.
    ///
    /// Settings this to true will inform the server that, no matter how slow
    /// that might be, the clients wants all rooms the filters apply to. When operating
    /// in this mode, `ranges` and  `sort` will be ignored  there will be no movement operations
    /// (`DELETE` followed by `INSERT`) as the client has the entire list and can work out whatever
    /// sort order they wish. There will still be `DELETE` and `INSERT` operations when rooms are
    /// left or joined respectively. In addition, there will be an initial `SYNC` operation to let
    /// the client know which rooms in the rooms object were from this list.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub slow_get_all_rooms: bool,

    /// The ranges of rooms we're interested in.
    pub ranges: Vec<(u64, u64)>,

    /// The sort ordering applied to this list of rooms. Sticky.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<String>,

    /// The details to be included per room
    #[serde(flatten)]
    pub room_details: RoomDetailsConfig,

    /// If tombstoned rooms should be returned and if so, with what information attached
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_old_rooms: Option<IncludeOldRooms>,

    /// Filters to apply to the list before sorting. Sticky.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<RequestListFilters>,

    /// An allow-list of event types which should be considered recent activity when sorting
    /// `by_recency`. By omitting event types from this field, clients can ensure that
    /// uninteresting events (e.g. a profil rename) do not cause a room to jump to the top of its
    /// list(s). Empty or omitted `bump_event_types` have no effect; all events in a room will be
    /// considered recent activity.
    ///
    /// NB. Changes to bump_event_types will NOT cause the room list to be reordered;
    /// it will only affect the ordering of rooms due to future updates.
    ///
    /// Sticky.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bump_event_types: Vec<TimelineEventType>,
}

/// Configuration for requesting room details.
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoomDetailsConfig {
    /// Required state for each room returned. An array of event type and state key tuples.
    ///
    /// Note that elements of this array are NOT sticky so they must be specified in full when they
    /// are changed. Sticky.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_state: Vec<(StateEventType, String)>,

    /// The maximum number of timeline events to return per room. Sticky.
    pub timeline_limit: usize,
}

/// Configuration for old rooms to include
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct IncludeOldRooms {
    /// Required state for each room returned. An array of event type and state key tuples.
    ///
    /// Note that elements of this array are NOT sticky so they must be specified in full when they
    /// are changed. Sticky.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_state: Vec<(StateEventType, String)>,

    /// The maximum number of timeline events to return per room. Sticky.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_limit: Option<usize>,
}

/// Sliding sync request room subscription (see [`super::Request::room_subscriptions`]).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoomSubscription {
    /// Required state for each returned room. An array of event type and
    /// state key tuples.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_state: Vec<(StateEventType, String)>,

    /// The maximum number of timeline events to return per room.
    pub timeline_limit: i64,

    /// Include the room heroes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_heroes: Option<bool>,
}

/// Operation applied to the specific SlidingSyncList
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum SlidingOp {
    /// Full reset of the given window.
    Sync,
    /// Insert an item at the given point, moves all following entry by
    /// one to the next Empty or Invalid field.
    Insert,
    /// Drop this entry, moves all following entry up by one.
    Delete,
    /// Mark these as invaldiated.
    Invalidate,
}

/// Updates to joined rooms.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct SyncOp {
    /// The sync operation to apply.
    pub op: SlidingOp,

    /// The range this list update applies to.
    pub range: Option<(u64, u64)>,

    /// Or the specific index the update applies to.
    pub index: Option<u64>,

    /// The list of room_ids updates to apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub room_ids: Vec<OwnedRoomId>,

    /// On insert and delete we are only receiving exactly one room_id.
    pub room_id: Option<OwnedRoomId>,
}

/// Single entry for a room-related read receipt configuration in
/// [`Receipts`].
#[derive(Clone, Debug, PartialEq)]
pub enum ReceiptsRoom {
    /// Get read receipts for all the subscribed rooms.
    AllSubscribed,

    /// Get read receipts for this particular room.
    Room(OwnedRoomId),
}

impl Serialize for ReceiptsRoom {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::AllSubscribed => serializer.serialize_str("*"),
            Self::Room(r) => r.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ReceiptsRoom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        match deserialize_cow_str(deserializer)?.as_ref() {
            "*" => Ok(Self::AllSubscribed),
            other => Ok(Self::Room(RoomId::parse(other).map_err(D::Error::custom)?.to_owned())),
        }
    }
}
