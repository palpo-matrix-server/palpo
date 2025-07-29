pub(crate) mod appservice;
mod console;
// pub(crate) mod debug;
pub(crate) mod federation;
pub(crate) mod media;
pub(crate) mod room;
pub(crate) mod server;
pub(crate) mod user;
pub(crate) use console::Console;
mod utils;
pub(crate) use utils::*;

use std::pin::Pin;
use std::sync::OnceLock;
use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    sync::{Arc, RwLock as StdRwLock, Weak},
    time::Instant,
};
use std::{fmt, time::SystemTime};

use clap::Parser;
use diesel::prelude::*;
use futures_util::{
    Future, FutureExt, TryFutureExt,
    io::{AsyncWriteExt, BufWriter},
    lock::Mutex,
};
use regex::Regex;
use serde_json::value::to_raw_value;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::RoomMutexGuard;
use crate::core::ServerName;
use crate::core::appservice::Registration;
use crate::core::events::TimelineEventType;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::{GuestAccess, RoomGuestAccessEventContent};
use crate::core::events::room::history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent};
use crate::core::events::room::join_rule::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::room::message::RoomMessageEventContent;
use crate::core::events::room::name::RoomNameEventContent;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::events::room::topic::RoomTopicEventContent;
use crate::core::identifiers::*;
use crate::data::connect;
use crate::data::schema::*;
pub(crate) use crate::macros::admin_command_dispatch;
use crate::room::timeline;
use crate::utils::HtmlEscape;
use crate::{AUTO_GEN_PASSWORD_LENGTH, AppError, AppResult, PduEvent, config, data, membership};
use palpo_core::events::room::message::Relation;

use self::{
    appservice::AppserviceCommand, federation::FederationCommand, media::MediaCommand, room::RoomCommand,
    server::ServerCommand, user::UserCommand,
};
use super::event::PduBuilder;

pub(crate) const PAGE_SIZE: usize = 100;

crate::macros::rustc_flags_capture! {}

/// Inputs to a command are a multi-line string and optional reply_id.
#[derive(Clone, Debug, Default)]
pub struct CommandInput {
    pub command: String,
    pub reply_id: Option<OwnedEventId>,
}
/// Prototype of the tab-completer. The input is buffered text when tab
/// asserted; the output will fully replace the input buffer.
pub type Completer = fn(&str) -> String;

/// Prototype of the command processor. This is a callback supplied by the
/// reloadable admin module.
pub type Processor = fn(CommandInput) -> ProcessorFuture;

/// Return type of the processor
pub type ProcessorFuture = Pin<Box<dyn Future<Output = ProcessorResult> + Send>>;

/// Result wrapping of a command's handling. Both variants are complete message
/// events which have digested any prior errors. The wrapping preserves whether
/// the command failed without interpreting the text. Ok(None) outputs are
/// dropped to produce no response.
pub type ProcessorResult = Result<Option<CommandOutput>, CommandOutput>;

/// Alias for the output structure.
pub type CommandOutput = RoomMessageEventContent;

/// Install the admin command processor
pub async fn init() {
    unimplemented!()
    // _ = admin_service
    //     .complete
    //     .write()
    //     .expect("locked for writing")
    //     .insert(processor::complete);
    // _ = admin_service.handle.write().await.insert(processor::dispatch);
}

/// Uninstall the admin command handler
pub async fn fini() {
    unimplemented!()
    // _ = admin_service.handle.write().await.take();
    // _ = admin_service.complete.write().expect("locked for writing").take();
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
    // #[command(subcommand)]
    // /// - Commands for debugging things
    // Debug(DebugCommand),
}

#[derive(Debug)]
pub enum AdminRoomEvent {
    ProcessMessage(String),
    SendMessage(RoomMessageEventContent),
}

pub(crate) struct Context<'a> {
    pub(crate) body: &'a [&'a str],
    pub(crate) timer: SystemTime,
    pub(crate) reply_id: Option<&'a EventId>,
    pub(crate) output: Mutex<BufWriter<Vec<u8>>>,
}

impl Context<'_> {
    pub(crate) fn write_fmt(
        &self,
        arguments: fmt::Arguments<'_>,
    ) -> impl Future<Output = AppResult<()>> + Send + '_ + use<'_> {
        let buf = format!("{arguments}");
        self.output
            .lock()
            .then(async move |mut output| output.write_all(buf.as_bytes()).map_err(Into::into).await)
    }

    pub(crate) fn write_str<'a>(&'a self, s: &'a str) -> impl Future<Output = AppResult<()>> + Send + 'a {
        self.output
            .lock()
            .then(async move |mut output| output.write_all(s.as_bytes()).map_err(Into::into).await)
    }
}

