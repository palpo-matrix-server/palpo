//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3sync

use std::{collections::BTreeMap, time::Duration};

use salvo::oapi::{ToParameters, ToSchema};
use serde::{Deserialize, Serialize};

use crate::events::{
    presence::PresenceEvent, AnyGlobalAccountDataEvent, AnyRoomAccountDataEvent, AnyStrippedStateEvent,
    AnySyncEphemeralRoomEvent, AnySyncStateEvent, AnySyncTimelineEvent, AnyToDeviceEvent,
};
use crate::{presence::PresenceState, serde::RawJson, DeviceKeyAlgorithm, OwnedEventId, OwnedRoomId};

use super::UnreadNotificationsCount;
use crate::client::filter::FilterDefinition;
use crate::device::DeviceLists;

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/sync",
//         1.1 => "/_matrix/client/v3/sync",
//     }
// };

/// Request type for the `sync` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct SyncEventsReqArgsV3 {
    /// A filter represented either as its full JSON definition or the ID of a saved filter.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterV3>,

    /// A point in time to continue a sync from.
    ///
    /// Should be a token from the `next_batch` field of a previous `/sync` request.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,

    /// Controls whether to include the full state for all rooms the user is a member of.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub full_state: bool,

    /// Controls whether the client is automatically marked as online by polling this API.
    ///
    /// Defaults to `PresenceState::Online`.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub set_presence: PresenceState,

    /// The maximum time to poll in milliseconds before returning this request.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(
        with = "crate::serde::duration::opt_ms",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout: Option<Duration>,
}

/// Response type for the `sync` endpoint.
#[derive(ToSchema, Serialize, Clone, Debug)]
pub struct SyncEventsResBodyV3 {
    /// The batch token to supply in the `since` param of the next `/sync` request.
    pub next_batch: String,

    /// Updates to rooms.
    #[serde(default, skip_serializing_if = "RoomsV3::is_empty")]
    pub rooms: RoomsV3,

    /// Updates to the presence status of other users.
    #[serde(default, skip_serializing_if = "PresenceV3::is_empty")]
    pub presence: PresenceV3,

    /// The global private data created by this user.
    #[serde(default, skip_serializing_if = "GlobalAccountDataV3::is_empty")]
    pub account_data: GlobalAccountDataV3,

    /// Messages sent directly between devices.
    #[serde(default, skip_serializing_if = "ToDeviceV3::is_empty")]
    pub to_device: ToDeviceV3,

    /// Information on E2E device updates.
    ///
    /// Only present on an incremental sync.
    #[serde(default, skip_serializing_if = "DeviceLists::is_empty")]
    pub device_lists: DeviceLists,

    /// For each key algorithm, the number of unclaimed one-time keys
    /// currently held on the server for a device.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub device_one_time_keys_count: BTreeMap<DeviceKeyAlgorithm, u64>,

    /// For each key algorithm, the number of unclaimed one-time keys
    /// currently held on the server for a device.
    ///
    /// The presence of this field indicates that the server supports
    /// fallback keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_unused_fallback_key_types: Option<Vec<DeviceKeyAlgorithm>>,
}
impl SyncEventsResBodyV3 {
    /// Creates a new `Response` with the given batch token.
    pub fn new(next_batch: String) -> Self {
        Self {
            next_batch,
            rooms: Default::default(),
            presence: Default::default(),
            account_data: Default::default(),
            to_device: Default::default(),
            device_lists: Default::default(),
            device_one_time_keys_count: BTreeMap::new(),
            device_unused_fallback_key_types: None,
        }
    }
}

/// A filter represented either as its full JSON definition or the ID of a saved filter.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
#[allow(clippy::large_enum_variant)]
#[serde(untagged)]
pub enum FilterV3 {
    // The filter definition needs to be (de)serialized twice because it is a URL-encoded JSON
    // string. Since only does the latter and this is a very uncommon
    // setup, we implement it through custom serde logic for this specific enum variant rather
    // than adding another palpo_api attribute.
    //
    // On the deserialization side, because this is an enum with #[serde(untagged)], serde
    // will try the variants in order (https://serde.rs/enum-representations.html). That means because
    // FilterDefinition is the first variant, JSON decoding is attempted first which is almost
    // functionally equivalent to looking at whether the first symbol is a '{' as the spec
    // says. (there are probably some corner cases like leading whitespace)
    /// A complete filter definition serialized to JSON.
    #[serde(with = "crate::serde::json_string")]
    FilterDefinition(FilterDefinition),

