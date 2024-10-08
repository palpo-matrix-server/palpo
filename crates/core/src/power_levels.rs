//! Common types for the [`m.room.power_levels` event][power_levels].
//!
//! [power_levels]: https://spec.matrix.org/latest/client-server-api/#mroompower_levels
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

/// The power level requirements for specific notification types.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct NotificationPowerLevels {
    /// The level required to trigger an `@room` notification.
    #[serde(
        default = "default_power_level",
        deserialize_with = "crate::serde::deserialize_v1_powerlevel"
    )]
    pub room: i64,
}

impl NotificationPowerLevels {
    /// Create a new `NotificationPowerLevels` with all-default values.
    pub fn new() -> Self {
        Self {
            room: default_power_level(),
        }
    }

    /// Value associated with the given `key`.
    pub fn get(&self, key: &str) -> Option<&i64> {
        match key {
            "room" => Some(&self.room),
            _ => None,
        }
    }

    /// Whether all fields have their default values.
    pub fn is_default(&self) -> bool {
        self.room == default_power_level()
    }
}

impl Default for NotificationPowerLevels {
    fn default() -> Self {
        Self::new()
    }
}

/// Used to default power levels to 50 during deserialization.
pub fn default_power_level() -> i64 {
    50
}
