use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct TypingConfig {
	/// Allow incoming typing updates from federation.
	#[serde(default = "true_fn")]
	pub allow_incoming: bool,

	/// Allow outgoing typing updates to federation.
	#[serde(default = "true_fn")]
	pub allow_outgoing: bool,

	/// Maximum time federation user can indicate typing.
	///
	/// default: 30
	#[serde(default = "default_typing_federation_timeout_s")]
	pub federation_timeout_s: u64,

	/// Minimum time local client can indicate typing. This does not override a
	/// client's request to stop typing. It only enforces a minimum value in
	/// case of no stop request.
	///
	/// default: 15
	#[serde(default = "default_typing_client_timeout_min_s")]
	pub client_timeout_min_s: u64,

	/// Maximum time local client can indicate typing.
	///
	/// default: 45
	#[serde(default = "default_typing_client_timeout_max_s")]
	pub client_timeout_max_s: u64,
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
