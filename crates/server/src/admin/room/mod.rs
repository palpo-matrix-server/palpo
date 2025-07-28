mod alias;
mod directory;
mod info;
mod moderation;

use clap::Subcommand;

use futures_util::StreamExt;

use self::{
    alias::RoomAliasCommand, directory::RoomDirectoryCommand, info::RoomInfoCommand, moderation::RoomModerationCommand,
};
use crate::admin::{PAGE_SIZE, admin_command, get_room_info};
use crate::core::OwnedRoomId;
use crate::macros::admin_command_dispatch;
use crate::{AppError, AppResult};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub(super) enum RoomCommand {
    /// - List all rooms the server knows about
    #[clap(alias = "list")]
    ListRooms {
        page: Option<usize>,

        /// Excludes rooms that we have federation disabled with
        #[arg(long)]
        exclude_disabled: bool,

        /// Excludes rooms that we have banned
        #[arg(long)]
        exclude_banned: bool,

        #[arg(long)]
        /// Whether to only output room IDs without supplementary room
        /// information
        no_details: bool,
    },

    #[command(subcommand)]
    /// - View information about a room we know about
    Info(RoomInfoCommand),

    #[command(subcommand)]
    /// - Manage moderation of remote or local rooms
    Moderation(RoomModerationCommand),

    #[command(subcommand)]
    /// - Manage rooms' aliases
    Alias(RoomAliasCommand),

    #[command(subcommand)]
    /// - Manage the room directory
    Directory(RoomDirectoryCommand),

    /// - Check if we know about a room
    Exists { room_id: OwnedRoomId },
}

#[admin_command]
pub(super) async fn list_rooms(
    &self,
    page: Option<usize>,
    exclude_disabled: bool,
    exclude_banned: bool,
    no_details: bool,
) -> AppResult<()> {
    // TODO: i know there's a way to do this with clap, but i can't seem to find it
    let page = page.unwrap_or(1);
    let mut rooms = self
        .services
        .rooms
        .metadata
        .iter_ids()
        .filter_map(async |room_id| {
            (!exclude_disabled || !self.services.rooms.metadata.is_disabled(room_id).await).then_some(room_id)
        })
        .filter_map(async |room_id| {
            (!exclude_banned || !self.services.rooms.metadata.is_banned(room_id).await).then_some(room_id)
        })
        .then(|room_id| get_room_info(self.services, room_id))
        .collect::<Vec<_>>()
        .await;

    rooms.sort_by_key(|r| r.1);
    rooms.reverse();

    let rooms = rooms
        .into_iter()
        .skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
        .take(PAGE_SIZE)
        .collect::<Vec<_>>();

    if rooms.is_empty() {
        return Err(AppError::public("No more rooms."));
    }

    let body = rooms
        .iter()
        .map(|(id, members, name)| {
            if no_details {
                format!("{id}")
            } else {
                format!("{id}\tMembers: {members}\tName: {name}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    self.write_str(&format!("Rooms ({}):\n```\n{body}\n```", rooms.len(),))
        .await
}

#[admin_command]
pub(super) async fn exists(&self, room_id: OwnedRoomId) -> AppResult<()> {
    let result = self.services.rooms.metadata.exists(&room_id).await;

    self.write_str(&format!("{result}")).await
}
