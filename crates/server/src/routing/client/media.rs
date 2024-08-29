use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use diesel::prelude::*;
use salvo::fs::NamedFile;
use salvo::prelude::*;

use crate::core::client::media::*;
use crate::core::{OwnedMxcUri, UnixMillis};
use crate::media::*;
use crate::schema::*;
use crate::{
    db, empty_ok, hoops, json_ok, utils, AppError, AppResult, AuthArgs, DepotExt, EmptyResult, JsonResult, MatrixError,
};

pub fn router() -> Router {
    let mut media = Router::with_path("media").oapi_tag("client");
    for v in ["v3", "r0"] {
        media = media.push(
            Router::with_path(v)
                .push(
                    Router::with_path("download/<server_name>/<media_id>")
                        .get(get_content)
                        .push(Router::with_path("<filename>").get(get_content_with_filename)),
                )
                .push(
                    Router::with_hoop(hoops::limit_rate)
                        .push(Router::with_path("create").post(create_mxc_uri))
                        .push(
                            Router::with_path("upload").post(upload), // .push(Router::with_path("<server_name>/<media_id>").put(upload_media)),
                        )
                        .push(Router::with_path("config").get(get_config))
                        .push(Router::with_path("preview_url").get(preview_url))
                        .push(Router::with_path("thumbnail/<server_name>/<media_id>").get(thumbnail)),
                ),
        )
    }
    media
}

// #GET /_matrix/media/r0/download/{server_name}/{media_id}
/// Load media from our server or over federation.
///
/// - Only allows federation if `allow_remote` is true
#[endpoint]
async fn get_content(_aa: AuthArgs, args: ContentReqArgs, req: &mut Request, res: &mut Response) -> AppResult<()> {
    let metadata = crate::media::get_metadata(&args.server_name, &args.media_id)?;
    let path = crate::media_path(&args.server_name, &args.media_id, metadata.file_extension.as_deref());
    if Path::new(&path).exists() {
        NamedFile::builder(path)
            .content_type(
                metadata
                    .content_type
                    .parse()
                    .map_err(|_| AppError::public("invalid content type."))?,
            )
            .send(req.headers(), res)
            .await;

        Ok(())
    } else if &*args.server_name != crate::server_name() && args.allow_remote {
        let mxc = format!("mxc://{}/{}", args.server_name, args.media_id);
        get_remote_content(&mxc, &args.server_name, &args.media_id, res).await
    } else {
        Err(MatrixError::not_found("Media not found.").into())
    }
}

// #GET /_matrix/media/r0/download/{server_name}/{media_id}/{file_name}
/// Load media from our server or over federation, permitting desired filename.
///
/// - Only allows federation if `allow_remote` is true
#[endpoint]
async fn get_content_with_filename(
    _aa: AuthArgs,
    args: ContentWithFileNameReqArgs,
    req: &mut Request,
    res: &mut Response,
) -> AppResult<()> {
    let metadata = crate::media::get_metadata(&args.server_name, &args.media_id)?;

    let path = crate::media_path(&args.server_name, &args.media_id, metadata.file_extension.as_deref());
    if Path::new(&path).exists() {
        NamedFile::builder(path)
            .content_type(
                metadata
                    .content_type
                    .parse()
                    .map_err(|_| AppError::public("invalid content type."))?,
            )
            .attached_name(args.filename)
            .send(req.headers(), res)
            .await;

        Ok(())
    } else if &*args.server_name != crate::server_name() && args.allow_remote {
        let mxc = format!("mxc://{}/{}", args.server_name, args.media_id);
        get_remote_content(&mxc, &args.server_name, &args.media_id, res).await
    } else {
        Err(MatrixError::not_found("Media not found.").into())
    }
}
#[endpoint]
fn create_mxc_uri(_aa: AuthArgs) -> JsonResult<CreateMxcUriResBody> {
    let media_id = utils::random_string(crate::MXC_LENGTH);
    let mxc = format!("mxc://{}/{}", crate::server_name(), media_id);
    // TODO: ?
    // diesel::insert_into(media_contents::table)
    //     .values(&NewMediaContent {
    //         media_id: &media_id,
    //         created_ts: chrono::Utc::now().timestamp_millis() as i64,
    //     })
    //     .execute(&mut db::connect()?)?;
    Ok(Json(CreateMxcUriResBody {
        content_uri: OwnedMxcUri::from(mxc),
        unused_expires_at: None,
    }))
}

