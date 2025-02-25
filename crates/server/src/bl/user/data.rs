use std::collections::HashMap;

use diesel::prelude::*;
use serde::de::DeserializeOwned;

use crate::core::identifiers::*;
use crate::core::{
    UnixMillis,
    events::{AnyEphemeralRoomEvent, AnyEphemeralRoomEventContent, RoomAccountDataEventType},
    serde::RawJson,
};
use crate::schema::*;
use crate::{AppError, AppResult, JsonValue, db};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_datas)]
pub struct DbUserData {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub data_type: String,
    pub json_data: JsonValue,
    pub occur_sn: i64,
    pub created_at: UnixMillis,
}
#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = user_datas)]
pub struct NewDbUserData {
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub data_type: String,
    pub json_data: JsonValue,
    pub occur_sn: i64,
    pub created_at: UnixMillis,
}

/// Places one event in the account data of the user and removes the previous entry.
#[tracing::instrument(skip(room_id, user_id, event_type, json_data))]
pub fn set_data(
    user_id: &UserId,
    room_id: Option<OwnedRoomId>,
    event_type: &str,
    json_data: JsonValue,
) -> AppResult<DbUserData> {
    let new_data = NewDbUserData {
        user_id: user_id.to_owned(),
        room_id: room_id.clone(),
        data_type: event_type.to_owned(),
        json_data,
        occur_sn: crate::next_sn()? as i64,
        created_at: UnixMillis::now(),
    };
    diesel::insert_into(user_datas::table)
        .values(&new_data)
        .on_conflict((user_datas::user_id, user_datas::room_id, user_datas::data_type))
        .do_update()
        .set(&new_data)
        .get_result::<DbUserData>(&mut *db::connect()?)
        .map_err(Into::into)
}

/// Searches the account data for a specific kind.
#[tracing::instrument(skip(room_id, user_id, kind))]
pub fn get_data<E: DeserializeOwned>(user_id: &UserId, room_id: Option<&RoomId>, kind: &str) -> AppResult<Option<E>> {
    let row = user_datas::table
        .filter(user_datas::user_id.eq(user_id))
        .filter(user_datas::room_id.eq(room_id).or(user_datas::room_id.is_null()))
        .filter(user_datas::data_type.eq(kind))
        .order_by(user_datas::id.desc())
        .first::<DbUserData>(&mut *db::connect()?)
        .optional()?;
    if let Some(row) = row {
        Ok(Some(serde_json::from_value(row.json_data)?))
    } else {
        Ok(None)
    }
}

/// Returns all changes to the account data that happened after `since`.
#[tracing::instrument(skip(room_id, user_id, since_sn))]
pub fn get_data_changes(
    room_id: Option<&RoomId>,
    user_id: &UserId,
    since_sn: i64,
) -> AppResult<HashMap<RoomAccountDataEventType, RawJson<AnyEphemeralRoomEvent>>> {
    let mut user_datas = HashMap::new();

    let db_datas = user_datas::table
        .filter(user_datas::user_id.eq(user_id))
        .filter(user_datas::room_id.eq(room_id).or(user_datas::room_id.is_null()))
        .filter(user_datas::occur_sn.ge(since_sn))
        .load::<DbUserData>(&mut *db::connect()?)?;

    for db_data in db_datas {
        let kind = RoomAccountDataEventType::from(&*db_data.data_type);
        let event_content: RawJson<AnyEphemeralRoomEventContent> = RawJson::from_value(&db_data.json_data)
            .map_err(|_| AppError::public("Database contains invalid account data."))?;
        user_datas.insert(
            kind.clone(),
            RawJson::from_value(&serde_json::json!({
                "type": kind,
                "content": event_content,
            }))?,
        );
    }

    Ok(user_datas)
}
