use serde::Deserialize;

use crate::core::serde::{default_false, default_true};
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "typing")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct TypingConfig {
    /// Allow incoming typing updates from federation.
    #[serde(default = "default_true")]
    pub allow_incoming: bool,

    /// Allow outgoing typing updates to federation.
    #[serde(default = "default_true")]
    pub allow_outgoing: bool,

    /// Maximum time federation user can indicate typing (seconds).
    ///
    /// default: 30
    #[serde(default = "default_federation_timeout")]
    pub federation_timeout: u64,

    /// Minimum time local client can indicate typing. This does not override a
    /// client's request to stop typing. It only enforces a minimum value in
    /// case of no stop request (seconds).
    ///
    /// default: 15
    #[serde(default = "default_client_timeout_min")]
    pub client_timeout_min: u64,

    /// Maximum time local client can indicate typing (seconds).
    ///
    /// default: 45
    #[serde(default = "default_client_timeout_max")]
    pub client_timeout_max: u64,
}

fn default_federation_timeout() -> u64 {
    30
}

fn default_client_timeout_min() -> u64 {
    15
}

fn default_client_timeout_max() -> u64 {
    45
}
