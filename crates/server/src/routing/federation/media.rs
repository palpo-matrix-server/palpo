use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::str::FromStr;

use diesel::prelude::*;
use image::imageops::FilterType;
use mime::Mime;
use salvo::fs::NamedFile;
use salvo::http::{HeaderValue, ResBody};
use salvo::prelude::*;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::core::federation::media::*;
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::media::*;
use crate::schema::*;
use crate::{db, hoops, AppResult, AuthArgs, MatrixError};

pub fn router() -> Router {
    Router::with_path("media")
        .hoop(hoops::limit_rate)
        .push(Router::with_path("download/{media_id}").get(get_content))
        .push(Router::with_path("thumbnail/{media_id}").get(get_thumbnail))
}

/// #GET /_matrix/media/r0/download/{server_name}/{media_id}
/// Load media from our server or over federation.
///
/// - Only allows federation if `allow_remote` is true
#[endpoint]
pub async fn get_content(args: ContentReqArgs, req: &mut Request, res: &mut Response) -> AppResult<()> {
    let server_name = &crate::config().server_name;
    if let Some(metadata) = crate::media::get_metadata(server_name, &args.media_id)? {
        let content_type = metadata
            .content_type
            .as_deref()
            .map(|c| Mime::from_str(c).ok())
            .flatten()
            .unwrap_or_else(|| {
                metadata
                    .upload_name
                    .as_ref()
                    .map(|name| mime_infer::infer_mime_type(name))
                    .unwrap_or(mime::APPLICATION_OCTET_STREAM)
            });

        let path = crate::media_path(server_name, &args.media_id);
        if Path::new(&path).exists() {
            NamedFile::builder(path)
                .content_type(content_type)
                .send(req.headers(), res)
                .await;
            Ok(())
        } else {
            Err(MatrixError::not_yet_uploaded("Media has not been uploaded yet").into())
        }
    } else {
        Err(MatrixError::not_yet_uploaded("Media has not been uploaded yet").into())
    }
}

