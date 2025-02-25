use diesel::prelude::*;

use crate::core::UnixMillis;
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{AppResult, db};

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

pub fn get_metadata(server_name: &ServerName, media_id: &str) -> AppResult<Option<DbMetadata>> {
    media_metadatas::table
        .filter(media_metadatas::media_id.eq(media_id))
        .filter(media_metadatas::origin_server.eq(server_name))
        .first::<DbMetadata>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}
