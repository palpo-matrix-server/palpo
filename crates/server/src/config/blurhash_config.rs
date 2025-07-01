use reqwest::{Proxy, Url};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct BlurhashConfig {
    /// blurhashing x component, 4 is recommended by https://blurha.sh/
    ///
    /// default: 4
    #[serde(default = "default_blurhash_x_component")]
    pub components_x: u32,
    /// blurhashing y component, 3 is recommended by https://blurha.sh/
    ///
    /// default: 3
    #[serde(default = "default_blurhash_y_component")]
    pub components_y: u32,
    /// Max raw size that the server will blurhash, this is the size of the
    /// image after converting it to raw data, it should be higher than the
    /// upload limit but not too high. The higher it is the higher the
    /// potential load will be for clients requesting blurhashes. The default
    /// is 33.55MB. Setting it to 0 disables blurhashing.
    ///
    /// default: 33554432
    #[serde(default = "default_blurhash_max_raw_size")]
    pub max_raw_size: u64,
}

// blurhashing defaults recommended by https://blurha.sh/
// 2^25
fn default_blurhash_max_raw_size() -> u64 { 33_554_432 }

fn default_blurhash_x_component() -> u32 { 4 }

fn default_blurhash_y_component() -> u32 { 3 }