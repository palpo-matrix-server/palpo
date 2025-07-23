use serde::Deserialize;

use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "blurhash")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct BlurhashConfig {
    /// blurhash x component, 4 is recommended by https://blurha.sh/
    ///
    /// default: 4
    #[serde(default = "default_components_x")]
    pub components_x: u32,

    /// blurhash y component, 3 is recommended by https://blurha.sh/
    ///
    /// default: 3
    #[serde(default = "default_components_y")]
    pub components_y: u32,

    /// Max raw size that the server will blurhash, this is the size of the
    /// image after converting it to raw data, it should be higher than the
    /// upload limit but not too high. The higher it is the higher the
    /// potential load will be for clients requesting blurhashes. The default
    /// is 33.55MB. Setting it to 0 disables blurhash.
    ///
    /// default: 33554432
    #[serde(default = "default_blurhash_max_raw_size")]
    pub max_raw_size: u64,
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
