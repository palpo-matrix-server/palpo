use salvo::prelude::*;
use salvo::size_limiter;
use url::Url;

use crate::AppResult;

mod auth;
pub use auth::{auth_by_access_token, auth_by_signatures};

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
pub fn get_origin_host(req: &mut Request) -> Option<String> {
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
