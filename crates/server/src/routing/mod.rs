mod appservice;
mod client;
mod federation;
mod identity;
mod push;

use std::time::Duration;

use salvo::http::header::{self, HeaderName};
use salvo::http::headers::authorization::{Authorization, Bearer};
use salvo::http::headers::HeaderMapExt;
use salvo::http::{Method, StatusCode};
use salvo::prelude::*;
use salvo::serve_static::StaticDir;
use salvo::size_limiter;
use url::Url;

use crate::{AppResult, DepotExt, JsonResult};
use crate::{AuthArgs, AuthedInfo};

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
        .push(Router::with_path("<*path>").get(StaticDir::new("./static")))
}
#[handler]
async fn access_control(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    let host = get_origin_host(req).unwrap_or_default();
    let headers = res.headers_mut();
    if host.ends_with(".sonc.ai") || host == "sonc.ai" || host.ends_with(".agora.pub") || host.ends_with(".sonc.ai") {
        headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
        headers.insert(
            "Access-Control-Allow-Methods",
            "GET,POST,PUT,DELETE,PATCH,OPTIONS".parse().unwrap(),
        );
        headers.insert(
            "Access-Control-Allow-Headers",
            "Accept,Content-Type,Authorization,Range".parse().unwrap(),
        );
        headers.insert(
            "Access-Control-Expose-Headers",
            "Access-Token,Response-Status,Content-Length,Content-Range"
                .parse()
                .unwrap(),
        );
        headers.insert("Access-Control-Allow-Credentials", "true".parse().unwrap());
    }
    headers.insert("Content-Security-Policy", "frame-ancestors 'self'".parse().unwrap());
    ctrl.call_next(req, depot, res).await;
    // headers.insert("Cross-Origin-Embedder-Policy", "require-corp".parse().unwrap());
    // headers.insert("Cross-Origin-Opener-Policy", "same-origin".parse().unwrap());
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