    /// The ID of a filter saved on the server.
    FilterId(String),
}

impl From<FilterDefinition> for FilterV3 {
    fn from(def: FilterDefinition) -> Self {
        Self::FilterDefinition(def)
    }
}

impl From<String> for FilterV3 {
    fn from(id: String) -> Self {
        Self::FilterId(id)
    }
}

/// Updates to rooms.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct RoomsV3 {
    /// The rooms that the user has left or been banned from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub leave: BTreeMap<OwnedRoomId, LeftRoomV3>,

    /// The rooms that the user has joined.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub join: BTreeMap<OwnedRoomId, JoinedRoomV3>,

    /// The rooms that the user has been invited to.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub invite: BTreeMap<OwnedRoomId, InvitedRoomV3>,

    /// The rooms that the user has knocked on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub knock: BTreeMap<OwnedRoomId, KnockedRoomV3>,
}

impl RoomsV3 {
    /// Creates an empty `Rooms`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there is no update in any room.
    pub fn is_empty(&self) -> bool {
        self.leave.is_empty() && self.join.is_empty() && self.invite.is_empty()
    }
}

/// Historical updates to left rooms.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct LeftRoomV3 {
    /// The timeline of messages and state changes in the room up to the point when the user
    /// left.
    #[serde(default, skip_serializing_if = "TimelineV3::is_empty")]
    pub timeline: TimelineV3,

    /// The state updates for the room up to the start of the timeline.
    #[serde(default, skip_serializing_if = "StateV3::is_empty")]
    pub state: StateV3,

    /// The private data that this user has attached to this room.
    #[serde(default, skip_serializing_if = "RoomAccountDataV3::is_empty")]
    pub account_data: RoomAccountDataV3,
}

impl LeftRoomV3 {
    /// Creates an empty `LeftRoom`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are updates in the room.
    pub fn is_empty(&self) -> bool {
        self.timeline.is_empty() && self.state.is_empty() && self.account_data.is_empty()
    }
}

/// Updates to joined rooms.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct JoinedRoomV3 {
    /// Information about the room which clients may need to correctly render it
    /// to users.
    #[serde(default, skip_serializing_if = "RoomSummaryV3::is_empty")]
    pub summary: RoomSummaryV3,

    /// Counts of [unread notifications] for this room.
    ///
    /// If `unread_thread_notifications` was set to `true` in the [`RoomEventFilter`], these
    /// include only the unread notifications for the main timeline.
    ///
    /// [unread notifications]: https://spec.matrix.org/latest/client-server-api/#receiving-notifications
    /// [`RoomEventFilter`]: crate::filter::RoomEventFilter
    #[serde(default, skip_serializing_if = "UnreadNotificationsCount::is_empty")]
    pub unread_notifications: UnreadNotificationsCount,

    /// Counts of [unread notifications] for threads in this room.
    ///
    /// This is a map from thread root ID to unread notifications in the thread.
    ///
    /// Only set if `unread_thread_notifications` was set to `true` in the [`RoomEventFilter`].
    ///
    /// [unread notifications]: https://spec.matrix.org/latest/client-server-api/#receiving-notifications
    /// [`RoomEventFilter`]: crate::filter::RoomEventFilter
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unread_thread_notifications: BTreeMap<OwnedEventId, UnreadNotificationsCount>,

    /// The timeline of messages and state changes in the room.
    #[serde(default, skip_serializing_if = "TimelineV3::is_empty")]
    pub timeline: TimelineV3,

    /// Updates to the state, between the time indicated by the `since` parameter, and the
    /// start of the `timeline` (or all state up to the start of the `timeline`, if
    /// `since` is not given, or `full_state` is true).
    #[serde(default, skip_serializing_if = "StateV3::is_empty")]
    pub state: StateV3,

    /// The private data that this user has attached to this room.
    #[serde(default, skip_serializing_if = "RoomAccountDataV3::is_empty")]
    pub account_data: RoomAccountDataV3,

    /// The ephemeral events in the room that aren't recorded in the timeline or state of the
    /// room.
    #[serde(default, skip_serializing_if = "EphemeralV3::is_empty")]
    pub ephemeral: EphemeralV3,

    /// The number of unread events since the latest read receipt.
    ///
    /// This uses the unstable prefix in [MSC2654].
    ///
    /// [MSC2654]: https://github.com/matrix-org/matrix-spec-proposals/pull/2654
    #[serde(rename = "org.matrix.msc2654.unread_count", skip_serializing_if = "Option::is_none")]
    pub unread_count: Option<u64>,
}

