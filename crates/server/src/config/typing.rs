use serde::Deserialize;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "typing")]
#[derive(Clone, Debug, Deserialize)]
pub struct TypingConfig {
    /// Allow incoming typing updates from federation.
    #[serde(default = "default_true")]
    pub allow_incoming: bool,

    /// Allow outgoing typing updates to federation.
    #[serde(default = "default_true")]
    pub allow_outgoing: bool,

    /// Maximum time federation user can indicate typing.
    ///
    /// default: 30_000
    #[serde(default = "default_federation_timeout")]
    pub federation_timeout: u64,

    /// Minimum time local client can indicate typing. This does not override a
    /// client's request to stop typing. It only enforces a minimum value in
    /// case of no stop request.
    ///
    /// default: 15_000
    #[serde(default = "default_client_timeout_min")]
    pub client_timeout_min: u64,

    /// Maximum time local client can indicate typing.
    ///
    /// default: 45_000
    #[serde(default = "default_client_timeout_max")]
    pub client_timeout_max: u64,
}

impl Default for TypingConfig {
    fn default() -> Self {
        Self {
            allow_incoming: true,
            allow_outgoing: true,
            federation_timeout: default_federation_timeout(),
            client_timeout_min: default_client_timeout_min(),
            client_timeout_max: default_client_timeout_max(),
        }
    }
}

fn default_federation_timeout() -> u64 {
    30_000
}

fn default_client_timeout_min() -> u64 {
    15_000
}

fn default_client_timeout_max() -> u64 {
    45_000
}
