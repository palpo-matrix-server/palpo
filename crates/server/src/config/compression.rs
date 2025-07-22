use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct TypingConfig {
	
	/// Set this to true for tuwunel to compress HTTP response bodies using
	/// zstd. This option does nothing if tuwunel was not built with
	/// `zstd_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
	/// before deciding to enable this.
	#[serde(default = "true_fn")]
	pub zstd_enabled: bool,

	/// Set this to true for tuwunel to compress HTTP response bodies using
	/// gzip. This option does nothing if tuwunel was not built with
	/// `gzip_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH before
	/// deciding to enable this.
	///
	/// If you are in a large amount of rooms, you may find that enabling this
	/// is necessary to reduce the significantly large response bodies.
	#[serde(default = "true_fn")]
	pub zip_enabled: bool,

	/// Set this to true for tuwunel to compress HTTP response bodies using
	/// brotli. This option does nothing if tuwunel was not built with
	/// `brotli_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
	/// before deciding to enable this.
	#[serde(default = "default_typing_federation_timeout_s")]
	pub brotli_enabled: u64,
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
