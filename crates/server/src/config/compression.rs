use serde::Deserialize;


use crate::core::serde::{default_false, default_true};

#[derive(Clone, Debug, Deserialize, Default)]
pub struct CompressionConfig {
	/// Set this to true for palpo to compress HTTP response bodies using
	/// zstd. This option does nothing if palpo was not built with
	/// `zstd_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
	/// before deciding to enable this.
	#[serde(default)]
	pub zstd_enabled: bool,

	/// Set this to true for palpo to compress HTTP response bodies using
	/// gzip. This option does nothing if palpo was not built with
	/// `gzip_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH before
	/// deciding to enable this.
	///
	/// If you are in a large amount of rooms, you may find that enabling this
	/// is necessary to reduce the significantly large response bodies.
	#[serde(default)]
	pub zip_enabled: bool,

	/// Set this to true for palpo to compress HTTP response bodies using
	/// brotli. This option does nothing if palpo was not built with
	/// `brotli_compression` feature. Please be aware that enabling HTTP
	/// compression may weaken TLS. Most users should not need to enable this.
	/// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
	/// before deciding to enable this.
	#[serde(default)]
	pub brotli_enabled: u64,
}

fn default_typing_federation_timeout_s() -> u64 { 30 }