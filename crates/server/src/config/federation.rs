use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::Deserialize;

use crate::core::serde::{default_false, default_true};
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "federation")]
#[derive(Clone, Debug, Deserialize)]
pub struct FederationConfig {
    /// Controls whether federation is allowed or not. It is not recommended to
    /// disable this after the fact due to potential federation breakage.
    #[serde(default = "default_true")]
    pub enable: bool,

    /// Allows federation requests to be made to itself
    ///
    /// This isn't intended and is very likely a bug if federation requests are
    /// being sent to yourself. This currently mainly exists for development
    /// purposes.
    #[serde(default)]
    pub allow_loopback: bool,

    /// Federation well-known resolution connection timeout.
    ///
    /// default: 6_000
    #[serde(default = "default_well_known_conn_timeout")]
    pub well_known_conn_timeout: u64,

    /// Federation HTTP well-known resolution request timeout.
    ///
    /// default: 10_000
    #[serde(default = "default_well_known_timeout")]
    pub well_known_timeout: u64,

    /// Federation client request timeout. You most definitely want
    /// this to be high to account for extremely large room joins, slow
    /// homeservers, your own resources etc.
    ///
    /// default: 300_000
    #[serde(default = "default_client_timeout")]
    pub client_timeout: u64,

    /// Federation client idle connection pool timeout.
    ///
    /// default: 25
    #[serde(default = "default_client_idle_timeout")]
    pub client_idle_timeout: u64,

    /// Federation client max idle connections per host. Defaults to 1 as
    /// generally the same open connection can be re-used.
    ///
    /// default: 1
    #[serde(default = "default_client_idle_per_host")]
    pub client_idle_per_host: u16,

    /// Federation sender request timeout. The time it takes for the
    /// remote server to process sent transactions can take a while.
    ///
    /// default: 180_000
    #[serde(default = "default_sender_timeout")]
    pub sender_timeout: u64,

    /// Federation sender idle connection pool timeout.
    ///
    /// default: 180_000
    #[serde(default = "default_sender_idle_timeout")]
    pub sender_idle_timeout: u64,

    /// Federation sender transaction retry backoff limit.
    ///
    /// default: 86400_000
    #[serde(default = "default_sender_retry_backoff_limit")]
    pub sender_retry_backoff_limit: u64,

    /// Set this to true to allow federating device display names / allow
    /// external users to see your device display name. If federation is
    /// disabled entirely (`allow_federation`), this is inherently false. For
    /// privacy reasons, this is best left disabled.
    #[serde(default)]
    pub allow_device_name: bool,

    /// Config option to allow or disallow incoming federation requests that
    /// obtain the profiles of our local users from
    /// `/_matrix/federation/v1/query/profile`
    ///
    /// Increases privacy of your local user's such as display names, but some
    /// remote users may get a false "this user does not exist" error when they
    /// try to invite you to a DM or room. Also can protect against profile
    /// spiders.
    ///
    /// This is inherently false if `allow_federation` is disabled
    #[serde(default = "default_true")]
    pub allow_inbound_profile_lookup: bool,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            enable: true,
            allow_loopback: false,
            well_known_conn_timeout: default_well_known_conn_timeout(),
            well_known_timeout: default_well_known_timeout(),
            client_timeout: default_client_timeout(),
            client_idle_timeout: default_client_idle_timeout(),
            client_idle_per_host: default_client_idle_per_host(),
            sender_timeout: default_sender_timeout(),
            sender_idle_timeout: default_sender_idle_timeout(),
            sender_retry_backoff_limit: default_sender_retry_backoff_limit(),
            allow_device_name: false,
            allow_inbound_profile_lookup: true,
        }
    }
}

fn default_well_known_conn_timeout() -> u64 {
    6_000
}

fn default_well_known_timeout() -> u64 {
    10_000
}

fn default_client_timeout() -> u64 {
    25_000
}

fn default_client_idle_timeout() -> u64 {
    25_000
}

fn default_client_idle_per_host() -> u16 {
    1
}

fn default_sender_timeout() -> u64 {
    180_000
}

fn default_sender_idle_timeout() -> u64 {
    180_000
}

fn default_sender_retry_backoff_limit() -> u64 {
    86400
}
