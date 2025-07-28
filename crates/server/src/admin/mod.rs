pub(crate) mod appservice;
mod console;
pub(crate) mod debug;
pub(crate) mod federation;
pub(crate) mod media;
pub(crate) mod room;
pub(crate) mod server;
pub(crate) mod user;
pub(crate) mod utils;
pub(crate) use console::Console;
pub(crate) use utils::*;

pub(crate) use crate::macros::{admin_command, admin_command_dispatch};
use clap::Parser;

use self::{
    appservice::AppserviceCommand, debug::DebugCommand, federation::FederationCommand, media::MediaCommand,
    room::RoomCommand, server::ServerCommand, user::UserCommand,
};
use crate::AppResult;
use crate::core::identifiers::*;

pub(crate) const PAGE_SIZE: usize = 100;

crate::macros::rustc_flags_capture! {}

/// Install the admin command processor
pub async fn init(admin_service: &palpo_service::admin::Service) {
    _ = admin_service
        .complete
        .write()
        .expect("locked for writing")
        .insert(processor::complete);
    _ = admin_service.handle.write().await.insert(processor::dispatch);
}

/// Uninstall the admin command handler
pub async fn fini(admin_service: &palpo_service::admin::Service) {
    _ = admin_service.handle.write().await.take();
    _ = admin_service.complete.write().expect("locked for writing").take();
}

#[derive(Debug, Parser)]
#[command(name = "palpo", version = crate::info::version())]
pub(super) enum AdminCommand {
    #[command(subcommand)]
    /// - Commands for managing appservices
    Appservices(AppserviceCommand),

    #[command(subcommand)]
    /// - Commands for managing local users
    Users(UserCommand),

    #[command(subcommand)]
    /// - Commands for managing rooms
    Rooms(RoomCommand),

    #[command(subcommand)]
    /// - Commands for managing federation
    Federation(FederationCommand),

    #[command(subcommand)]
    /// - Commands for managing the server
    Server(ServerCommand),

    #[command(subcommand)]
    /// - Commands for managing media
    Media(MediaCommand),

    #[command(subcommand)]
    /// - Commands for debugging things
    Debug(DebugCommand),
}

pub(crate) struct RoomInfo {
    pub(crate) room_id: OwnedRoomId,
    pub(crate) joined_members: u64,
    pub(crate) name: String,
}

pub(crate) async fn get_room_info(room_id: &RoomId) -> RoomInfo {
    RoomInfo {
        room_id: room_id.to_owned(),
        joined_members: crate::room::joined_member_count(room_id).unwrap_or(0),
        name: crate::room::get_name(room_id).unwrap_or_else(|_| room_id.to_string()),
    }
}

#[tracing::instrument(skip_all, name = "command")]
pub(super) async fn process(command: AdminCommand, context: &Context<'_>) -> AppResult<()> {
    use AdminCommand::*;

    match command {
        Appservices(command) => appservice::process(command, context).await,
        Media(command) => media::process(command, context).await,
        Users(command) => user::process(command, context).await,
        Rooms(command) => room::process(command, context).await,
        Federation(command) => federation::process(command, context).await,
        Server(command) => server::process(command, context).await,
        Debug(command) => debug::process(command, context).await,
    }
}
