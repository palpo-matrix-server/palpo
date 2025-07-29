use serde::Deserialize;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "logger")]
#[derive(Clone, Debug, Deserialize)]
pub struct LoggerConfig {
    /// Max log level for palpo. Allows debug, info, warn, or error.
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

    // pretty, compact, json
    #[serde(default = "default_format")]
    pub format: String,

    /// Output logs with ANSI colours.
    #[serde(default = "default_true")]
    pub ansi_colors: bool,

    /// Configures the span events which will be outputted with the log.
    ///
    /// default: "none"
    #[serde(default = "default_span_events")]
    pub span_events: String,

    /// Configures whether EnvFilter matches values using regular expressions.
    /// See the tracing_subscriber documentation on Directives.
    ///
    /// default: true
    #[serde(default = "default_true")]
    pub filter_regex: bool,

    /// Toggles the display of ThreadId in tracing log output.
    ///
    /// default: false
    #[serde(default)]
    pub thread_ids: bool,

    /// Set to true to log guest registrations in the admin room. Note that
    /// these may be noisy or unnecessary if you're a public homeserver.
    #[serde(default)]
    pub guest_registrations: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            format: default_format(),
            ansi_colors: true,
            span_events: default_span_events(),
            filter_regex: true,
            thread_ids: false,
            guest_registrations: false,
        }
    }
}

/// do debug logging by default for debug builds
#[must_use]
pub fn default_level() -> String {
    cfg!(debug_assertions)
        .then_some("debug")
        .unwrap_or("info")
        .to_owned()
}

/// do compact logging by default
#[must_use]
pub fn default_format() -> String {
    "pretty".to_owned()
}
#[must_use]
pub fn default_span_events() -> String {
    "none".into()
}
