mod metadata;
mod thumbnail;

pub use metadata::*;
pub use thumbnail::*;

use std::time::Duration;

use salvo::Response;

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