impl JoinedRoomV3 {
    /// Creates an empty `JoinedRoom`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no updates in the room.
    pub fn is_empty(&self) -> bool {
        let is_empty = self.summary.is_empty()
            && self.unread_notifications.is_empty()
            && self.unread_thread_notifications.is_empty()
            && self.timeline.is_empty()
            && self.state.is_empty()
            && self.account_data.is_empty()
            && self.ephemeral.is_empty();

        #[cfg(not(feature = "unstable-msc2654"))]
        return is_empty;

        #[cfg(feature = "unstable-msc2654")]
        return is_empty && self.unread_count.is_none();
    }
}

/// Updates to knocked rooms.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct KnockedRoomV3 {
    /// The knock state.
    pub knock_state: KnockStateV3,
}

/// A mapping from a key `events` to a list of `StrippedStateEvent`.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct KnockStateV3 {
    /// The list of events.
    pub events: Vec<RawJson<AnyStrippedStateEvent>>,
}

/// Events in the room.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct TimelineV3 {
    /// True if the number of events returned was limited by the `limit` on the filter.
    ///
    /// Default to `false`.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub limited: bool,

    /// A token that can be supplied to to the `from` parameter of the
    /// `/rooms/{room_id}/messages` endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_batch: Option<String>,

    /// A list of events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnySyncTimelineEvent>>,
}

impl TimelineV3 {
    /// Creates an empty `Timeline`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no timeline updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// State events in the room.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct StateV3 {
    /// A list of state events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnySyncStateEvent>>,
}

impl StateV3 {
    /// Creates an empty `State`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no state updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Creates a `State` with events
    pub fn with_events(events: Vec<RawJson<AnySyncStateEvent>>) -> Self {
        Self {
            events,
            ..Default::default()
        }
    }
}

impl From<Vec<RawJson<AnySyncStateEvent>>> for StateV3 {
    fn from(events: Vec<RawJson<AnySyncStateEvent>>) -> Self {
        Self::with_events(events)
    }
}

/// The global private data created by this user.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct GlobalAccountDataV3 {
    /// A list of events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnyGlobalAccountDataEvent>>,
}

impl GlobalAccountDataV3 {
    /// Creates an empty `GlobalAccountData`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no global account data updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// The private data that this user has attached to this room.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct RoomAccountDataV3 {
    /// A list of events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnyRoomAccountDataEvent>>,
}

impl RoomAccountDataV3 {
    /// Creates an empty `RoomAccountData`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no room account data updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Ephemeral events not recorded in the timeline or state of the room.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct EphemeralV3 {
    /// A list of events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnySyncEphemeralRoomEvent>>,
}

impl EphemeralV3 {
    /// Creates an empty `Ephemeral`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no ephemeral event updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Information about room for rendering to clients.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct RoomSummaryV3 {
    /// Users which can be used to generate a room name if the room does not have one.
    ///
    /// Required if room name or canonical aliases are not set or empty.
    #[serde(rename = "m.heroes", default, skip_serializing_if = "Vec::is_empty")]
    pub heroes: Vec<String>,

    /// Number of users whose membership status is `join`.
    /// Required if field has changed since last sync; otherwise, it may be
    /// omitted.
    #[serde(rename = "m.joined_member_count", skip_serializing_if = "Option::is_none")]
    pub joined_member_count: Option<u64>,

    /// Number of users whose membership status is `invite`.
    /// Required if field has changed since last sync; otherwise, it may be
    /// omitted.
    #[serde(rename = "m.invited_member_count", skip_serializing_if = "Option::is_none")]
    pub invited_member_count: Option<u64>,
}

impl RoomSummaryV3 {
    /// Creates an empty `RoomSummary`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no room summary updates.
    pub fn is_empty(&self) -> bool {
        self.heroes.is_empty() && self.joined_member_count.is_none() && self.invited_member_count.is_none()
    }
}

/// Updates to the rooms that the user has been invited to.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct InvitedRoomV3 {
    /// The state of a room that the user has been invited to.
    #[serde(default, skip_serializing_if = "InviteStateV3::is_empty")]
    pub invite_state: InviteStateV3,
}

impl InvitedRoomV3 {
    /// Creates an empty `InvitedRoom`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no updates to this room.
    pub fn is_empty(&self) -> bool {
        self.invite_state.is_empty()
    }
}

impl From<InviteStateV3> for InvitedRoomV3 {
    fn from(invite_state: InviteStateV3) -> Self {
        InvitedRoomV3 {
            invite_state,
            ..Default::default()
        }
    }
}

