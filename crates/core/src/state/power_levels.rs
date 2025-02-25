use std::collections::BTreeMap;

use crate::events::{TimelineEventType, room::power_levels::RoomPowerLevelsEventContent};
use crate::{
    OwnedUserId,
    power_levels::{NotificationPowerLevels, default_power_level},
    serde::{btreemap_deserialize_v1_powerlevel_values, deserialize_v1_powerlevel},
};
use serde::Deserialize;
use serde_json::{Error, from_str as from_json_str};
use tracing::error;

use crate::RoomVersion;

#[derive(Deserialize, Default)]
struct IntRoomPowerLevelsEventContent {
    #[serde(default = "default_power_level")]
    ban: i64,

    #[serde(default)]
    events: BTreeMap<TimelineEventType, i64>,

    #[serde(default)]
    events_default: i64,

    #[serde(default)]
    invite: i64,

    #[serde(default = "default_power_level")]
    kick: i64,

    #[serde(default = "default_power_level")]
    redact: i64,

    #[serde(default = "default_power_level")]
    state_default: i64,

    #[serde(default)]
    users: BTreeMap<OwnedUserId, i64>,

    #[serde(default)]
    users_default: i64,

    #[serde(default)]
    notifications: IntNotificationPowerLevels,
}

impl From<IntRoomPowerLevelsEventContent> for RoomPowerLevelsEventContent {
    fn from(int_pl: IntRoomPowerLevelsEventContent) -> Self {
        let IntRoomPowerLevelsEventContent {
            ban,
            events,
            events_default,
            invite,
            kick,
            redact,
            state_default,
            users,
            users_default,
            notifications,
        } = int_pl;

        let mut pl = Self::new();
        pl.ban = ban;
        pl.events = events;
        pl.events_default = events_default;
        pl.invite = invite;
        pl.kick = kick;
        pl.redact = redact;
        pl.state_default = state_default;
        pl.users = users;
        pl.users_default = users_default;
        pl.notifications = notifications.into();

        pl
    }
}

#[derive(Deserialize)]
struct IntNotificationPowerLevels {
    #[serde(default = "default_power_level")]
    room: i64,
}

impl Default for IntNotificationPowerLevels {
    fn default() -> Self {
        Self {
            room: default_power_level(),
        }
    }
}

impl From<IntNotificationPowerLevels> for NotificationPowerLevels {
    fn from(int_notif: IntNotificationPowerLevels) -> Self {
        let mut notif = Self::new();
        notif.room = int_notif.room;

        notif
    }
}

pub(crate) fn deserialize_power_levels(
    content: &str,
    room_version: &RoomVersion,
) -> Option<RoomPowerLevelsEventContent> {
    if room_version.integer_power_levels {
        match from_json_str::<IntRoomPowerLevelsEventContent>(content) {
            Ok(content) => Some(content.into()),
            Err(_) => {
                error!("m.room.power_levels event is not valid with integer values");
                None
            }
        }
    } else {
        match from_json_str(content) {
            Ok(content) => Some(content),
            Err(_) => {
                error!("m.room.power_levels event is not valid with integer or string integer values");
                None
            }
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct PowerLevelsContentFields {
    #[serde(default, deserialize_with = "btreemap_deserialize_v1_powerlevel_values")]
    pub(crate) users: BTreeMap<OwnedUserId, i64>,

    #[serde(default, deserialize_with = "deserialize_v1_powerlevel")]
    pub(crate) users_default: i64,
}

#[derive(Deserialize)]
struct IntPowerLevelsContentFields {
    #[serde(default)]
    users: BTreeMap<OwnedUserId, i64>,

    #[serde(default)]
    users_default: i64,
}

impl From<IntPowerLevelsContentFields> for PowerLevelsContentFields {
    fn from(pl: IntPowerLevelsContentFields) -> Self {
        let IntPowerLevelsContentFields { users, users_default } = pl;
        Self { users, users_default }
    }
}

pub(crate) fn deserialize_power_levels_content_fields(
    content: &str,
    room_version: &RoomVersion,
) -> Result<PowerLevelsContentFields, Error> {
    if room_version.integer_power_levels {
        from_json_str::<IntPowerLevelsContentFields>(content).map(|r| r.into())
    } else {
        from_json_str(content)
    }
}

#[derive(Deserialize)]
pub(crate) struct PowerLevelsContentInvite {
    #[serde(default, deserialize_with = "deserialize_v1_powerlevel")]
    pub(crate) invite: i64,
}

#[derive(Deserialize)]
struct IntPowerLevelsContentInvite {
    #[serde(default)]
    invite: i64,
}

impl From<IntPowerLevelsContentInvite> for PowerLevelsContentInvite {
    fn from(pl: IntPowerLevelsContentInvite) -> Self {
        let IntPowerLevelsContentInvite { invite } = pl;
        Self { invite }
    }
}

pub(crate) fn deserialize_power_levels_content_invite(
    content: &str,
    room_version: &RoomVersion,
) -> Result<PowerLevelsContentInvite, Error> {
    if room_version.integer_power_levels {
        from_json_str::<IntPowerLevelsContentInvite>(content).map(|r| r.into())
    } else {
        from_json_str(content)
    }
}

#[derive(Deserialize)]
pub(crate) struct PowerLevelsContentRedact {
    #[serde(default = "default_power_level", deserialize_with = "deserialize_v1_powerlevel")]
    pub(crate) redact: i64,
}

#[derive(Deserialize)]
pub(crate) struct IntPowerLevelsContentRedact {
    #[serde(default = "default_power_level")]
    redact: i64,
}

impl From<IntPowerLevelsContentRedact> for PowerLevelsContentRedact {
    fn from(pl: IntPowerLevelsContentRedact) -> Self {
        let IntPowerLevelsContentRedact { redact } = pl;
        Self { redact }
    }
}

pub(crate) fn deserialize_power_levels_content_redact(
    content: &str,
    room_version: &RoomVersion,
) -> Result<PowerLevelsContentRedact, Error> {
    if room_version.integer_power_levels {
        from_json_str::<IntPowerLevelsContentRedact>(content).map(|r| r.into())
    } else {
        from_json_str(content)
    }
}
