use diesel::prelude::*;
use serde::Deserialize;

use crate::core::{client::filter::FilterDefinition, identifiers::*, JsonValue, UnixMillis};
use crate::schema::*;
use crate::{db, AppResult};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_filters)]
pub struct DbUserFilter {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub filter: JsonValue,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_filters)]
pub struct NewDbUserFilter {
    pub user_id: OwnedUserId,
    pub filter: JsonValue,
    pub created_at: UnixMillis,
}

pub fn get_filter(user_id: &UserId, filter_id: i64) -> AppResult<Option<FilterDefinition>> {
    let filter = user_filters::table
        .filter(user_filters::id.eq(filter_id))
        .filter(user_filters::user_id.eq(user_id))
        .select(user_filters::filter)
        .first(&mut *db::connect()?)
        .optional()?;
    if let Some(filter) = filter {
        Ok(Some(serde_json::from_value(filter)?))
    } else {
        Ok(None)
    }
}

pub fn create_filter(user_id: &UserId, filter: &FilterDefinition) -> AppResult<i64> {
    let filter = diesel::insert_into(user_filters::table)
        .values(NewDbUserFilter {
            user_id: user_id.to_owned(),
            filter: serde_json::to_value(filter)?,
            created_at: UnixMillis::now(),
        })
        .get_result::<DbUserFilter>(&mut *db::connect()?)?;
    Ok(filter.id)
}
