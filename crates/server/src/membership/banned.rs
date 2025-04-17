use salvo::conn::SocketAddr;

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
    // if services.users.is_admin(user_id).await {
	// 	return Ok(());
	// }

	// if let Some(room_id) = room_id {
	// 	if services.rooms.metadata.is_banned(room_id).await
	// 		|| services
	// 			.config
	// 			.forbidden_remote_server_names
	// 			.is_match(room_id.server_name().unwrap().host())
	// 	{
	// 		warn!(
	// 			"User {user_id} who is not an admin attempted to send an invite for or \
	// 			 attempted to join a banned room or banned room server name: {room_id}"
	// 		);

	// 		if services.server.config.auto_deactivate_banned_room_attempts {
	// 			warn!(
	// 				"Automatically deactivating user {user_id} due to attempted banned room join"
	// 			);

	// 			if services.server.config.admin_room_notices {
	// 				services
	// 					.admin
	// 					.send_message(RoomMessageEventContent::text_plain(format!(
	// 						"Automatically deactivating user {user_id} due to attempted banned \
	// 						 room join from IP {client_ip}"
	// 					)))
	// 					.await
	// 					.ok();
	// 			}

	// 			let all_joined_rooms: Vec<OwnedRoomId> = services
	// 				.rooms
	// 				.state_cache
	// 				.rooms_joined(user_id)
	// 				.map(Into::into)
	// 				.collect()
	// 				.await;

	// 			full_user_deactivate(services, user_id, &all_joined_rooms).await?;
	// 		}

	// 		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	// 	}
	// } else if let Some(server_name) = server_name {
	// 	if services
	// 		.config
	// 		.forbidden_remote_server_names
	// 		.is_match(server_name.host())
	// 	{
	// 		warn!(
	// 			"User {user_id} who is not an admin tried joining a room which has the server \
	// 			 name {server_name} that is globally forbidden. Rejecting.",
	// 		);

	// 		if services.server.config.auto_deactivate_banned_room_attempts {
	// 			warn!(
	// 				"Automatically deactivating user {user_id} due to attempted banned room join"
	// 			);

	// 			if services.server.config.admin_room_notices {
	// 				services
	// 					.admin
	// 					.send_message(RoomMessageEventContent::text_plain(format!(
	// 						"Automatically deactivating user {user_id} due to attempted banned \
	// 						 room join from IP {client_ip}"
	// 					)))
	// 					.await
	// 					.ok();
	// 			}

	// 			let all_joined_rooms: Vec<OwnedRoomId> = services
	// 				.rooms
	// 				.state_cache
	// 				.rooms_joined(user_id)
	// 				.map(Into::into)
	// 				.collect()
	// 				.await;

	// 			full_user_deactivate(services, user_id, &all_joined_rooms).await?;
	// 		}

	// 		return Err!(Request(Forbidden("This remote server is banned on this homeserver.")));
	// 	}
	// }
}
