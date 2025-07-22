use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct LoggerConfig {
	/// Max log level for tuwunel. Allows debug, info, warn, or error.
	///
	/// See also:
	/// https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives
	///
	/// **Caveat**:
	/// For release builds, the tracing crate is configured to only implement
	/// levels higher than error to avoid unnecessary overhead in the compiled
	/// binary from trace macros. For debug builds, this restriction is not
	/// applied.
	///
	/// default: "info"
	#[serde(default = "default_level")]
	pub level: String,

	/// Output logs with ANSI colours.
	#[serde(default = "true_fn")]
	pub ansi_colors: bool,

	/// Configures the span events which will be outputted with the log.
	///
	/// default: "none"
	#[serde(default = "default_log_span_events")]
	pub span_events: String,

	/// Configures whether TUWUNEL_LOG EnvFilter matches values using regular
	/// expressions. See the tracing_subscriber documentation on Directives.
	///
	/// default: true
	#[serde(default = "true_fn")]
	pub filter_regex: bool,

	/// Toggles the display of ThreadId in tracing log output.
	///
	/// default: false
	#[serde(default)]
	pub thread_ids: bool,

	/// Set to true to log guest registrations in the admin room. Note that
	/// these may be noisy or unnecessary if you're a public homeserver.
    pub guest_registrations: bool,
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
