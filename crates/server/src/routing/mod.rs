mod appservice;
mod client;
mod federation;
mod identity;
mod media;
mod push;

use palpo_core::client::discovery::{ClientWellKnownResBody, HomeServerInfo, SlidingSyncProxyInfo};
use palpo_core::federation::discovery::ServerWellKnownResBody;
use salvo::prelude::*;
use salvo::serve_static::StaticDir;
use url::Url;

use crate::{AppResult, JsonResult, hoops, json_ok};

pub mod prelude {
    pub use crate::core::MatrixError;
    pub use crate::core::identifiers::*;
    pub use crate::core::serde::{JsonValue, RawJson};
    pub use crate::{
        AppError, AppResult, AuthArgs, DepotExt, EmptyResult, JsonResult, OptionalExtension, empty_ok, hoops, json_ok,
    };
}

pub fn router() -> Router {
    Router::new()
        .hoop(hoops::ensure_accept)
        .hoop(hoops::limit_size)
        .push(
            Router::with_path("_matrix")
                .push(client::router())
                .push(media::router())
                .push(federation::router())
                .push(federation::key::router())
                .push(identity::router())
                .push(appservice::router())
                .push(push::router()),
        )
        .push(
            Router::with_path(".well-known/matrix")
                .push(Router::with_path("client").get(well_known_client))
                .push(Router::with_path("server").get(well_known_server)),
        )
        .push(Router::with_path("{*path}").get(StaticDir::new("./static")))
}
fn get_origin_host(req: &mut Request) -> Option<String> {
    let origin = req
        .headers()
        .get("Origin")
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        .unwrap_or_default();
    Url::parse(origin)
        .ok()
        .and_then(|url| url.host_str().map(|v| v.to_owned()))
}

#[handler]
pub async fn limit_rate() -> AppResult<()> {
    Ok(())
}

#[endpoint]
fn well_known_client() -> JsonResult<ClientWellKnownResBody> {
    let client_url = crate::well_known_client();
    json_ok(ClientWellKnownResBody {
        homeserver: HomeServerInfo {
            base_url: client_url.clone(),
        },
        identity_server: None,
        tile_server: None,
        authentication: None,
        sliding_sync_proxy: Some(SlidingSyncProxyInfo { url: client_url }),
    })
}
#[endpoint]
fn well_known_server() -> JsonResult<ServerWellKnownResBody> {
    json_ok(ServerWellKnownResBody {
        server: crate::well_known_server(),
    })
}