pub(crate) struct RoomInfo {
    pub(crate) id: OwnedRoomId,
    pub(crate) joined_members: u64,
    pub(crate) name: String,
}

pub(crate) fn get_room_info(room_id: &RoomId) -> RoomInfo {
    RoomInfo {
        id: room_id.to_owned(),
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
        // Debug(command) => debug::process(command, context).await,
    }
}

/// Maximum number of commands which can be queued for dispatch.
const COMMAND_QUEUE_LIMIT: usize = 512;

/// Sends markdown notice to the admin room as the admin user.
pub async fn send_notice(body: &str) -> AppResult<()> {
    send_message(RoomMessageEventContent::notice_markdown(body)).await
}

/// Sends markdown message (not an m.notice for notification reasons) to the
/// admin room as the admin user.
pub async fn send_text(body: &str) -> AppResult<()> {
    send_message(RoomMessageEventContent::text_markdown(body)).await
}

/// Sends a message to the admin room as the admin user (see send_text() for
/// convenience).
pub async fn send_message(message_content: RoomMessageEventContent) -> AppResult<()> {
    let user_id = &config::server_user_id();
    let room_id = crate::room::get_admin_room()?;
    respond_to_room(message_content, &room_id, user_id).await
}

/// Posts a command to the command processor queue and returns. Processing
/// will take place on the service worker's task asynchronously. Errors if
/// the queue is full.
pub async fn command(command: String, reply_id: Option<OwnedEventId>) -> AppResult<()> {
    unimplemented!()
    // let Some(sender) = self.channel.read().expect("locked for reading").clone() else {
    //     return Err(AppError::Public("Admin command queue unavailable."));
    // };

    // sender
    //     .send(CommandInput { command, reply_id })
    //     .await
    //     .map_err(|e| AppError::Public(format!("Failed to enqueue admin command: {e:?}")))
}

/// Dispatches a command to the processor on the current task and waits for
/// completion.
pub async fn command_in_place(command: String, reply_id: Option<OwnedEventId>) -> ProcessorResult {
    process_command(CommandInput { command, reply_id }).await
}

/// Invokes the tab-completer to complete the command. When unavailable,
/// None is returned.
pub fn complete_command(command: &str) -> Option<String> {
    unimplemented!()
    // complete
    //     .read()
    //     .expect("locked for reading")
    //     .map(|complete| complete(command))
}

async fn handle_signal(sig: &'static str) {
    unimplemented!()
    // if sig == execute::SIGNAL {
    //     signal_execute().await.ok();
    // }

    // #[cfg(feature = "console")]
    // self.console.handle_signal(sig).await;
}

async fn handle_command(command: CommandInput) {
    match process_command(command).await {
        Ok(None) => debug!("Command successful with no response"),
        Ok(Some(output)) | Err(output) => handle_response(output).await.unwrap(),
    }
}

async fn process_command(command: CommandInput) -> ProcessorResult {
    unimplemented!()
    // let handle = &handle_read().await.expect("Admin module is not loaded");

    // let services = self
    //     .services
    //     .services
    //     .read()
    //     .expect("locked")
    //     .as_ref()
    //     .and_then(Weak::upgrade)
    //     .expect("Services self-reference not initialized.");

    // handle(services, command).await
}

// Parse chat messages from the admin room into an AdminCommand object
fn parse_admin_command(command_line: &str) -> std::result::Result<AdminCommand, String> {
    // Note: argv[0] is `@palpo:servername:`, which is treated as the main command
    let mut argv: Vec<_> = command_line.split_whitespace().collect();

    // Replace `help command` with `command --help`
    // Clap has a help subcommand, but it omits the long help description.
    if argv.len() > 1 && argv[1] == "help" {
        argv.remove(1);
        argv.push("--help");
    }

    // Backwards compatibility with `register_appservice`-style commands
    let command_with_dashes;
    if argv.len() > 1 && argv[1].contains('_') {
        command_with_dashes = argv[1].replace('_', "-");
        argv[1] = &command_with_dashes;
    }

    AdminCommand::try_parse_from(argv).map_err(|error| error.to_string())
}

