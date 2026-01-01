//! `GET /_matrix/client/*/media/thumbnail/{serverName}/{mediaId}`
//!
//! Get a thumbnail of content from the media store.
//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv1mediathumbnailservernamemediaid

use std::time::Duration;

use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use crate::{
    IdParseError, MxcUri, OwnedServerName,
    api::{auth_scheme::AccessToken, request, response},
    http_headers::ContentDisposition,
    media::Method,
    metadata,
};

// metadata! {
//     method: GET,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         unstable("org.matrix.msc3916") => "/_matrix/client/unstable/org.matrix.msc3916/media/thumbnail/{server_name}/{media_id}",
//         1.11 | stable("org.matrix.msc3916.stable") => "/_matrix/client/v1/media/thumbnail/{server_name}/{media_id}",
//     }
// }

/// Request type for the `get_content_thumbnail` endpoint.
#[derive(ToParameters, Deserialize, Debug)]
pub struct GetMediaThumbnailArgs {
    /// The server name from the mxc:// URI (the authoritory component).
    #[salvo(parameter(parameter_in = Path))]
    pub server_name: OwnedServerName,

    /// The media ID from the mxc:// URI (the path component).
    #[salvo(parameter(parameter_in = Path))]
    pub media_id: String,

    /// The desired resizing method.
    #[salvo(parameter(parameter_in = Query))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,

    /// The *desired* width of the thumbnail.
    ///
    /// The actual thumbnail may not match the size specified.
    #[palpo_api(query)]
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
        default = "crate::media::default_download_timeout",
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

// /// Response type for the `get_content_thumbnail` endpoint.
// #[response(error = crate::Error)]
// pub struct GetMediaThumbnailResBody {
//     /// A thumbnail of the requested content.
//     #[palpo_api(raw_body)]
//     pub file: Vec<u8>,

//     /// The content type of the thumbnail.
//     #[palpo_api(header = CONTENT_TYPE)]
//     pub content_type: Option<String>,

//     /// The value of the `Content-Disposition` HTTP header, possibly containing the name of the
//     /// file that was previously uploaded.
//     ///
//     /// See [MDN] for the syntax.
//     ///
//     /// [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Disposition#Syntax
//     #[palpo_api(header = CONTENT_DISPOSITION)]
//     pub content_disposition: Option<ContentDisposition>,
// }

impl GetMediaThumbnailArgs {
    /// Creates a new `GetMediaThumbnailArgs` with the given media ID, server name, desired thumbnail width
    /// and desired thumbnail height.
    pub fn new(media_id: String, server_name: OwnedServerName, width: u32, height: u32) -> Self {
        Self {
            media_id,
            server_name,
            method: None,
            width,
            height,
            timeout_ms: crate::media::default_download_timeout(),
            animated: None,
        }
    }

    /// Creates a new `Request` with the given URI, desired thumbnail width and
    /// desired thumbnail height.
    pub fn from_uri(uri: &MxcUri, width: UInt, height: UInt) -> Result<Self, IdParseError> {
        let (server_name, media_id) = uri.parts()?;

        Ok(Self::new(
            media_id.to_owned(),
            server_name.to_owned(),
            width,
            height,
        ))
    }
}

// impl Response {
//     /// Creates a new `Response` with the given thumbnail.
//     pub fn new(
//         file: Vec<u8>,
//         content_type: String,
//         content_disposition: ContentDisposition,
//     ) -> Self {
//         Self {
//             file,
//             content_type: Some(content_type),
//             content_disposition: Some(content_disposition),
//         }
//     }
// }
