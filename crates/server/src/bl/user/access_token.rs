use diesel::prelude::*;
use once_cell::sync::Lazy;
use salvo::oapi::ToParameters;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_access_tokens)]
pub struct DbAccessToken {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub puppets_user_id: Option<OwnedUserId>,
    pub last_validated: Option<UnixMillis>,
    pub refresh_token_id: Option<i64>,
    pub is_used: bool,
    pub expired_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_access_tokens)]
pub struct NewDbAccessToken {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
    pub token: String,
    pub puppets_user_id: Option<OwnedUserId>,
    pub last_validated: Option<UnixMillis>,
    pub refresh_token_id: Option<i64>,
    pub is_used: bool,
    pub expired_at: Option<UnixMillis>,
    pub created_at: UnixMillis,
}

impl NewDbAccessToken {
    pub fn new(user_id: OwnedUserId, device_id: OwnedDeviceId, token: String) -> Self {
        Self {
            user_id,
            device_id,
            token,
            puppets_user_id: None,
            last_validated: None,
            refresh_token_id: None,
            is_used: false,
            expired_at: None,
            created_at: UnixMillis::now(),
        }
    }
}
