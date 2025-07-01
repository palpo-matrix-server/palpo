use diesel::prelude::*;

use crate::core::federation::transaction::Edu;
use crate::core::presence::{PresenceContent, PresenceState, PresenceUpdate};
use crate::core::{OwnedServerName, UnixMillis, UserId};
use crate::data::connect;
use crate::data::schema::*;
use crate::data::user::{NewDbPresence, last_presence};
use crate::{AppResult, config, data, sending};

/// Resets the presence timeout, so the user will stay in their current presence state.
pub fn ping_presence(user_id: &UserId, new_state: &PresenceState) -> AppResult<()> {
    if !config::allow_local_presence() {
        return Ok(());
    }

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

    data::user::set_presence(
        NewDbPresence {
            user_id: user_id.to_owned(),
            stream_id: None,
            state: Some(new_state.to_string()),
            status_msg,
            last_active_at: Some(UnixMillis::now()),
            last_federation_update_at: None,
            last_user_sync_at: None,
            currently_active: Some(currently_active),
            occur_sn: None,
        },
        false,
    )?;
    Ok(())
}

/// Adds a presence event which will be saved until a new event replaces it.
pub fn set_presence(
    sender_id: &UserId,
    presence_state: Option<PresenceState>,
    status_msg: Option<String>,
    force: bool,
) -> AppResult<bool> {
    if !config::allow_local_presence() {
        return Ok(false);
    }

    let Some(presence_state) = presence_state else {
        data::user::remove_presence(sender_id)?;
        return Ok(false);
    };
    let db_presence = NewDbPresence {
        user_id: sender_id.to_owned(),
        stream_id: None,
        state: Some(presence_state.to_string()),
        status_msg: status_msg.clone(),
        last_active_at: None,
        last_federation_update_at: None,
        last_user_sync_at: None,
        currently_active: Some(presence_state == PresenceState::Online),
        occur_sn: None,
    };

    let state_changed = data::user::set_presence(db_presence, force)?;
    if state_changed {
        let edu = Edu::Presence(PresenceContent {
            push: vec![PresenceUpdate {
                user_id: sender_id.to_owned(),
                status_msg,
                last_active_ago: 0,
                currently_active: presence_state == PresenceState::Online,
                presence: presence_state,
            }],
        });

        let joined_rooms = data::user::joined_rooms(sender_id)?;
        let remote_servers = room_joined_servers::table
            .filter(room_joined_servers::room_id.eq_any(joined_rooms))
            .select(room_joined_servers::server_id)
            .distinct()
            .load::<OwnedServerName>(&mut connect()?)?;

        sending::send_edu_servers(remote_servers.into_iter(), &edu)?;
    }

    Ok(state_changed)
}
