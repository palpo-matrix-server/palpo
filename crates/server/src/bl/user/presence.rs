use std::collections::HashMap;

use crate::core::{
    events::presence::{PresenceEvent, PresenceEventContent},
    presence::PresenceState,
    OwnedUserId, RoomId, UserId,
};

use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;
use crate::user::DbProfile;
use crate::{db, diesel_exists, AppError, AppResult, JsonValue};

/// Represents data required to be kept in order to implement the presence specification.
#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_presences)]
pub struct DbPresence {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub stream_id: Option<i64>,
    pub state: JsonValue,
    pub status_msg: Option<String>,
    pub last_active_at: Option<i64>,
    pub last_federation_update_at: Option<i64>,
    pub last_user_sync_at: Option<i64>,
    pub currently_active: Option<bool>,
}

#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = user_presences)]
pub struct NewDbPresence {
    pub user_id: OwnedUserId,
    pub room_id: Option<OwnedRoomId>,
    pub stream_id: Option<i64>,
    pub state: JsonValue,
    pub status_msg: Option<String>,
    pub last_active_at: Option<i64>,
    pub last_federation_update_at: Option<i64>,
    pub last_user_sync_at: Option<i64>,
    pub currently_active: Option<bool>,
}

impl DbPresence {
    /// Creates a PresenceEvent from available data.
    pub fn to_presence_event(&self, user_id: &UserId, room_id: Option<&RoomId>) -> AppResult<PresenceEvent> {
        let now = UnixMillis::now();
        let state = serde_json::from_value(self.state.clone())?;
        let last_active_ago = if state == PresenceState::Online {
            None
        } else {
            self.last_active_at
                .map(|last_active_at| now.0.saturating_sub(last_active_at as u64))
        };

        let DbProfile {
            display_name,
            avatar_url,
            ..
        } = crate::user::get_profile(user_id, room_id)?.ok_or_else(|| AppError::public("profile not found"))?;
        Ok(PresenceEvent {
            sender: user_id.to_owned(),
            content: PresenceEventContent {
                presence: state,
                status_msg: self.status_msg.clone(),
                currently_active: self.currently_active,
                last_active_ago,
                display_name,
                avatar_url,
            },
        })
    }
}

/// Resets the presence timeout, so the user will stay in their current presence state.
pub fn ping_presence(user_id: &UserId) -> AppResult<()> {
    diesel::insert_into(user_presences::table)
        .values((
            user_presences::user_id.eq(user_id),
            user_presences::last_active_at.eq(UnixMillis::now().0 as i64),
        ))
        .on_conflict(user_presences::user_id)
        .do_update()
        .set(user_presences::last_active_at.eq(UnixMillis::now().0 as i64))
        .execute(&mut *db::connect()?)?;
    Ok(())
}
pub fn get_last_presence(user_id: &UserId, room_id: &RoomId) -> AppResult<Option<DbPresence>> {
    user_presences::table
        .filter(user_presences::user_id.eq(user_id))
        .first::<DbPresence>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}

/// Adds a presence event which will be saved until a new event replaces it.
pub fn set_presence(presence: NewDbPresence) -> AppResult<()> {
    let query = user_presences::table
        .filter(user_presences::user_id.eq(&presence.user_id))
        .filter(user_presences::room_id.eq(&presence.room_id));
    if diesel_exists!(query, &mut *db::connect()?)? {
        diesel::update(query).set(&presence).execute(&mut db::connect()?)?;
    } else {
        diesel::insert_into(user_presences::table)
            .values(&presence)
            .execute(&mut db::connect()?)?;
    }
    Ok(())
}

/// Removes the presence record for the given user from the database.
pub fn remove_presence(user_id: &UserId) -> AppResult<()> {
    diesel::delete(user_presences::table.filter(user_presences::user_id.eq(user_id))).execute(&mut db::connect()?)?;
    Ok(())
}

/// Returns the most recent presence updates that happened after the event with id `since`.
pub fn presence_since(room_id: &RoomId, since_sn: i64) -> AppResult<HashMap<OwnedUserId, PresenceEvent>> {
    // TODO: presence_since
    Ok(HashMap::new())
}

fn parse_presence_event(bytes: &[u8]) -> AppResult<PresenceEvent> {
    let mut presence: PresenceEvent =
        serde_json::from_slice(bytes).map_err(|_| AppError::public("Invalid presence event in db."))?;

    let current_timestamp: u64 = UnixMillis::now().get();

    if presence.content.presence == PresenceState::Online {
        // Don't set last_active_ago when the user is online
        presence.content.last_active_ago = None;
    } else {
        // Convert from timestamp to duration
        presence.content.last_active_ago = presence
            .content
            .last_active_ago
            .map(|timestamp| current_timestamp - timestamp);
    }

    Ok(presence)
}
