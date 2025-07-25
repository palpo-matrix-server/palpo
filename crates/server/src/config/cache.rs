use serde::Deserialize;

use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "cache")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct CacheConfig {}
