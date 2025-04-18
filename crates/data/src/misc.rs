use diesel::prelude::*;

use crate::core::serde::JsonValue;
use crate::core::{OwnedServerName, UnixMillis};
use crate::schema::*;

#[derive(Identifiable, Queryable, Insertable, Debug, Clone)]
#[diesel(table_name = server_signing_keys, primary_key(server_id))]
pub struct DbServerSigningKeys {
    pub server_id: OwnedServerName,
    pub key_data: JsonValue,
    pub updated_at: UnixMillis,
    pub created_at: UnixMillis,
}
