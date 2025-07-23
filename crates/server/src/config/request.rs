use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::Deserialize;

use crate::core::serde::{default_false, default_true};

#[derive(Clone, Debug, Deserialize, Default)]
pub struct RequestConfig {
    /// Max request size for file uploads in bytes. Defaults to 20MB.
    ///
    /// default: 20971520
    #[serde(default = "default_max_request_size")]
    pub max_request_size: u32,

    /// default: 192
    #[serde(default = "default_max_fetch_prev_events")]
    pub max_fetch_prev_events: u16,

    /// Default/base connection timeout (seconds). This is used only by URL
    /// previews and update/news endpoint checks.
    ///
    /// default: 10
    #[serde(default = "default_request_conn_timeout")]
    pub request_conn_timeout: u64,

    /// Default/base request timeout (seconds). The time waiting to receive more
    /// data from another server. This is used only by URL previews,
    /// update/news, and misc endpoint checks.
    ///
    /// default: 35
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,

    /// Default/base request total timeout (seconds). The time limit for a whole
    /// request. This is set very high to not cancel healthy requests while
    /// serving as a backstop. This is used only by URL previews and update/news
    /// endpoint checks.
    ///
    /// default: 320
    #[serde(default = "default_request_total_timeout")]
    pub request_total_timeout: u64,

    /// Default/base idle connection pool timeout (seconds). This is used only
    /// by URL previews and update/news endpoint checks.
    ///
    /// default: 5
    #[serde(default = "default_request_idle_timeout")]
    pub request_idle_timeout: u64,

    /// Default/base max idle connections per host. This is used only by URL
    /// previews and update/news endpoint checks. Defaults to 1 as generally the
    /// same open connection can be re-used.
    ///
    /// default: 1
    #[serde(default = "default_request_idle_per_host")]
    pub request_idle_per_host: u16,
}
fn default_max_request_size() -> usize {
	20 * 1024 * 1024 // Default to 20 MB
}
