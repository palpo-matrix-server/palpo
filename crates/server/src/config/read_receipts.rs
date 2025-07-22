use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct ReadReceiptsConfig {
	/// Allow receiving incoming read receipts from remote servers.
	#[serde(default = "true_fn")]
	pub allow_incoming: bool,

	/// Allow sending read receipts to remote servers.
	#[serde(default = "true_fn")]
	pub allow_outgoing: bool,
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
