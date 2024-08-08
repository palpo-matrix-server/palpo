use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = media_thumbnails)]
pub struct DbThumbnail {
    pub id: i64,
    pub media_id: String,
    pub media_origin: OwnedServerName,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
    pub resize_method: String,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = media_thumbnails)]
pub struct NewDbThumbnail {
    pub media_id: String,
    pub media_origin: OwnedServerName,
    pub media_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
    pub resize_method: String,
    pub created_at: UnixMillis,
}

pub fn get_thumbnail(media_id: &str, origin: &ServerName) -> AppResult<DbThumbnail> {
    media_thumbnails::table
        .filter(media_thumbnails::media_id.eq(media_id))
        .filter(media_thumbnails::media_origin.eq(origin))
        .first::<DbThumbnail>(&mut *db::connect()?)
        .map_err(Into::into)
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
