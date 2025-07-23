use serde::Deserialize;

use crate::core::serde::{default_false, default_true};

#[derive(Clone, Debug, Deserialize, Default)]
pub struct PresenceConfig {
    	/// Allow local (your server only) presence updates/requests.
	///
	/// Note that presence on palpo is very fast unlike Synapse's. If using
	/// outgoing presence, this MUST be enabled.
	#[serde(default = "default_true")]
	pub allow_local: bool,

	/// Allow incoming federated presence updates/requests.
	///
	/// This option receives presence updates from other servers, but does not
	/// send any unless `allow_outgoing_presence` is true. Note that presence on
	/// palpo is very fast unlike Synapse's.
	#[serde(default = "default_true")]
	pub allow_incoming: bool,

	/// Allow outgoing presence updates/requests.
	///
	/// This option sends presence updates to other servers, but does not
	/// receive any unless `allow_incoming_presence` is true. Note that presence
	/// on palpo is very fast unlike Synapse's. If using outgoing presence,
	/// you MUST enable `allow_local_presence` as well.
	#[serde(default = "default_true")]
	pub allow_outgoing: bool,

	/// How many seconds without presence updates before you become idle.
	/// Defaults to 5 minutes.
	///
	/// default: 300_000
	#[serde(default = "default_presence_idle_timeout")]
	pub idle_timeout: u64,

	/// How many seconds without presence updates before you become offline.
	/// Defaults to 30 minutes.
	///
	/// default: 1800_000
	#[serde(default = "default_presence_offline_timeout")]
	pub offline_timeout: u64,

	/// Enable the presence idle timer for remote users.
	///
	/// Disabling is offered as an optimization for servers participating in
	/// many large rooms or when resources are limited. Disabling it may cause
	/// incorrect presence states (i.e. stuck online) to be seen for some remote
	/// users.
	#[serde(default = "default_true")]
	pub timeout_remote_users: bool,
}

fn default_presence_offline_timeout() -> u64 { 30 * 60 * 1000 }

fn default_presence_idle_timeout() -> u64 { 5 * 60 * 1000 }