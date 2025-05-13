use diesel::prelude::*;

use crate::core::UnixMillis;
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{DataResult, connect};

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

pub fn delete_user_refresh_tokens(user_id: &UserId) -> DataResult<()> {
    diesel::delete(user_refresh_tokens::table.filter(user_refresh_tokens::user_id.eq(user_id)))
        .execute(&mut connect()?)?;
    Ok(())
}

pub fn delete_device_refresh_tokens(user_id: &UserId, device_id: &DeviceId) -> DataResult<()> {
    diesel::delete(
        user_refresh_tokens::table
            .filter(user_refresh_tokens::user_id.eq(user_id))
            .filter(user_refresh_tokens::device_id.eq(device_id)),
    )
    .execute(&mut connect()?)?;
    Ok(())
}
