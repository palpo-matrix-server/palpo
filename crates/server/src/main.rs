#![allow(dead_code)]
// #![deny(unused_crate_dependencies)]
#[macro_use]
extern crate diesel;
extern crate dotenvy;
// #[macro_use]
// extern crate thiserror;
// #[macro_use]
// extern crate anyhow;
// #[macro_use]
// mod macros;
// #[macro_use]
// pub mod permission;

#[macro_use]
extern crate tracing;

pub mod routing;
// pub(crate) mod schema;
pub mod bl;
pub use bl::*;
pub mod config;
pub mod db;
pub mod env_vars;
pub mod hoops;
pub mod schema;
pub mod utils;

pub mod error;
pub use crate::core::error::MatrixError;
pub use error::AppError;
pub use palpo_core as core;
#[macro_use]
mod macros;
#[macro_use]
use serde_json;
pub(crate) use serde_json::Value as JsonValue;

use std::env;
use std::sync::Arc;
use std::time::Duration;

use diesel::r2d2;
use dotenvy::dotenv;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use salvo::cors::{self, AllowHeaders, Cors};
use salvo::http::header::{self, HeaderName};
use salvo::http::Method;
use salvo::logging::Logger;

pub use diesel::result::Error as DieselError;
use salvo::prelude::*;
use scheduled_thread_pool::ScheduledThreadPool;
use tracing_futures::Instrument;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::config::ServerConfig;
use crate::db::{ConnectionConfig, DieselPool};

pub type AppResult<T> = Result<T, crate::AppError>;
pub type JsonResult<T> = Result<Json<T>, crate::AppError>;
pub type EmptyResult = Result<Json<EmptyObject>, crate::AppError>;

pub fn json_ok<T>(data: T) -> JsonResult<T> {
    Ok(Json(data))
}
pub fn empty_ok() -> JsonResult<EmptyObject> {
    Ok(Json(EmptyObject {}))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // if dotenvy::from_filename(".env.local").is_err() {
    //     println!(".env.local file is not found");
    // }
    if let Err(e) = dotenv() {
        println!("dotenv error: {:?}", e);
    }
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "palpo=warn,palpo_core=warn,salvo=warn".to_owned());
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .init();

    println!("RUST_LOG: {}", env::var("RUST_LOG").unwrap_or_default());

    salvo::http::request::set_secure_max_size(1024 * 1024 * 100);

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

    let thread_pool = Arc::new(ScheduledThreadPool::new(conf.db.helper_threads));

    let db_primary = {
        let db_connection_config = ConnectionConfig {
            statement_timeout: conf.db.statement_timeout,
        };

        let db_config = r2d2::Pool::builder()
            .max_size(conf.db.pool_size)
            .min_idle(conf.db.min_idle)
            .connection_timeout(Duration::from_millis(conf.db.connection_timeout))
            .connection_customizer(Box::new(db_connection_config))
            .thread_pool(thread_pool.clone());

        DieselPool::new(&conf.db.url, &conf.db, db_config).unwrap()
    };
    crate::db::DIESEL_POOL.set(db_primary).expect("diesel pool should be set");
    crate::config::CONFIG.set(conf).expect("config should be set");

    let acceptor = TcpListener::new(crate::server_addr()).bind().await;
    salvo::http::request::set_secure_max_size(8 * 1024 * 1024);

    let router = routing::router();
    let doc = OpenApi::new("palpo api", "0.0.1").merge_router(&router);
    let router = router
        .unshift(doc.into_router("/api-doc/openapi.json"))
        .unshift(
            Scalar::new("/api-doc/openapi.json")
                .title("Palpo - Scalar")
                .into_router("/scalar"),
        )
        .unshift(SwaggerUi::new("/api-doc/openapi.json").into_router("/swagger-ui"));
    let service = Service::new(router).hoop(Logger::new()).hoop(
        Cors::new()
            .allow_origin(cors::Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
            .allow_headers(AllowHeaders::list([
                salvo::http::header::ACCEPT,
                salvo::http::header::CONTENT_TYPE,
                salvo::http::header::AUTHORIZATION,
                salvo::http::header::RANGE,
            ]))
            .max_age(Duration::from_secs(86400))
            .into_handler(),
    );
    crate::admin::supervise();
    Server::new(acceptor)
        .serve(service)
        .instrument(tracing::info_span!("server.serve"))
        .await;
    Ok(())
}
