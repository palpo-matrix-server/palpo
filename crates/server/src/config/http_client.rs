use serde::Deserialize;

use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "client")]
#[derive(Clone, Debug, Deserialize)]
pub struct HttpClientConfig {
    /// Well-known resolution connection timeout.
    ///
    /// default: 6_000
    #[serde(default = "default_well_known_conn_timeout")]
    pub well_known_conn_timeout: u64,

    /// HTTP well-known resolution request timeout.
    ///
    /// default: 10_000
    #[serde(default = "default_well_known_timeout")]
    pub well_known_timeout: u64,

    /// Federation client request timeout. You most definitely want
    /// this to be high to account for extremely large room joins, slow
    /// homeservers, your own resources etc.
    ///
    /// default: 300_000
    #[serde(default = "default_federation_timeout")]
    pub federation_timeout: u64,

    /// Federation client request retry times.
    ///
    /// default: 3
    #[serde(default = "default_federation_retries")]
    pub federation_retries: u32,

    /// Federation client idle connection pool timeout.
    ///
    /// default: 25
    #[serde(default = "default_federation_idle_timeout")]
    pub federation_idle_timeout: u64,

    /// Federation client max idle connections per host. Defaults to 1 as
    /// generally the same open connection can be re-used.
    ///
    /// default: 1
    #[serde(default = "default_federation_idle_per_host")]
    pub federation_idle_per_host: u16,
    // /// Federation sender request timeout. The time it takes for the
    // /// remote server to process sent transactions can take a while.
    // ///
    // /// default: 180_000
    // #[serde(default = "default_sender_timeout")]
    // pub sender_timeout: u64,

    // /// Federation sender idle connection pool timeout.
    // ///
    // /// default: 180_000
    // #[serde(default = "default_sender_idle_timeout")]
    // pub sender_idle_timeout: u64,

    // /// Federation sender transaction retry backoff limit.
    // ///
    // /// default: 86400_000
    // #[serde(default = "default_sender_retry_backoff_limit")]
    // pub sender_retry_backoff_limit: u64,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            well_known_conn_timeout: default_well_known_conn_timeout(),
            well_known_timeout: default_well_known_timeout(),
            federation_timeout: default_federation_timeout(),
            federation_retries: default_federation_retries(),
            federation_idle_timeout: default_federation_idle_timeout(),
            federation_idle_per_host: default_federation_idle_per_host(),
            // sender_timeout: default_sender_timeout(),
            // sender_idle_timeout: default_sender_idle_timeout(),
            // sender_retry_backoff_limit: default_sender_retry_backoff_limit(),
        }
    }
}

fn default_well_known_conn_timeout() -> u64 {
    6_000
}

fn default_well_known_timeout() -> u64 {
    10_000
}

fn default_federation_timeout() -> u64 {
    25_000
}

fn default_federation_retries() -> u32 {
    2
}

fn default_federation_idle_timeout() -> u64 {
    25_000
}

fn default_federation_idle_per_host() -> u16 {
    1
}

// fn default_sender_timeout() -> u64 {
//     180_000
// }

// fn default_sender_idle_timeout() -> u64 {
//     180_000
// }

// fn default_sender_retry_backoff_limit() -> u64 {
//     86400
// }
