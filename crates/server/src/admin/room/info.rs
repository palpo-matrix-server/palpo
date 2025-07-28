use clap::Subcommand;
use futures_util::StreamExt;
use crate::core::OwnedRoomId;
use palpo_core::{AppError, AppResult};

use crate::{admin_command, admin_command_dispatch};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub(crate) enum RoomInfoCommand {
	/// - List joined members in a room
	ListJoinedMembers {
		room_id: OwnedRoomId,

		/// Lists only our local users in the specified room
		#[arg(long)]
		local_only: bool,
	},

	/// - Displays room topic
	///
	/// Room topics can be huge, so this is in its
	/// own separate command
	ViewRoomTopic {
		room_id: OwnedRoomId,
	},
}

async fn list_joined_members(ctx: &Context<'_>, room_id: OwnedRoomId, local_only: bool) -> AppResult<()> {
	let room_name = self
		.services
		.rooms
		.state_accessor
		.get_name(&room_id)
		.await
		.unwrap_or_else(|_| room_id.to_string());

	let member_info: Vec<_> = self
		.services
		.rooms
		.state_cache
		.room_members(&room_id)
		.ready_filter(|user_id| {
			local_only
				.then(|| user_id.is_local())
				.unwrap_or(true)
		})
		.map(ToOwned::to_owned)
		.filter_map(async |user_id| {
			Some((
				self.services
					.users
					.displayname(&user_id)
					.await
					.unwrap_or_else(|_| user_id.to_string()),
				user_id,
			))
		})
		.collect()
		.await;

	let num = member_info.len();
	let body = member_info
		.into_iter()
		.map(|(displayname, mxid)| format!("{mxid} | {displayname}"))
		.collect::<Vec<_>>()
		.join("\n");

	ctx.write_str(&format!("{num} Members in Room \"{room_name}\":\n```\n{body}\n```",))
		.await
}

async fn view_room_topic(ctx: &Context<'_>, room_id: OwnedRoomId) -> AppResult<()> {
	let Ok(room_topic) = self
		.services
		.rooms
		.state_accessor
		.get_room_topic(&room_id)
		.await
	else {
		return Err(AppError::public("Room does not have a room topic set."));
	};

	ctx.write_str(&format!("Room topic:\n```\n{room_topic}\n```"))
		.await
}
