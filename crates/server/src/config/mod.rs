mod db_config;
mod server_config;

use once_cell::sync::OnceCell;

pub use db_config::*;
pub use server_config::*;

pub static CONFIG: OnceCell<ServerConfig> = OnceCell::new();

pub fn get() -> &'static ServerConfig {
    CONFIG.get().unwrap()
}
