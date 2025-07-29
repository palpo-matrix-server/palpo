mod preview;
mod remote;
pub use remote::*;

use std::cmp;
use std::num::Saturating;
use std::time::Duration;use std::time::SystemTime;

pub use preview::*;
use salvo::Response;

use crate::core::OwnedMxcUri;
use crate::core::federation::media::ContentReqArgs;
use crate::core::media::Method;
use crate::core::http_headers::ContentDisposition;
use crate::core::{ServerName, media};
use crate::{AppResult, exts::*, join_path};

#[derive(Debug)]
pub struct FileMeta {
	pub content: Option<Vec<u8>>,
	pub content_type: Option<String>,
	pub content_disposition: Option<ContentDisposition>,
}

/// Dimension specification for a thumbnail.
#[derive(Debug)]
pub struct Dimension {
    pub width: u32,
    pub height: u32,
    pub method: Method,
}

impl Dimension {
    /// Instantiate a Dim with optional method
    #[inline]
    #[must_use]
    pub fn new(width: u32, height: u32, method: Option<Method>) -> Self {
        Self {
            width,
            height,
            method: method.unwrap_or(Method::Scale),
        }
    }

    pub fn scaled(&self, image: &Self) -> AppResult<Self> {
        let image_width = image.width;
        let image_height = image.height;

        let width = cmp::min(self.width, image_width);
        let height = cmp::min(self.height, image_height);

        let use_width = Saturating(width) * Saturating(image_height) < Saturating(height) * Saturating(image_width);

        let x = if use_width {
            let dividend = (Saturating(height) * Saturating(image_width)).0;
            dividend / image_height
        } else {
            width
        };

        let y = if !use_width {
            let dividend = (Saturating(width) * Saturating(image_height)).0;
            dividend / image_width
        } else {
            height
        };

        Ok(Self {
            width: x,
            height: y,
            method: Method::Scale,
        })
    }

    /// Returns width, height of the thumbnail and whether it should be cropped.
    /// Returns None when the server should send the original file.
    /// Ignores the input Method.
    #[must_use]
    pub fn normalized(&self) -> Self {
        match (self.width, self.height) {
            (0..=32, 0..=32) => Self::new(32, 32, Some(Method::Crop)),
            (0..=96, 0..=96) => Self::new(96, 96, Some(Method::Crop)),
            (0..=320, 0..=240) => Self::new(320, 240, Some(Method::Scale)),
            (0..=640, 0..=480) => Self::new(640, 480, Some(Method::Scale)),
            (0..=800, 0..=600) => Self::new(800, 600, Some(Method::Scale)),
            _ => Self::default(),
        }
    }

    /// Returns true if the method is Crop.
    #[inline]
    #[must_use]
    pub fn crop(&self) -> bool {
        self.method == Method::Crop
    }
}

impl Default for Dimension {
    #[inline]
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            method: Method::Scale,
        }
    }
}

pub async fn get_remote_content(
    _mxc: &str,
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
    join_path!(&crate::config::get().space_path, "media", key)
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

pub fn get_all_mxcs() -> AppResult<Vec<OwnedMxcUri>> {
    unimplemented!()
}

pub async fn delete_all_remote_media_at_after_time(
    time: SystemTime,
    before: bool,
    after: bool,
    yes_i_want_to_delete_local_media: bool,
) -> AppResult<u64> {
    unimplemented!()
}
