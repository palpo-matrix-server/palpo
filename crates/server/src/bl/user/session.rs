use diesel::prelude::*;
use salvo::http::headers::{
    authorization::{Authorization, Bearer, Credentials},
    HeaderMapExt,
};
use salvo::oapi::ToParameters;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::identifiers::*;
use crate::core::serde::default_true;
use crate::core::UnixMillis;
use crate::schema::*;

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
