mod appservice;
mod client;
mod federation;
mod identity;
mod push;

use palpo_core::client::discovery::{ClientWellKnownResBody, HomeServerInfo, SlidingSyncProxyInfo};
use palpo_core::federation::discovery::ServerWellKnownResBody;
use salvo::http::header::{self, HeaderName};
use salvo::http::headers::authorization::{Authorization, Bearer};
use salvo::http::headers::HeaderMapExt;
use salvo::http::{Method, StatusCode};
use salvo::prelude::*;
use salvo::serve_static::StaticDir;
use salvo::size_limiter;
use url::Url;

use crate::{json_ok, AppResult, AuthArgs, DepotExt, JsonResult};

#[handler]
pub async fn limit_size(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    let mut max_size = 1024 * 1024 * 16;
    if let Some(ctype) = req.content_type() {
        if ctype.type_() == mime::MULTIPART {
            max_size = 1024 * 1024 * 1024;
        }
    }
    let limiter = size_limiter::max_size(max_size);
    limiter.handle(req, depot, res, ctrl).await;
}

pub fn router() -> Router {
    Router::new()
        .hoop(limit_size)
        .push(
            Router::with_path("_matrix")
                .push(client::router())
                .push(client::media::router())
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
        .push(Router::with_path("<*path>").get(StaticDir::new("./static")))
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

#[handler]
async fn require_authed(aa: AuthArgs, req: &mut Request, _depot: &mut Depot, res: &mut Response) {
    let auth_header = req.headers().typed_get::<Authorization<Bearer>>();
    // let token = match &auth_header {
    //     Some(Authorization(bearer)) => Some(bearer.token()),
    //     None => params.access_token.as_deref(),
    // };
    // depot.inject(sender);
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