/// # `GET /_matrix/federation/v1/media/thumbnail/{serverName}/{mediaId}`
#[endpoint]
pub async fn get_thumbnail(
    _aa: AuthArgs,
    args: ThumbnailReqArgs,
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
) -> AppResult<()> {
    let server_name = &crate::config().server_name;
    let tbs = media_thumbnails::table.load::<DbThumbnail>(&mut *db::connect()?)?;

    if let Some(DbThumbnail {
        content_disposition,
        content_type,
        ..
    }) = crate::media::get_thumbnail(server_name, &args.media_id, args.width, args.height)?
    {
        let thumb_path = crate::media_path(
            server_name,
            &format!("{}.{}x{}", args.media_id, args.width, args.height),
        );

        res.add_header("Cross-Origin-Resource-Policy", "cross-origin", true)?;
        let mut file = NamedFile::builder(&thumb_path)
            .content_type(
                Mime::from_str(&content_type)
                    .ok()
                    .unwrap_or(mime::APPLICATION_OCTET_STREAM),
            )
            .build()
            .await?;
        if let Some(Ok(content_disposition)) = content_disposition.as_deref().map(HeaderValue::from_str) {
            file.set_content_disposition(content_disposition);
        }

        return Ok(());
    }

    let (width, height, crop) = crate::media::thumbnail_properties(args.width, args.height).unwrap_or((0, 0, false)); // 0, 0 because that's the original file

    let thumb_path = crate::media_path(server_name, &format!("{}.{width}x{height}", &args.media_id));
    if let Some(DbThumbnail {
        media_id,
        width,
        height,
        content_disposition,
        content_type,
        ..
    }) = crate::media::get_thumbnail(server_name, &args.media_id, width, height)?
    {
        // Using saved thumbnail
        let mut file = NamedFile::builder(&thumb_path)
            .content_type(
                Mime::from_str(&content_type)
                    .ok()
                    .unwrap_or(mime::APPLICATION_OCTET_STREAM),
            )
            .build()
            .await?;
        if let Some(Ok(content_disposition)) = content_disposition.as_deref().map(HeaderValue::from_str) {
            file.set_content_disposition(content_disposition);
        }
        file.send(req.headers(), res).await;

        Ok(())
    } else if let Ok(Some(DbMetadata {
        content_disposition,
        content_type,
        media_id,
        ..
    })) = crate::media::get_metadata(server_name, &args.media_id)
    {
        // Generate a thumbnail
        let image_path = crate::media_path(server_name, &args.media_id);
        if let Ok(image) = image::open(&image_path) {
            let original_width = image.width();
            let original_height = image.height();
            if width > original_width || height > original_height {
                let mut file = NamedFile::builder(&thumb_path)
                    .content_type(
                        content_type
                            .as_deref()
                            .map(|c| Mime::from_str(c).ok())
                            .flatten()
                            .unwrap_or(mime::APPLICATION_OCTET_STREAM),
                    )
                    .build()
                    .await?;
                if let Some(Ok(content_disposition)) = content_disposition.as_deref().map(HeaderValue::from_str) {
                    file.set_content_disposition(content_disposition);
                }
                file.send(req.headers(), res).await;
                return Ok(());
            }

            let thumbnail = if crop {
                image.resize_to_fill(width, height, FilterType::CatmullRom)
            } else {
                let (exact_width, exact_height) = {
                    // Copied from image::dynimage::resize_dimensions
                    let ratio = u64::from(original_width) * u64::from(height);
                    let nratio = u64::from(width) * u64::from(original_height);

                    let use_width = nratio <= ratio;
                    let intermediate = if use_width {
                        u64::from(original_height) * u64::from(width) / u64::from(original_width)
                    } else {
                        u64::from(original_width) * u64::from(height) / u64::from(original_height)
                    };
                    if use_width {
                        if intermediate <= u64::from(::std::u32::MAX) {
                            (width, intermediate as u32)
                        } else {
                            (
                                (u64::from(width) * u64::from(::std::u32::MAX) / intermediate) as u32,
                                ::std::u32::MAX,
                            )
                        }
                    } else if intermediate <= u64::from(::std::u32::MAX) {
                        (intermediate as u32, height)
                    } else {
                        (
                            ::std::u32::MAX,
                            (u64::from(height) * u64::from(::std::u32::MAX) / intermediate) as u32,
                        )
                    }
                };

                image.thumbnail_exact(exact_width, exact_height)
            };

            let mut thumbnail_bytes = Vec::new();
            thumbnail.write_to(&mut Cursor::new(&mut thumbnail_bytes), image::ImageFormat::Png)?;

            // Save thumbnail in database so we don't have to generate it again next time
            diesel::insert_into(media_thumbnails::table)
                .values(&NewDbThumbnail {
                    media_id: args.media_id.clone(),
                    origin_server: server_name.clone(),
                    content_type: "mage/png".into(),
                    content_disposition: None,
                    file_size: thumbnail_bytes.len() as i64,
                    width: width as i32,
                    height: height as i32,
                    resize_method: "_".into(),
                    created_at: UnixMillis::now(),
                })
                .execute(&mut *db::connect()?)?;
            let mut f = File::create(&thumb_path).await?;
            f.write_all(&thumbnail_bytes).await?;

            let mut file = NamedFile::builder(&thumb_path)
                .content_type(
                    content_type
                        .as_deref()
                        .map(|c| Mime::from_str(c).ok())
                        .flatten()
                        .unwrap_or(mime::APPLICATION_OCTET_STREAM),
                )
                .build()
                .await?;
            if let Some(Ok(content_disposition)) = content_disposition.as_deref().map(HeaderValue::from_str) {
                file.set_content_disposition(content_disposition);
            }
            file.send(req.headers(), res).await;
            Ok(())
        } else {
            // Couldn't parse file to generate thumbnail, send original
            let mut file = NamedFile::builder(&image_path)
                .content_type(
                    content_type
                        .as_deref()
                        .map(|c| Mime::from_str(c).ok())
                        .flatten()
                        .unwrap_or(mime::APPLICATION_OCTET_STREAM),
                )
                .build()
                .await?;
            if let Some(Ok(content_disposition)) = content_disposition.as_deref().map(HeaderValue::from_str) {
                file.set_content_disposition(content_disposition);
            }
            file.send(req.headers(), res).await;
            Ok(())
        }
    } else {
        Err(MatrixError::not_found("file not found").into())
    }
}
