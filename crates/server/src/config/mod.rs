
mod server_config;

use std::sync::OnceLock;

use figment::providers::{Env, Format, Toml};
use figment::Figment;

pub use server_config::*;
pub use crate::data::DbConfig;

pub static CONFIG: OnceLock<ServerConfig> = OnceLock::new();

pub fn init() {
    let raw_config = Figment::new()
        .merge(Toml::file(Env::var("PALPO_CONFIG").as_deref().unwrap_or("palpo.toml")))
        .merge(Env::prefixed("PALPO_").global());

    let conf = match raw_config.extract::<ServerConfig>() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("It looks like your config is invalid. The following error occurred: {e}");
            std::process::exit(1);
        }
    };

    crate::config::CONFIG.set(conf).expect("config should be set");
}
pub fn get() -> &'static ServerConfig {
    CONFIG.get().unwrap()
}