/// The state of a room that the user has been invited to.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct InviteStateV3 {
    /// A list of state events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnyStrippedStateEvent>>,
}

impl InviteStateV3 {
    /// Creates an empty `InviteState`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no state updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl From<Vec<RawJson<AnyStrippedStateEvent>>> for InviteStateV3 {
    fn from(events: Vec<RawJson<AnyStrippedStateEvent>>) -> Self {
        Self {
            events,
            ..Default::default()
        }
    }
}

/// Updates to the presence status of other users.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct PresenceV3 {
    /// A list of events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<PresenceEvent>>,
}

impl PresenceV3 {
    /// Creates an empty `Presence`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no presence updates.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Messages sent directly between devices.
#[derive(ToSchema, Clone, Debug, Default, Deserialize, Serialize)]
pub struct ToDeviceV3 {
    /// A list of to-device events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RawJson<AnyToDeviceEvent>>,
}

impl ToDeviceV3 {
    /// Creates an empty `ToDevice`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns true if there are no to-device events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use assign::assign;
    use serde_json::{from_value as from_json_value, json, to_value as to_json_value};

    use super::Timeline;

    #[test]
    fn timeline_serde() {
        let timeline = assign!(Timeline::new(), { limited: true });
        let timeline_serialized = json!({ "limited": true });
        assert_eq!(to_json_value(timeline).unwrap(), timeline_serialized);

        let timeline_deserialized = from_json_value::<Timeline>(timeline_serialized).unwrap();
        assert!(timeline_deserialized.limited);

        let timeline_default = Timeline::default();
        assert_eq!(to_json_value(timeline_default).unwrap(), json!({}));

        let timeline_default_deserialized = from_json_value::<Timeline>(json!({})).unwrap();
        assert!(!timeline_default_deserialized.limited);
    }
}

#[cfg(all(test))]
mod server_tests {
    use std::time::Duration;

    use crate::{api::IncomingRequest as _, presence::PresenceState};
    use assert_matches2::assert_matches;

    use super::{Filter, Request};

    #[test]
    fn deserialize_all_query_params() {
        let uri = http::Uri::builder()
            .scheme("https")
            .authority("matrix.org")
            .path_and_query(
                "/_matrix/client/r0/sync\
                ?filter=myfilter\
                &since=myts\
                &full_state=false\
                &set_presence=offline\
                &timeout=5000",
            )
            .build()
            .unwrap();

        let req = Request::try_from_http_request(
            http::Request::builder().uri(uri).body(&[] as &[u8]).unwrap(),
            &[] as &[String],
        )
        .unwrap();

        assert_matches!(req.filter, Some(Filter::FilterId(id)));
        assert_eq!(id, "myfilter");
        assert_eq!(req.since.as_deref(), Some("myts"));
        assert!(!req.full_state);
        assert_eq!(req.set_presence, PresenceState::Offline);
        assert_eq!(req.timeout, Some(Duration::from_millis(5000)));
    }

    #[test]
    fn deserialize_no_query_params() {
        let uri = http::Uri::builder()
            .scheme("https")
            .authority("matrix.org")
            .path_and_query("/_matrix/client/r0/sync")
            .build()
            .unwrap();

        let req = Request::try_from_http_request(
            http::Request::builder().uri(uri).body(&[] as &[u8]).unwrap(),
            &[] as &[String],
        )
        .unwrap();

        assert_matches!(req.filter, None);
        assert_eq!(req.since, None);
        assert!(!req.full_state);
        assert_eq!(req.set_presence, PresenceState::Online);
        assert_eq!(req.timeout, None);
    }

    #[test]
    fn deserialize_some_query_params() {
        let uri = http::Uri::builder()
            .scheme("https")
            .authority("matrix.org")
            .path_and_query(
                "/_matrix/client/r0/sync\
                ?filter=EOKFFmdZYF\
                &timeout=0",
            )
            .build()
            .unwrap();

        let req = Request::try_from_http_request(
            http::Request::builder().uri(uri).body(&[] as &[u8]).unwrap(),
            &[] as &[String],
        )
        .unwrap();

        assert_matches!(req.filter, Some(Filter::FilterId(id)));
        assert_eq!(id, "EOKFFmdZYF");
        assert_eq!(req.since, None);
        assert!(!req.full_state);
        assert_eq!(req.set_presence, PresenceState::Online);
        assert_eq!(req.timeout, Some(Duration::from_millis(0)));
    }
}
