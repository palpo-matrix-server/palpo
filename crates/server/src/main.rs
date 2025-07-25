#![allow(dead_code, missing_docs)]
// #![deny(unused_crate_dependencies)]
// #[macro_use]
// extern crate diesel;
// extern crate dotenvy;
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

pub mod auth;
pub mod config;
pub mod env_vars;
pub mod hoops;
pub mod routing;
pub mod utils;
pub use auth::{AuthArgs, AuthedInfo};
pub mod admin;
pub mod appservice;
pub mod directory;
pub mod event;
pub mod exts;
pub mod federation;
pub mod media;
pub mod membership;
pub mod room;
pub mod sending;
pub mod server_key;
pub mod state;
pub mod transaction_id;
pub mod uiaa;
pub mod user;
pub use exts::*;
mod cjson;
pub use cjson::Cjson;
mod signing_keys;
pub mod sync_v3;
pub mod sync_v5;
pub mod watcher;
pub use event::{PduBuilder, PduEvent, SnPduEvent};
pub use signing_keys::SigningKeys;
mod global;
pub use global::*;

pub mod error;
pub use core::error::MatrixError;

pub use error::AppError;
pub use palpo_core as core;
pub use palpo_data as data;
pub use palpo_server_macros as macros;

use std::time::Duration;

pub use diesel::result::Error as DieselError;
use dotenvy::dotenv;
use salvo::catcher::Catcher;
use salvo::compression::{Compression, CompressionLevel};
use salvo::conn::rustls::{Keycert, RustlsConfig};
use salvo::cors::{self, AllowHeaders, Cors};
use salvo::http::Method;
use salvo::logging::Logger;
use salvo::prelude::*;
use tracing_futures::Instrument;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::config::ServerConfig;

pub type AppResult<T> = Result<T, crate::AppError>;
pub type DieselResult<T> = Result<T, diesel::result::Error>;
pub type JsonResult<T> = Result<Json<T>, crate::AppError>;
pub type CjsonResult<T> = Result<Cjson<T>, crate::AppError>;
pub type EmptyResult = Result<Json<EmptyObject>, crate::AppError>;

pub fn json_ok<T>(data: T) -> JsonResult<T> {
    Ok(Json(data))
}
pub fn cjson_ok<T>(data: T) -> CjsonResult<T> {
    Ok(Cjson(data))
}
pub fn empty_ok() -> JsonResult<EmptyObject> {
    Ok(Json(EmptyObject {}))
}

pub trait OptionalExtension<T> {
    fn optional(self) -> AppResult<Option<T>>;
}

impl<T> OptionalExtension<T> for AppResult<T> {
    fn optional(self) -> AppResult<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(AppError::Matrix(e)) => {
                if e.is_not_found() {
                    Ok(None)
                } else {
                    Err(AppError::Matrix(e))
                }
            }
            Err(AppError::Diesel(diesel::result::Error::NotFound)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[macro_export]
macro_rules! join_path {
    ($($part:expr),+) => {
        {
            let mut p = std::path::PathBuf::new();
            $(
                p.push($part);
            )*
            path_slash::PathBufExt::to_slash_lossy(&p).to_string()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // if dotenvy::from_filename(".env.local").is_err() {
    //     println!(".env.local file is not found");
    // }
    if let Err(e) = dotenv() {
        tracing::info!("dotenv error: {:?}", e);
    }

    crate::config::init();
    let conf = crate::config::get();
    conf.check().expect("config is not valid!");
    match &*conf.logger.format {
        "json" => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(&conf.logger.level)
                .with_ansi(conf.logger.ansi_colors)
                .with_thread_ids(conf.logger.thread_ids)
                .with_span_events(FmtSpan::CLOSE)
                .init();
        }
        "compact" => {
            tracing_subscriber::fmt()
                .compact()
                .with_env_filter(&conf.logger.level)
                .with_ansi(conf.logger.ansi_colors)
                .with_thread_ids(conf.logger.thread_ids)
                .without_time()
                .with_span_events(FmtSpan::CLOSE)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .pretty()
                .with_env_filter(&conf.logger.level)
                .with_ansi(conf.logger.ansi_colors)
                .with_thread_ids(conf.logger.thread_ids)
                .with_span_events(FmtSpan::CLOSE)
                .init();
        }
    }

    crate::data::init(&conf.db.clone().into_data_db_config());

    crate::sending::guard::start();

    let router = routing::router();
    // let doc = OpenApi::new("palpo api", "0.0.1").merge_router(&router);
    // let router = router
    //     .unshift(doc.into_router("/api-doc/openapi.json"))
    //     .unshift(
    //         Scalar::new("/api-doc/openapi.json")
    //             .title("Palpo - Scalar")
    //             .into_router("/scalar"),
    //     )
    //     .unshift(SwaggerUi::new("/api-doc/openapi.json").into_router("/swagger-ui"));
    let catcher = Catcher::default().hoop(hoops::catch_status_error);
    let service = Service::new(router)
        .catcher(catcher)
        .hoop(hoops::default_accept_json)
        .hoop(Logger::new())
        .hoop(
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
        )
        .hoop(hoops::remove_json_utf8);
    let service = if conf.compression.is_enabled() {
        let mut compression = Compression::new();
        if conf.compression.enable_brotli {
            compression = compression.enable_zstd(CompressionLevel::Fastest);
        }
        if conf.compression.enable_zstd {
            compression = compression.enable_zstd(CompressionLevel::Fastest);
        }
        if conf.compression.enable_gzip {
            compression = compression.enable_gzip(CompressionLevel::Fastest);
        }
        service.hoop(compression)
    } else {
        service
    };
    crate::admin::supervise();
    let _ = crate::data::user::unset_all_presences();

    salvo::http::request::set_global_secure_max_size(8 * 1024 * 1024);
    let conf = crate::config::get();
    println!("Listening on {}", conf.listen_addr);
    if let Some(tls_conf) = conf.enabled_tls() {
        let acceptor = TcpListener::new(&conf.listen_addr)
            .rustls(RustlsConfig::new(
                Keycert::new()
                    .cert_from_path(&tls_conf.cert)?
                    .key_from_path(&tls_conf.key)?,
            ))
            .bind()
            .await;
        Server::new(acceptor)
            .serve(service)
            .instrument(tracing::info_span!("server.serve"))
            .await
    } else {
        let acceptor = TcpListener::new(&conf.listen_addr).bind().await;
        Server::new(acceptor)
            .serve(service)
            .instrument(tracing::info_span!("server.serve"))
            .await
    };
    Ok(())
}
