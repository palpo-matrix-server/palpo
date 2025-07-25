use serde::Deserialize;

use crate::core::serde::{default_false, default_true};
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "compression")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct CompressionConfig {
    /// Set this to true for palpo to compress HTTP response bodies using
    /// zstd.
    #[serde(default)]
    pub enable_zstd: bool,

    /// Set this to true for palpo to compress HTTP response bodies using
    /// gzip.
    ///
    /// If you are in a large amount of rooms, you may find that enabling this
    /// is necessary to reduce the significantly large response bodies.
    #[serde(default)]
    pub enable_gzip: bool,

    /// Set this to true for palpo to compress HTTP response bodies using
    /// brotli.
    #[serde(default)]
    pub enable_brotli: bool,
}

impl CompressionConfig {
    pub fn is_enabled(&self) -> bool {
        self.enable_zstd || self.enable_gzip || self.enable_brotli
    }
}