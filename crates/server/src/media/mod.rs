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
