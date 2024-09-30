use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use diesel::prelude::*;
use image::imageops::FilterType;
use mime::Mime;
use salvo::fs::NamedFile;
use salvo::http::{HeaderValue, ResBody};
use salvo::prelude::*;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::client::media::*;
use crate::core::client::media::*;
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::schema::*;
use crate::{db, empty_ok, hoops, json_ok, utils, AppResult, AuthArgs, EmptyResult, JsonResult, MatrixError};

pub fn router() -> Router {
    let mut media = Router::with_path("media").oapi_tag("media");
    for v in ["v3", "v1", "r0"] {
        media = media
            .push(
                Router::with_path(v)
                    .hoop(hoops::auth_by_access_token)
                    .push(Router::with_path("create").post(create_mxc_uri))
                    .push(
                        Router::with_path("upload")
                            .post(create_content)
                            .push(Router::with_path("<server_name>/<media_id>").put(upload_content)),
                    )
                    .push(
                        Router::with_hoop(hoops::limit_rate)
                            .push(Router::with_path("config").get(get_config))
                            .push(Router::with_path("preview_url").get(preview_url))
                            .push(Router::with_path("thumbnail/<server_name>/<media_id>").get(get_thumbnail)),
                    ),
            )
            .push(
                Router::with_path(v).push(
                    Router::with_path("download/<server_name>/<media_id>")
                        .get(get_content)
                        .push(Router::with_path("<filename>").get(get_content_with_filename)),
                ),
            )
    }
    media
}