// #POST /_matrix/media/r0/upload
/// Permanently save media in the server.
///
/// - Some metadata will be saved in the database
/// - Media will be saved in the media/ directory
#[endpoint]
async fn upload(
    _aa: AuthArgs,
    args: UploadContentReqArgs,
    req: &mut Request,
    depot: &mut Depot,
) -> JsonResult<UploadContentResBody> {
    // let authed = depot.take_authed_info()?;

    let upload_name = args.filename.clone().unwrap_or_default().to_owned();
    let file_extension = utils::fs::get_file_ext(&upload_name);

    let payload = req
        .payload_with_max_size(crate::max_request_size() as usize)
        .await
        .unwrap();
    let checksum = utils::hash::hash_data_sha2_256(payload)?;
    let media_id = checksum.to_base32_crockford();
    let mxc = format!("mxc://{}/{}", crate::config().server_name, media_id);

    let conf = crate::config();

    let dest_path = crate::media_path(&conf.server_name, &media_id, Some(&*file_extension));
    let metadata = NewDbMetadata {
        media_id,
        origin_server: conf.server_name.clone(),
        content_type: args.content_type.clone().unwrap_or_default(),
        upload_name,
        file_extension: if file_extension.is_empty() {
            None
        } else {
            Some(file_extension)
        },
        file_size: payload.len() as i64,
        hash: checksum.to_hex_uppercase(),
        created_by: None,
        created_at: UnixMillis::now(),
    };

    diesel::insert_into(media_metadatas::table)
        .values(&metadata)
        .execute(&mut *db::connect()?)?;

    let dest_path = Path::new(&dest_path);
    if dest_path.exists() {
        let metadata = fs::metadata(dest_path)?;
        if metadata.len() != payload.len() as u64 {
            if let Err(e) = fs::remove_file(dest_path) {
                tracing::error!(error = ?e, "remove media file failed");
            }
        }
    }
    if !dest_path.exists() {
        let parent_dir = utils::fs::get_parent_dir(&dest_path);
        fs::create_dir_all(&parent_dir)?;

        let mut file = File::create(dest_path)?;
        file.write_all(&payload)?;

        //TODO: thumbnail support
    }

    json_ok(UploadContentResBody {
        content_uri: mxc.try_into().unwrap(),
        blurhash: None,
    })
}

// #[endpoint]
// async fn upload_media(_aa: AuthArgs, depot: &mut Depot) -> JsonResult<UploadContentResBody> {
//     let mxc = format!(
//         "mxc://{}/{}",
//         &crate::config().server_name,
//         utils::random_string(MXC_LENGTH)
//     );

//     // crate::room::create(
//     //     mxc.clone(),
//     //     body.filename
//     //         .as_ref()
//     //         .map(|filename| "inline; filename=".to_owned() + filename)
//     //         .as_deref(),
//     //     body.content_type.as_deref(),
//     //     &body.file,
//     // )
//     // .await?;

//     // json_ok(UploadContentResBody {
//     //     content_uri: mxc.try_into().expect("Invalid mxc:// URI"),
//     //     blurhash: None,
//     // })
// }

// #GET /_matrix/media/r0/config
/// Returns max upload size.
#[endpoint]
async fn get_config(_aa: AuthArgs) -> JsonResult<ConfigResBody> {
    json_ok(ConfigResBody {
        upload_size: crate::max_request_size().into(),
    })
}

#[endpoint]
async fn preview_url(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}

/// #GET /_matrix/media/r0/thumbnail/{server_name}/{media_id}
/// Load media thumbnail from our server or over federation.
///
/// - Only allows federation if `allow_remote` is true
///
///

