mod db_config;
mod server_config;

use std::sync::OnceLock;

pub use db_config::*;
pub use server_config::*;

pub static CONFIG: OnceLock<ServerConfig> = OnceLock::new();

pub fn get() -> &'static ServerConfig {
    CONFIG.get().unwrap()
}
