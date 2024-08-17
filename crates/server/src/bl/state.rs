use std::sync::Arc;

use crate::core::{
    events::{room::canonical_alias::RoomCanonicalAliasEventContent, AnyStateEventContent, StateEventType},
    EventId, RoomId, UserId,
};
use crate::event::PduBuilder;
use crate::{db, AppError, AppResult, MatrixError};

pub async fn send_state_event_for_key(
    user_id: &UserId,
    room_id: &RoomId,
    event_type: &StateEventType,
    json: AnyStateEventContent,
    state_key: String,
) -> AppResult<Arc<EventId>> {
    // TODO: Review this check, error if event is unparsable, use event type, allow alias if it
    // previously existed
    if let Ok(canonical_alias) = serde_json::from_value::<RoomCanonicalAliasEventContent>(serde_json::to_value(&json)?)
    {
        let mut aliases = canonical_alias.alt_aliases.clone();

        if let Some(alias) = canonical_alias.alias {
            aliases.push(alias);
        }

        for alias in aliases {
            if alias.server_name() != crate::server_name()
                || crate::room::resolve_local_alias(&alias)?
                    .filter(|room| room == room_id) // Make sure it's the right room
                    .is_none()
            {
                return Err(MatrixError::forbidden(
                    "You are only allowed to send canonical_alias \
                    events when it's aliases already exists",
                )
                .into());
            }
        }
    }
    let event_id = crate::room::timeline::build_and_append_pdu(
        PduBuilder {
            event_type: event_type.to_string().into(),
            content: serde_json::from_value(serde_json::to_value(json)?)?,
            unsigned: None,
            state_key: Some(state_key),
            redacts: None,
        },
        user_id,
        room_id,
    )?
    .event_id;

    Ok(event_id)
}
