use std::collections::HashMap;

use diesel::prelude::*;
use palpo_core::Seqnum;
use serde::{Deserialize, Serialize};

use crate::core::{
    OwnedUserId, RoomId, UserId,
    events::presence::{PresenceEvent, PresenceEventContent},
    presence::PresenceState,
};

use crate::core::UnixMillis;
use crate::schema::*;
use crate::{AppError, AppResult, MatrixError, db};

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
    pub occur_sn: Option<i64>,
}

impl DbPresence {
    /// Creates a PresenceEvent from available data.
    pub fn to_presence_event(&self, user_id: &UserId) -> AppResult<PresenceEvent> {
        let now = UnixMillis::now();
        let state = self.state.as_deref().map(PresenceState::from).unwrap_or_default();
        let last_active_ago = if state == PresenceState::Online {
            None
        } else {
            self.last_active_at
                .map(|last_active_at| now.0.saturating_sub(last_active_at.0))
        };

        let profile = crate::user::get_profile(user_id, None)?;
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
pub fn ping_presence(user_id: &UserId, new_state: &PresenceState) -> AppResult<()> {
    const REFRESH_TIMEOUT: u64 = 60 * 1000;

    let last_presence = last_presence(user_id);
    let state_changed = match last_presence {
        Err(_) => true,
        Ok(ref presence) => presence.content.presence != *new_state,
    };

    let last_last_active_ago = match last_presence {
        Err(_) => 0_u64,
        Ok(ref presence) => presence.content.last_active_ago.unwrap_or_default().into(),
    };

    if !state_changed && last_last_active_ago < REFRESH_TIMEOUT {
        return Ok(());
    }

    let status_msg = match last_presence {
        Ok(presence) => presence.content.status_msg.clone(),
        Err(_) => Some(String::new()),
    };

    let currently_active = *new_state == PresenceState::Online;

    set_presence(
        NewDbPresence {
            user_id: user_id.to_owned(),
            stream_id: None,
            state: Some(new_state.to_string()),
            status_msg: None,
            last_active_at: Some(UnixMillis::now()),
            last_federation_update_at: None,
            last_user_sync_at: None,
            currently_active: Some(currently_active),
            occur_sn: None,
        },
        false,
    )
}
pub fn last_presence(user_id: &UserId) -> AppResult<PresenceEvent> {
    if let Some(data) = user_presences::table
        .filter(user_presences::user_id.eq(user_id))
        .first::<DbPresence>(&mut *db::connect()?)
        .optional()?
    {
        Ok(data.to_presence_event(user_id)?)
    } else {
        Err(MatrixError::not_found("No presence data found for user").into())
    }
}

/// Adds a presence event which will be saved until a new event replaces it.
pub fn set_presence(mut presence: NewDbPresence, force: bool) -> AppResult<()> {
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
            .optional()?
            .flatten();
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
            if presence.occur_sn.is_none() {
                presence.occur_sn = Some(crate::next_sn()?);
            }
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
pub fn presences_since(since_sn: i64) -> AppResult<HashMap<OwnedUserId, PresenceEvent>> {
    let presences = user_presences::table
        .filter(user_presences::occur_sn.ge(since_sn))
        .load::<DbPresence>(&mut *db::connect()?)?;
    presences
        .into_iter()
        .map(|presence| {
            presence
                .to_presence_event(&presence.user_id)
                .map(|event| (presence.user_id, event))
        })
        .collect()
}

#[inline]
pub fn from_json_bytes_to_event(bytes: &[u8], user_id: &UserId) -> AppResult<PresenceEvent> {
    let presence = Presence::from_json_bytes(bytes)?;
    let event = presence.to_presence_event(user_id);

    Ok(event)
}

/// Represents data required to be kept in order to implement the presence
/// specification.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(super) struct Presence {
    state: PresenceState,
    currently_active: bool,
    last_active_ts: u64,
    status_msg: Option<String>,
}

impl Presence {
    #[must_use]
    pub(super) fn new(
        state: PresenceState,
        currently_active: bool,
        last_active_ts: u64,
        status_msg: Option<String>,
    ) -> Self {
        Self {
            state,
            currently_active,
            last_active_ts,
            status_msg,
        }
    }

    pub(super) fn from_json_bytes(bytes: &[u8]) -> AppResult<Self> {
        serde_json::from_slice(bytes).map_err(|_| AppError::public("Invalid presence data in database"))
    }

    /// Creates a PresenceEvent from available data.
    pub(super) fn to_presence_event(&self, user_id: &UserId) -> PresenceEvent {
        let now = UnixMillis::now();
        let last_active_ago = Some(now.0.saturating_sub(self.last_active_ts));

        PresenceEvent {
            sender: user_id.to_owned(),
            content: PresenceEventContent {
                presence: self.state.clone(),
                status_msg: self.status_msg.clone(),
                currently_active: Some(self.currently_active),
                last_active_ago,
                display_name: crate::user::display_name(user_id).ok().flatten(),
                avatar_url: crate::user::avatar_url(user_id).ok().flatten(),
            },
        }
    }
}
