use diesel::prelude::*;
use once_cell::sync::Lazy;
use salvo::oapi::ToParameters;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_refresh_tokens)]
pub struct DbRefreshToken {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub next_token_id: Option<i64>,
    pub expired_at: Option<i64>,
    pub ultimate_session_expired_at: Option<i64>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_refresh_tokens)]
pub struct NewDbRefreshToken {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub next_token_id: Option<i64>,
    pub expired_at: Option<i64>,
    pub ultimate_session_expired_at: Option<i64>,
    pub created_at: UnixMillis,
}
