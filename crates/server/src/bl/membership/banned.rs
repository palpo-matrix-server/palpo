use salvo::conn::SocketAddr;

use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::identifiers::*;

use crate::AppResult;

/// Checks if the room is banned in any way possible and the sender user is not
/// an admin.
///
/// Performs automatic deactivation if `auto_deactivate_banned_room_attempts` is
/// enabled
#[tracing::instrument]
pub fn banned_room_check(
    user_id: &UserId,
    room_id: Option<&RoomId>,
    server_name: Option<&ServerName>,
    client_addr: &SocketAddr,
) -> AppResult<()> {
    // TODO: NOW
    return Ok(());
}
