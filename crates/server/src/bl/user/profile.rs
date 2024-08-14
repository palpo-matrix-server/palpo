
use std::sync::LazyLock;

use diesel::prelude::*;
use salvo::oapi::ToParameters;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::identifiers::*;
use crate::core::{OwnedMxcUri, OwnedRoomId};
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_profiles)]
pub struct DbProfile {
    pub id: i64,
    pub user_id: OwnedUserId,
    // pub server_name: Option<OwnedServerName>,
    pub room_id: Option<OwnedRoomId>,
    pub display_name: Option<String>,
    pub avatar_url: Option<OwnedMxcUri>,
    pub blurhash: Option<String>,
}

#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_profiles)]
pub struct NewDbProfile {
    pub user_id: OwnedUserId,
    // pub server_name: Option<OwnedServerName>,
    pub room_id: Option<OwnedRoomId>,
    pub display_name: Option<String>,
    pub avatar_url: Option<OwnedMxcUri>,
    pub blurhash: Option<String>,
}

pub fn get_profile(user_id: &UserId, room_id: Option<&RoomId>) -> AppResult<Option<DbProfile>> {
    user_profiles::table
        .filter(user_profiles::user_id.eq(user_id.as_str()))
        .filter(user_profiles::room_id.eq(room_id))
        .first::<DbProfile>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}
