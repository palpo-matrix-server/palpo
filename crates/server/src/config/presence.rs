use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct PresenceConfig {
    	/// Allow local (your server only) presence updates/requests.
	///
	/// Note that presence on tuwunel is very fast unlike Synapse's. If using
	/// outgoing presence, this MUST be enabled.
	#[serde(default = "true_fn")]
	pub allow_local: bool,

	/// Allow incoming federated presence updates/requests.
	///
	/// This option receives presence updates from other servers, but does not
	/// send any unless `allow_outgoing_presence` is true. Note that presence on
	/// tuwunel is very fast unlike Synapse's.
	#[serde(default = "true_fn")]
	pub allow_incoming: bool,

	/// Allow outgoing presence updates/requests.
	///
	/// This option sends presence updates to other servers, but does not
	/// receive any unless `allow_incoming_presence` is true. Note that presence
	/// on tuwunel is very fast unlike Synapse's. If using outgoing presence,
	/// you MUST enable `allow_local_presence` as well.
	#[serde(default = "true_fn")]
	pub allow_outgoing: bool,

	/// How many seconds without presence updates before you become idle.
	/// Defaults to 5 minutes.
	///
	/// default: 300
	#[serde(default = "default_presence_idle_timeout_s")]
	pub idle_timeout_secs: u64,

	/// How many seconds without presence updates before you become offline.
	/// Defaults to 30 minutes.
	///
	/// default: 1800
	#[serde(default = "default_presence_offline_timeout_s")]
	pub offline_timeout_secs: u64,

	/// Enable the presence idle timer for remote users.
	///
	/// Disabling is offered as an optimization for servers participating in
	/// many large rooms or when resources are limited. Disabling it may cause
	/// incorrect presence states (i.e. stuck online) to be seen for some remote
	/// users.
	#[serde(default = "true_fn")]
	pub timeout_remote_users: bool,
}

// blurhash defaults recommended by https://blurha.sh/
// 2^25
fn default_blurhash_max_raw_size() -> u64 {
    33_554_432
}

fn default_components_x() -> u32 {
    4
}

fn default_components_y() -> u32 {
    3
}
