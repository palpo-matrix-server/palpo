use std::collections::HashMap;

use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::UnixMillis;
use crate::core::events::presence::{PresenceEvent, PresenceEventContent};
use crate::core::presence::PresenceState;
use crate::core::{OwnedUserId, UserId};
use crate::data::connect;
use crate::data::schema::*;
use crate::data::user::{set_presence, NewDbPresence, last_presence};
use crate::{AppError, AppResult, MatrixError, data};

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
    )?;
    Ok(())
}
