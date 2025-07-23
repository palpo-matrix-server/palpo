use serde::Deserialize;

use crate::core::serde::{default_false, default_true};
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "dns")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct DnsConfig {
    /// Maximum entries stored in DNS memory-cache. The size of an entry may
    /// vary so please take care if raising this value excessively. Only
    /// decrease this when using an external DNS cache. Please note that
    /// systemd-resolved does *not* count as an external cache, even when
    /// configured to do so.
    ///
    /// default: 32768
    #[serde(default = "default_cache_entries")]
    pub cache_entries: u32,

    /// Minimum time-to-live in seconds for entries in the DNS cache. The
    /// default may appear high to most administrators; this is by design as the
    /// majority of NXDOMAINs are correct for a long time (e.g. the server is no
    /// longer running Matrix). Only decrease this if you are using an external
    /// DNS cache.
    ///
    /// default: 10800
    #[serde(default = "default_min_ttl")]
    pub min_ttl: u64,

    /// Minimum time-to-live in seconds for NXDOMAIN entries in the DNS cache.
    /// This value is critical for the server to federate efficiently.
    /// NXDOMAIN's are assumed to not be returning to the federation and
    /// aggressively cached rather than constantly rechecked.
    ///
    /// Defaults to 3 days as these are *very rarely* false negatives.
    ///
    /// default: 259200
    #[serde(default = "default_min_ttl_nxdomain")]
    pub min_ttl_nxdomain: u64,

    /// Number of DNS nameserver retries after a timeout or error.
    ///
    /// default: 10
    #[serde(default = "default_attempts")]
    pub attempts: u16,

    /// The number of seconds to wait for a reply to a DNS query. Please note
    /// that recursive queries can take up to several seconds for some domains,
    /// so this value should not be too low, especially on slower hardware or
    /// resolvers.
    ///
    /// default: 10
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Fallback to TCP on DNS errors. Set this to false if unsupported by
    /// nameserver.
    #[serde(default = "default_true")]
    pub tcp_fallback: bool,
}

fn default_cache_entries() -> u32 {
    32768
}

fn default_min_ttl() -> u64 {
    60 * 180
}

fn default_min_ttl_nxdomain() -> u64 {
    60 * 60 * 24 * 3
}

fn default_attempts() -> u16 {
    10
}

fn default_timeout() -> u64 {
    10
}