// Utility to turn clap's `--help` text to HTML.
fn usage_to_html(text: &str, server_name: &ServerName) -> String {
    // Replace `@palpo:servername:-subcmdname` with `@palpo:servername: subcmdname`
    let text = text.replace(&format!("@palpo:{server_name}:-"), &format!("@palpo:{server_name}: "));

    // For the palpo admin room, subcommands become main commands
    let text = text.replace("SUBCOMMAND", "COMMAND");
    let text = text.replace("subcommand", "command");

    // Escape option names (e.g. `<element-id>`) since they look like HTML tags
    let text = text.replace('<', "&lt;").replace('>', "&gt;");

    // Italicize the first line (command name and version text)
    let re = Regex::new("^(.*?)\n").expect("Regex compilation should not fail");
    let text = re.replace_all(&text, "<em>$1</em>\n");

    // Unmerge wrapped lines
    let text = text.replace("\n            ", "  ");

    // Wrap option names in backticks. The lines look like:
    //     -V, --version  Prints version information
    // And are converted to:
    // <code>-V, --version</code>: Prints version information
    // (?m) enables multi-line mode for ^ and $
    let re = Regex::new("(?m)^    (([a-zA-Z_&;-]+(, )?)+)  +(.*)$").expect("Regex compilation should not fail");
    let text = re.replace_all(&text, "<code>$1</code>: $4");

    // Look for a `[commandbody]` tag. If it exists, use all lines below it that
    // start with a `#` in the USAGE section.
    let mut text_lines: Vec<&str> = text.lines().collect();
    let mut command_body = String::new();

    if let Some(line_index) = text_lines.iter().position(|line| *line == "[commandbody]") {
        text_lines.remove(line_index);

        while text_lines
            .get(line_index)
            .map(|line| line.starts_with('#'))
            .unwrap_or(false)
        {
            command_body += if text_lines[line_index].starts_with("# ") {
                &text_lines[line_index][2..]
            } else {
                &text_lines[line_index][1..]
            };
            command_body += "[nobr]\n";
            text_lines.remove(line_index);
        }
    }

    let text = text_lines.join("\n");

    // Improve the usage section
    let text = if command_body.is_empty() {
        // Wrap the usage line in code tags
        let re = Regex::new("(?m)^USAGE:\n    (@palpo:.*)$").expect("Regex compilation should not fail");
        re.replace_all(&text, "USAGE:\n<code>$1</code>").to_string()
    } else {
        // Wrap the usage line in a code block, and add a yaml block example
        // This makes the usage of e.g. `register-appservice` more accurate
        let re = Regex::new("(?m)^USAGE:\n    (.*?)\n\n").expect("Regex compilation should not fail");
        re.replace_all(&text, "USAGE:\n<pre>$1[nobr]\n[commandbodyblock]</pre>")
            .replace("[commandbodyblock]", &command_body)
    };

    // Add HTML line-breaks

    text.replace("\n\n\n", "\n\n")
        .replace('\n', "<br>\n")
        .replace("[nobr]<br>", "")
}

async fn handle_response(content: RoomMessageEventContent) -> AppResult<()> {
    let Some(Relation::Reply { in_reply_to }) = content.relates_to.as_ref() else {
        return Ok(());
    };

    let Ok(pdu) = timeline::get_pdu(&in_reply_to.event_id) else {
        error!(
            event_id = ?in_reply_to.event_id,
            "Missing admin command in_reply_to event"
        );
        return Ok(());
    };

    let response_sender = if crate::room::is_admin_room(&pdu.room_id)? {
        config::server_user_id()
    } else {
        &pdu.sender
    };

    respond_to_room(content, &pdu.room_id, response_sender).await
}

async fn respond_to_room(content: RoomMessageEventContent, room_id: &RoomId, user_id: &UserId) -> AppResult<()> {
    assert!(crate::room::is_admin_room(room_id)?, "sender is not admin");

    let state_lock = crate::room::lock_state(&room_id).await;

    if let Err(e) = timeline::build_and_append_pdu(PduBuilder::timeline(&content), user_id, room_id, &state_lock) {
        handle_response_error(e, room_id, user_id, &state_lock).await?;
    }

    Ok(())
}

async fn handle_response_error(
    e: AppError,
    room_id: &RoomId,
    user_id: &UserId,
    state_lock: &RoomMutexGuard,
) -> AppResult<()> {
    error!("Failed to build and append admin room response PDU: \"{e}\"");
    let content = RoomMessageEventContent::text_plain(format!(
        "Failed to build and append admin room PDU: \"{e}\"\n\nThe original admin command \
			 may have finished successfully, but we could not return the output."
    ));

    timeline::build_and_append_pdu(PduBuilder::timeline(&content), user_id, room_id, state_lock)?;

    Ok(())
}
