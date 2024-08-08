mod metadata;
mod thumbnail;

pub use metadata::*;

use salvo::Response;

use crate::core::ServerName;
use crate::{join_path, AppResult};

pub async fn get_remote_content(
    mxc: &str,
    server_name: &ServerName,
    media_id: &str,
    res: &mut Response,
) -> AppResult<()> {
    let content_response = crate::sending::get(server_name.build_url(&format!("media/{media_id}"))?)
        .exec()
        .await?;

    res.status_code(content_response.status());
    res.stream(content_response.bytes_stream());

    // crate::media::create_media(
    //     mxc.to_owned(),
    //     content_response.content_disposition.as_deref(),
    //     content_response.content_type.as_deref(),
    //     &content_response.file,
    // )
    // .await?;

    Ok(())
}

fn get_media_path(key: &str) -> String {
    join_path!(&crate::config().space_path, "media", key)
}
