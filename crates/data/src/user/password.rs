use diesel::prelude::*;

use crate::core::UnixMillis;
use crate::core::identifiers::*;
use crate::schema::*;

#[derive(Identifiable, Debug, Clone)]
#[diesel(table_name = user_passwords)]
pub struct DbPassword {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub hash: String,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Queryable, Debug, Clone)]
#[diesel(table_name = user_passwords)]
pub struct NewDbPassword {
    pub user_id: OwnedUserId,
    pub hash: String,
    pub created_at: UnixMillis,
}
