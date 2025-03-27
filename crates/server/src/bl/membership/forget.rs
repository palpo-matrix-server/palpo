use diesel::prelude::*;
use palpo_core::appservice::third_party;
use salvo::http::StatusError;
use tokio::sync::RwLock;

use crate::core::client::membership::{JoinRoomResBody, ThirdPartySigned};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{ AppResult, MatrixError, db, diesel_exists};

/// Makes a user forget a room.
#[tracing::instrument]
pub fn forget_room(user_id: &UserId, room_id: &RoomId) -> AppResult<()> {
    if diesel_exists!(
        room_users::table
            .filter(room_users::user_id.eq(user_id))
            .filter(room_users::room_id.eq(room_id))
            .filter(room_users::membership.eq("join")),
        &mut db::connect()?
    )? {
        return Err(MatrixError::unknown("The user has not left the room.").into());
    }
    diesel::update(
        room_users::table
            .filter(room_users::user_id.eq(user_id))
            .filter(room_users::room_id.eq(room_id)),
    )
    .set(room_users::forgotten.eq(true))
    .execute(&mut db::connect()?)?;
    Ok(())
}
