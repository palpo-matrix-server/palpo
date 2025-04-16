mod preview;
pub use preview::*;

use std::time::Duration;

use salvo::Response;
use url::Url;

use crate::core::federation::media::ContentReqArgs;
use crate::core::{ServerName, media};
use crate::{AppResult, exts::*, join_path};

pub async fn get_remote_content(
    mxc: &str,
    server_name: &ServerName,
    media_id: &str,
    res: &mut Response,
) -> AppResult<()> {
    let content_req = crate::core::media::content_request(
        &server_name.origin().await,
        media::ContentReqArgs {
            server_name: server_name.to_owned(),
            media_id: media_id.to_owned(),
            timeout_ms: Duration::from_secs(20),
            allow_remote: true,
            allow_redirect: true,
        },
    )?
    .into_inner();
    let content_response =
        if let Ok(content_response) = crate::sending::send_federation_request(server_name, content_req).await {
            content_response
        } else {
            let content_req = crate::core::federation::media::content_request(
                &server_name.origin().await,
                ContentReqArgs {
                    media_id: media_id.to_owned(),
                    timeout_ms: Duration::from_secs(20),
                },
            )?
            .into_inner();
            crate::sending::send_federation_request(server_name, content_req).await?
        };

    *res.headers_mut() = content_response.headers().to_owned();
    res.status_code(content_response.status());
    res.stream(content_response.bytes_stream());

    Ok(())
}

fn get_media_path(key: &str) -> String {
    join_path!(&crate::config().space_path, "media", key)
}

/// Returns width, height of the thumbnail and whether it should be cropped. Returns None when
/// the server should send the original file.
pub fn thumbnail_properties(width: u32, height: u32) -> Option<(u32, u32, bool)> {
    match (width, height) {
        (0..=32, 0..=32) => Some((32, 32, true)),
        (0..=96, 0..=96) => Some((96, 96, true)),
        (0..=320, 0..=240) => Some((320, 240, false)),
        (0..=640, 0..=480) => Some((640, 480, false)),
        (0..=800, 0..=600) => Some((800, 600, false)),
        _ => None,
    }
}

pub fn url_preview_allowed(url: &Url) -> bool {
    if ["http", "https"]
        .iter()
        .all(|&scheme| scheme != url.scheme().to_lowercase())
    {
        debug!("Ignoring non-HTTP/HTTPS URL to preview: {}", url);
        return false;
    }

    let host = match url.host_str() {
        None => {
            debug!("Ignoring URL preview for a URL that does not have a host (?): {}", url);
            return false;
        }
        Some(h) => h.to_owned(),
    };

    let conf = crate::config();
    let allowlist_domain_contains = &conf.url_preview_domain_contains_allowlist;
    let allowlist_domain_explicit = &conf.url_preview_domain_explicit_allowlist;
    let denylist_domain_explicit = &conf.url_preview_domain_explicit_denylist;
    let allowlist_url_contains = &conf.url_preview_url_contains_allowlist;

    if allowlist_domain_contains.contains(&"*".to_owned())
        || allowlist_domain_explicit.contains(&"*".to_owned())
        || allowlist_url_contains.contains(&"*".to_owned())
    {
        debug!(
            "Config key contains * which is allowing all URL previews. Allowing URL {}",
            url
        );
        return true;
    }

    if !host.is_empty() {
        if denylist_domain_explicit.contains(&host) {
            debug!(
                "Host {} is not allowed by url_preview_domain_explicit_denylist (check 1/4)",
                &host
            );
            return false;
        }

        if allowlist_domain_explicit.contains(&host) {
            debug!(
                "Host {} is allowed by url_preview_domain_explicit_allowlist (check 2/4)",
                &host
            );
            return true;
        }

        if allowlist_domain_contains
            .iter()
            .any(|domain_s| domain_s.contains(&host.clone()))
        {
            debug!(
                "Host {} is allowed by url_preview_domain_contains_allowlist (check 3/4)",
                &host
            );
            return true;
        }

        if allowlist_url_contains
            .iter()
            .any(|url_s| url.to_string().contains(url_s))
        {
            debug!(
                "URL {} is allowed by url_preview_url_contains_allowlist (check 4/4)",
                &host
            );
            return true;
        }

        // check root domain if available and if user has root domain checks
        if conf.url_preview_check_root_domain {
            debug!("Checking root domain");
            match host.split_once('.') {
                None => return false,
                Some((_, root_domain)) => {
                    if denylist_domain_explicit.contains(&root_domain.to_owned()) {
                        debug!(
                            "Root domain {} is not allowed by \
    						 url_preview_domain_explicit_denylist (check 1/3)",
                            &root_domain
                        );
                        return true;
                    }

                    if allowlist_domain_explicit.contains(&root_domain.to_owned()) {
                        debug!(
                            "Root domain {} is allowed by url_preview_domain_explicit_allowlist \
    						 (check 2/3)",
                            &root_domain
                        );
                        return true;
                    }

                    if allowlist_domain_contains
                        .iter()
                        .any(|domain_s| domain_s.contains(&root_domain.to_owned()))
                    {
                        debug!(
                            "Root domain {} is allowed by url_preview_domain_contains_allowlist \
    						 (check 3/3)",
                            &root_domain
                        );
                        return true;
                    }
                }
            }
        }
    }

    false
}