/// Downloads a file's thumbnail.
///
/// Here's an example on how it works:
///
/// - Client requests an image with width=567, height=567
/// - Server rounds that up to (800, 600), so it doesn't have to save too many thumbnails
/// - Server rounds that up again to (958, 600) to fix the aspect ratio (only for width,height>96)
/// - Server creates the thumbnail and sends it to the user
///
/// For width,height <= 96 the server uses another thumbnailing algorithm which crops the image afterwards.
#[endpoint]
async fn thumbnail(_aa: AuthArgs, args: ThumbnailReqArgs, depot: &mut Depot) -> EmptyResult {
    // let mxc = format!("mxc://{}/{}", body.server_name, body.media_id);

    // if let Some(FileMeta { content_type, file, .. }) = crate::media::thumbnail::get_thumbnail(
    //     mxc.clone(),
    //     body.width.try_into().map_err(|_| MatrixError::invalid_param("Width is invalid."))?,
    //     body.height.try_into().map_err(|_| MatrixError::invalid_param("Width is invalid."))?,
    // )
    // .await?
    // {
    //     json_ok(ThumbnailResBody {
    //         file,
    //         content_type,
    //         cross_origin_resource_policy: Some("cross-origin".to_owned()),
    //     })
    // } else if &*body.server_name != &crate::config().server_name && body.allow_remote {
    //     let get_thumbnail_response = crate::sending::send_federation_request(
    //         &body.server_name,
    //         ThumbnailReqArgs {
    //             allow_remote: false,
    //             height: body.height,
    //             width: body.width,
    //             method: body.method.clone(),
    //             server_name: body.server_name.clone(),
    //             media_id: body.media_id.clone(),
    //             timeout_ms: Duration::from_secs(20),
    //             allow_redirect: false,
    //         },
    //     )
    //     .await?;

    //     crate::media::thumbnail::upload_thumbnail(
    //         mxc,
    //         None,
    //         get_thumbnail_response.content_type.as_deref(),
    //         body.width.try_into().expect("all UInts are valid u32s"),
    //         body.height.try_into().expect("all UInts are valid u32s"),
    //         &get_thumbnail_response.file,
    //     )
    //     .await?;

    //     json_ok(get_thumbnail_response)
    // } else {
    //     Err(MatrixError::not_found("Media not found."))
    // }

    // let (width, height, crop) = thumbnail_properties(width, height).unwrap_or((0, 0, false)); // 0, 0 because that's the original file

    // if let Ok((content_disposition, content_type, key)) = db::search_file_metadata(mxc.clone(), width, height) {
    //     // Using saved thumbnail
    //     let path = crate::media_path(&key);
    //     let mut file = Vec::new();
    //     File::open(path).await?.read_to_end(&mut file).await?;

    //     Ok(Some(FileMeta {
    //         content_disposition,
    //         content_type,
    //         file: file.to_vec(),
    //     }))
    // } else if let Ok((content_disposition, content_type, key)) = db::search_file_metadata(mxc.clone(), 0, 0) {
    //     // Generate a thumbnail
    //     let path = crate::media_path(&key);
    //     let mut file = Vec::new();
    //     File::open(path).await?.read_to_end(&mut file).await?;

    //     if let Ok(image) = image::load_from_memory(&file) {
    //         let original_width = image.width();
    //         let original_height = image.height();
    //         if width > original_width || height > original_height {
    //             return Ok(Some(FileMeta {
    //                 content_disposition,
    //                 content_type,
    //                 file: file.to_vec(),
    //             }));
    //         }

    //         let thumbnail = if crop {
    //             image.resize_to_fill(width, height, FilterType::CatmullRom)
    //         } else {
    //             let (exact_width, exact_height) = {
    //                 // Copied from image::dynimage::resize_dimensions
    //                 let ratio = u64::from(original_width) * u64::from(height);
    //                 let nratio = u64::from(width) * u64::from(original_height);

    //                 let use_width = nratio <= ratio;
    //                 let intermediate = if use_width {
    //                     u64::from(original_height) * u64::from(width) / u64::from(original_width)
    //                 } else {
    //                     u64::from(original_width) * u64::from(height) / u64::from(original_height)
    //                 };
    //                 if use_width {
    //                     if intermediate <= u64::from(::std::u32::MAX) {
    //                         (width, intermediate as u32)
    //                     } else {
    //                         ((u64::from(width) * u64::from(::std::u32::MAX) / intermediate) as u32, ::std::u32::MAX)
    //                     }
    //                 } else if intermediate <= u64::from(::std::u32::MAX) {
    //                     (intermediate as u32, height)
    //                 } else {
    //                     (::std::u32::MAX, (u64::from(height) * u64::from(::std::u32::MAX) / intermediate) as u32)
    //                 }
    //             };

    //             image.thumbnail_exact(exact_width, exact_height)
    //         };

    //         let mut thumbnail_bytes = Vec::new();
    //         thumbnail.write_to(&mut Cursor::new(&mut thumbnail_bytes), image::ImageOutputFormat::Png)?;

    //         // Save thumbnail in database so we don't have to generate it again next time
    //         let thumbnail_key = db::create_file_metadata(mxc, width, height, content_disposition.as_deref(), content_type.as_deref())?;

    //         let path = crate::media_path(&thumbnail_key);
    //         let mut f = File::create(path).await?;
    //         f.write_all(&thumbnail_bytes).await?;

    //         Ok(Some(FileMeta {
    //             content_disposition,
    //             content_type,
    //             file: thumbnail_bytes.to_vec(),
    //         }))
    //     } else {
    //         // Couldn't parse file to generate thumbnail, send original
    //         Ok(Some(FileMeta {
    //             content_disposition,
    //             content_type,
    //             file: file.to_vec(),
    //         }))
    //     }
    // } else {
    //     Ok(None)
    // }

    empty_ok()
}
