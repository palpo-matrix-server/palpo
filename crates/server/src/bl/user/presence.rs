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
use crate::{db, diesel_exists, AppError, AppResult};

/// Represents data required to be kept in order to implement the presence specification.
#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_presences)]
pub struct DbPresence {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub stream_id: Option<i64>,
    pub state: Option<String>,
    pub status_msg: Option<String>,
    pub last_active_at: Option<UnixMillis>,
    pub last_federation_update_at: Option<UnixMillis>,
    pub last_user_sync_at: Option<UnixMillis>,
    pub currently_active: Option<bool>,
    pub occur_sn: i64,
}

#[derive(Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = user_presences)]
pub struct NewDbPresence {
    pub user_id: OwnedUserId,
    pub stream_id: Option<i64>,
    pub state: Option<String>,
    pub status_msg: Option<String>,
    pub last_active_at: Option<UnixMillis>,
    pub last_federation_update_at: Option<UnixMillis>,
    pub last_user_sync_at: Option<UnixMillis>,
    pub currently_active: Option<bool>,
}

impl DbPresence {
    /// Creates a PresenceEvent from available data.
    pub fn to_presence_event(&self, user_id: &UserId, room_id: Option<&RoomId>) -> AppResult<PresenceEvent> {
        let now = UnixMillis::now();
        let state = self.state.as_deref().map(PresenceState::from).unwrap_or_default();
        let last_active_ago = if state == PresenceState::Online {
            None
        } else {
            self.last_active_at
                .map(|last_active_at| now.0.saturating_sub(last_active_at.0))
        };

        let mut profile = crate::user::get_profile(user_id, room_id)?;
        if profile.is_none() && room_id.is_some() {
            profile = crate::user::get_profile(user_id, None)?;
        }
        Ok(PresenceEvent {
            sender: user_id.to_owned(),
            content: PresenceEventContent {
                presence: state,
                status_msg: self.status_msg.clone(),
                currently_active: self.currently_active,
                last_active_ago,
                display_name: profile.as_ref().and_then(|p| p.display_name.clone()),
                avatar_url: profile.as_ref().and_then(|p| p.avatar_url.clone()),
            },
        })
    }
}

/// Resets the presence timeout, so the user will stay in their current presence state.
pub fn ping_presence(user_id: &UserId, state: &PresenceState) -> AppResult<()> {
    set_presence(NewDbPresence {
        user_id: user_id.to_owned(),
        stream_id: None,
        state: Some(state.to_string()),
        status_msg: None,
        last_active_at: Some(UnixMillis::now()),
        last_federation_update_at: None,
        last_user_sync_at: None,
        currently_active: None, //TODO,
    }, false)
}
pub fn get_last_presence(user_id: &UserId) -> AppResult<Option<DbPresence>> {
    user_presences::table
        .filter(user_presences::user_id.eq(user_id))
        // .filter(user_presences::room_id.is_null())
        .first::<DbPresence>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}

/// Adds a presence event which will be saved until a new event replaces it.
pub fn set_presence(presence: NewDbPresence, force: bool) -> AppResult<()> {
    if force {
        diesel::delete(user_presences::table.filter(user_presences::user_id.eq(&presence.user_id)))
        .execute(&mut db::connect()?)?;
    diesel::insert_into(user_presences::table)
        .values(&presence)
        .on_conflict(user_presences::user_id)
        .do_update()
        .set(&presence)
        .execute(&mut db::connect()?)?;
    } else {
        let old_state = user_presences::table
            .filter(user_presences::user_id.eq(&presence.user_id))
            .select(user_presences::state)
            .first::<Option<String>>(&mut db::connect()?)
            .optional()?.flatten();
        if old_state != presence.state && presence.state.is_some() {
            diesel::delete(user_presences::table.filter(user_presences::user_id.eq(&presence.user_id)))
                .execute(&mut db::connect()?)?;
            diesel::insert_into(user_presences::table)
                .values(&presence)
                .on_conflict(user_presences::user_id)
                .do_update()
                .set(&presence)
                .execute(&mut db::connect()?)?;
        } else {
            diesel::update(user_presences::table.filter(user_presences::user_id.eq(&presence.user_id)))
                .set(&presence)
                .execute(&mut db::connect()?)?;
        }
    }
    Ok(())
}

/// Removes the presence record for the given user from the database.
pub fn remove_presence(user_id: &UserId) -> AppResult<()> {
    diesel::delete(user_presences::table.filter(user_presences::user_id.eq(user_id))).execute(&mut db::connect()?)?;
    Ok(())
}

/// Returns the most recent presence updates that happened after the event with id `since`.
pub fn presences_since(room_id: &RoomId, since_sn: i64) -> AppResult<HashMap<OwnedUserId, PresenceEvent>> {
    let presences = user_presences::table
        .filter(user_presences::occur_sn.ge(since_sn))
        .load::<DbPresence>(&mut *db::connect()?)?;
    presences.into_iter()
        .map(|presence| {
            presence
                .to_presence_event(&presence.user_id, Some(room_id))
                .map(|event| (presence.user_id, event))
        })
        .collect()
}
