use serde::Deserialize;

use crate::core::serde::{default_false, default_true};

#[derive(Clone, Debug, Deserialize, Default)]
pub struct TypingConfig {
	/// Allow incoming typing updates from federation.
	#[serde(default = "default_true")]
	pub allow_incoming: bool,

	/// Allow outgoing typing updates to federation.
	#[serde(default = "default_true")]
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