/// Endpoints for the media repository.
use std::time::Duration;

use salvo::oapi::{ToParameters, ToSchema};
use serde::{Deserialize, Serialize};

use crate::http_headers::ContentDisposition;
use crate::media::Method;
use crate::sending::{SendRequest, SendResult};
use crate::serde::StringEnum;
use crate::{OwnedMxcUri, OwnedServerName, PrivOwnedStr, ServerName, UnixMillis};

/// The `multipart/mixed` mime "essence".
const MULTIPART_MIXED: &str = "multipart/mixed";
/// The maximum number of headers to parse in a body part.
const MAX_HEADERS_COUNT: usize = 32;
/// The length of the generated boundary.
const GENERATED_BOUNDARY_LENGTH: usize = 30;

/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1mediathumbnailmediaid
pub fn thumbnail_request(server: &ServerName, args: ThumbnailReqArgs) -> SendResult<SendRequest> {
    let mut url = server.build_url(&format!("/federation/v1/media/thumbnail/{}", args.media_id))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("width", &args.width.to_string());
        query.append_pair("height", &args.height.to_string());
        query.append_pair("timeout_ms", &args.timeout_ms.as_millis().to_string());
    }
    Ok(crate::sending::get(url))
}

/// Request type for the `get_content_thumbnail` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct ThumbnailReqArgs {
    /// The media ID from the mxc:// URI (the path component).
    #[salvo(parameter(parameter_in = Path))]
    pub media_id: String,

    /// The desired resizing method.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,

    /// The *desired* width of the thumbnail.
    ///
    /// The actual thumbnail may not match the size specified.
    #[salvo(parameter(parameter_in = Query))]
    pub width: u32,

    /// The *desired* height of the thumbnail.
    ///
    /// The actual thumbnail may not match the size specified.
    #[salvo(parameter(parameter_in = Query))]
    pub height: u32,

    /// The maximum duration that the client is willing to wait to start receiving data, in the
    /// case that the content has not yet been uploaded.
    ///
    /// The default value is 20 seconds.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(
        with = "crate::serde::duration::ms",
        default = "crate::client::media::default_download_timeout",
        skip_serializing_if = "crate::media::is_default_download_timeout"
    )]
    pub timeout_ms: Duration,

    /// Whether the server should return an animated thumbnail.
    ///
    /// When `Some(true)`, the server should return an animated thumbnail if possible and
    /// supported. When `Some(false)`, the server must not return an animated
    /// thumbnail. When `None`, the server should not return an animated thumbnail.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animated: Option<bool>,
}

/// Response type for the `get_content_thumbnail` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct ThumbnailResBody {
    /// The metadata of the thumbnail.
    pub metadata: ContentMetadata,

    /// The content of the thumbnail.
    pub content: FileOrLocation,
}

/// `/v1/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/server-server-api/#get_matrixfederationv1mediadownloadmediaid
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/_matrix/media/r0/download/:server_name/:media_id",
//         1.1 => "/_matrix/media/v3/download/:server_name/:media_id",
//     }
// };

pub fn content_request(server: &ServerName, args: ContentReqArgs) -> SendResult<SendRequest> {
    let url = server.build_url(&format!(
        "federation/v1/media/download/{}?timeout_ms={}",
        args.media_id,
        args.timeout_ms.as_millis()
    ))?;
    Ok(crate::sending::get(url))
}

/// Request type for the `get_media_content` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct ContentReqArgs {
    /// The media ID from the mxc:// URI (the path component).
    #[salvo(parameter(parameter_in = Path))]
    pub media_id: String,

    /// The maximum duration that the client is willing to wait to start receiving data, in the
    /// case that the content has not yet been uploaded.
    ///
    /// The default value is 20 seconds.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(
        with = "crate::serde::duration::ms",
        default = "crate::client::media::default_download_timeout",
        skip_serializing_if = "crate::client::media::is_default_download_timeout"
    )]
    pub timeout_ms: Duration,
}

/// Response type for the `get_content` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct ContentResBody {
    /// The metadata of the media.
    pub metadata: ContentMetadata,

    /// The content of the media.
    pub content: FileOrLocation,
}

/// A file from the content repository or the location where it can be found.
#[derive(ToSchema, Serialize, Debug, Clone)]
pub enum FileOrLocation {
    /// The content of the file.
    File(Content),

    /// The file is at the given URL.
    Location(String),
}

/// The content of a file from the content repository.
#[derive(ToSchema, Serialize, Debug, Clone)]
pub struct Content {
    /// The content of the file as bytes.
    pub file: Vec<u8>,

    /// The content type of the file that was previously uploaded.
    pub content_type: Option<String>,

    /// The value of the `Content-Disposition` HTTP header, possibly containing the name of the
    /// file that was previously uploaded.
    pub content_disposition: Option<ContentDisposition>,
}
/// The metadata of a file from the content repository.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone, Default)]
pub struct ContentMetadata {}

impl ContentMetadata {
    /// Creates a new empty `ContentMetadata`.
    pub fn new() -> Self {
        Self {}
    }
}
