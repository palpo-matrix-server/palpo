use serde::Deserialize;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "admin")]
#[derive(Clone, Debug, Deserialize)]
pub struct AccountConfig {
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self {
        }
    }
}