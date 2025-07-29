use clap::Subcommand;
use futures_util::StreamExt;

use crate::admin::{Context, PAGE_SIZE, RoomInfo, get_room_info};
use crate::core::OwnedRoomId;
use crate::{AppError, AppResult};

#[derive(Debug, Subcommand)]
pub(crate) enum RoomDirectoryCommand {
    /// - Publish a room to the room directory
    Publish {
        /// The room id of the room to publish
        room_id: OwnedRoomId,
    },

    /// - Unpublish a room to the room directory
    Unpublish {
        /// The room id of the room to unpublish
        room_id: OwnedRoomId,
    },

    /// - List rooms that are published
    List { page: Option<usize> },
}

pub(super) async fn process(command: RoomDirectoryCommand, context: &Context<'_>) -> AppResult<()> {
    match command {
        RoomDirectoryCommand::Publish { room_id } => {
            crate::room::directory::set_public(&room_id, true)?;
            context.write_str("Room published").await
        }
        RoomDirectoryCommand::Unpublish { room_id } => {
            crate::room::directory::set_public(&room_id, false)?;
            context.write_str("Room unpublished").await
        }
        RoomDirectoryCommand::List { page } => {
            // TODO: i know there's a way to do this with clap, but i can't seem to find it
            let page = page.unwrap_or(1);
            let mut rooms: Vec<_> = crate::room::public_room_ids()?
                .into_iter()
                .map(|room_id| get_room_info(&room_id))
                .collect();

            rooms.sort_by_key(|r| r.joined_members);
            rooms.reverse();

            let rooms: Vec<_> = rooms
                .into_iter()
                .skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
                .take(PAGE_SIZE)
                .collect();

            if rooms.is_empty() {
                return Err(AppError::public("No more rooms."));
            }

            let body = rooms
                .iter()
                .map(|info| {
                    format!(
                        "{} | Members: {} | Name: {}",
                        info.id, info.joined_members, info.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            context
                .write_str(&format!("Rooms (page {page}):\n```\n{body}\n```",))
                .await
        }
    }
}
