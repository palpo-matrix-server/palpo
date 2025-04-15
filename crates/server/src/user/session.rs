use diesel::prelude::*;
use serde_json::Value;

use crate::core::UnixMillis;
use crate::core::identifiers::*;
use crate::data::schema::*;

#[derive(Insertable, Identifiable, Debug, Clone)]
#[diesel(table_name = user_sessions)]
pub struct DbSession {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub session_id: String,
    pub value: Value,
    pub expired_at: i64,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_sessions)]
pub struct NewDbSession {
    pub user_id: OwnedUserId,
    pub session_id: String,
    pub value: Value,
    pub expired_at: i64,
    pub created_at: UnixMillis,
}
