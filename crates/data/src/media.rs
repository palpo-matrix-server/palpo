use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;
use crate::{connect, DataResult};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = media_metadatas)]
pub struct DbMetadata {
    pub id: i64,
    pub media_id: String,
    pub origin_server: OwnedServerName,
    pub content_type: Option<String>,
    pub disposition_type: Option<String>,
    pub file_name: Option<String>,
    pub file_extension: Option<String>,
    pub file_size: i64,
    pub file_hash: Option<String>,
    pub created_by: Option<OwnedUserId>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = media_metadatas)]
pub struct NewDbMetadata {
    pub media_id: String,
    pub origin_server: OwnedServerName,
    pub content_type: Option<String>,
    pub disposition_type: Option<String>,
    pub file_name: Option<String>,
    pub file_extension: Option<String>,
    pub file_size: i64,
    pub file_hash: Option<String>,
    pub created_by: Option<OwnedUserId>,
    pub created_at: UnixMillis,
}

pub fn get_metadata(server_name: &ServerName, media_id: &str) -> DataResult<Option<DbMetadata>> {
    media_metadatas::table
        .filter(media_metadatas::media_id.eq(media_id))
        .filter(media_metadatas::origin_server.eq(server_name))
        .first::<DbMetadata>(&mut *connect()?)
        .optional()
        .map_err(Into::into)
}

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = media_thumbnails)]
pub struct DbThumbnail {
    pub id: i64,
    pub media_id: String,
    pub origin_server: OwnedServerName,
    pub content_type: String,
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
    pub origin_server: OwnedServerName,
    pub content_type: String,
    pub file_size: i64,
    pub width: i32,
    pub height: i32,
    pub resize_method: String,
    pub created_at: UnixMillis,
}

pub fn get_thumbnail(
    origin_server: &ServerName,
    media_id: &str,
    width: u32,
    height: u32,
) -> DataResult<Option<DbThumbnail>> {
    media_thumbnails::table
        .filter(media_thumbnails::origin_server.eq(origin_server))
        .filter(media_thumbnails::media_id.eq(media_id))
        .filter(media_thumbnails::width.eq(width as i32))
        .filter(media_thumbnails::height.eq(height as i32))
        .first::<DbThumbnail>(&mut *connect()?)
        .optional()
        .map_err(Into::into)
}
