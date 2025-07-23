use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::Deserialize;

use crate::core::serde::{default_false, default_true};

#[derive(Clone, Debug, Deserialize, Default)]
pub struct FederationConfig {
    /// Allows federation requests to be made to itself
    ///
    /// This isn't intended and is very likely a bug if federation requests are
    /// being sent to yourself. This currently mainly exists for development
    /// purposes.
    #[serde(default)]
    pub allow_loopback: bool,

	/// Federation well-known resolution connection timeout (seconds).
	///
	/// default: 6
	#[serde(default = "default_well_known_conn_timeout")]
	pub well_known_conn_timeout: u64,

	/// Federation HTTP well-known resolution request timeout (seconds).
	///
	/// default: 10
	#[serde(default = "default_well_known_timeout")]
	pub well_known_timeout: u64,

	/// Federation client request timeout (seconds). You most definitely want
	/// this to be high to account for extremely large room joins, slow
	/// homeservers, your own resources etc.
	///
	/// default: 300
	#[serde(default = "default_client_timeout")]
	pub client_timeout: u64,

	/// Federation client idle connection pool timeout (seconds).
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

	/// Federation sender request timeout (seconds). The time it takes for the
	/// remote server to process sent transactions can take a while.
	///
	/// default: 180
	#[serde(default = "default_sender_timeout")]
	pub sender_timeout: u64,

	/// Federation sender idle connection pool timeout (seconds).
	///
	/// default: 180
	#[serde(default = "default_sender_idle_timeout")]
	pub sender_idle_timeout: u64,

	/// Federation sender transaction retry backoff limit (seconds).
	///
	/// default: 86400
	#[serde(default = "default_sender_retry_backoff_limit")]
	pub sender_retry_backoff_limit: u64,
}


fn default_well_known_conn_timeout() -> u64 { 6 }

fn default_well_known_timeout() -> u64 { 10 }

fn default_client_timeout() -> u64 { 25 }

fn default_client_idle_timeout() -> u64 { 25 }

fn default_client_idle_per_host() -> u16 { 1 }

fn default_sender_timeout() -> u64 { 180 }

fn default_sender_idle_timeout() -> u64 { 180 }

fn default_sender_retry_backoff_limit() -> u64 { 86400 }
