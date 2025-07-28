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

use std::sync::OnceLock;
use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    sync::Arc,
    time::Instant,
};
use std::{fmt, time::SystemTime};

use clap::Parser;
use diesel::prelude::*;
use futures::{
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
use crate::room::timeline;
use crate::utils::{self, HtmlEscape};
use crate::{AUTO_GEN_PASSWORD_LENGTH, AppError, AppResult, PduEvent, config, data, membership, room};

use self::{
    appservice::AppserviceCommand, debug::DebugCommand, federation::FederationCommand, media::MediaCommand,
    room::RoomCommand, server::ServerCommand, user::UserCommand,
};
use super::event::PduBuilder;
pub(crate) use crate::macros::admin_command_dispatch;

pub(crate) const PAGE_SIZE: usize = 100;

crate::macros::rustc_flags_capture! {}

/// Install the admin command processor
pub async fn init() {
    _ = admin_service
        .complete
        .write()
        .expect("locked for writing")
        .insert(processor::complete);
    _ = admin_service.handle.write().await.insert(processor::dispatch);
}

/// Uninstall the admin command handler
pub async fn fini() {
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
    ) -> impl Future<Output = AppResult> + Send + '_ + use<'_> {
        let buf = format!("{arguments}");
        self.output
            .lock()
            .then(async move |mut output| output.write_all(buf.as_bytes()).map_err(Into::into).await)
    }

    pub(crate) fn write_str<'a>(&'a self, s: &'a str) -> impl Future<Output = AppResult> + Send + 'a {
        self.output
            .lock()
            .then(async move |mut output| output.write_all(s.as_bytes()).map_err(Into::into).await)
    }
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

pub fn process_message(room_message: String) -> AppResult<()> {
    sender()
        .send(AdminRoomEvent::ProcessMessage(room_message))
        .map_err(|e| AppError::internal(format!("failed to process message to admin room: {e}")))
}

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
    let user_id = &config::server_user();
    let room_id = self.get_admin_room().await?;
    respond_to_room(message_content, &room_id, user_id).boxed().await
}

/// Posts a command to the command processor queue and returns. Processing
/// will take place on the service worker's task asynchronously. Errors if
/// the queue is full.
pub async fn command(command: String, reply_id: Option<OwnedEventId>) -> AppResult<()> {
    let Some(sender) = self.channel.read().expect("locked for reading").clone() else {
        return Err!("Admin command queue unavailable.");
    };

    sender
        .send(CommandInput { command, reply_id })
        .await
        .map_err(|e| err!("Failed to enqueue admin command: {e:?}"))
}

/// Dispatches a command to the processor on the current task and waits for
/// completion.
pub async fn command_in_place(command: String, reply_id: Option<OwnedEventId>) -> ProcessorResult {
    self.process_command(CommandInput { command, reply_id }).await
}

/// Invokes the tab-completer to complete the command. When unavailable,
/// None is returned.
pub fn complete_command(command: &str) -> Option<String> {
    self.complete
        .read()
        .expect("locked for reading")
        .map(|complete| complete(command))
}

async fn handle_signal(sig: &'static str) {
    if sig == execute::SIGNAL {
        self.signal_execute().await.ok();
    }

    #[cfg(feature = "console")]
    self.console.handle_signal(sig).await;
}

async fn handle_command(command: CommandInput) {
    match self.process_command(command).await {
        Ok(None) => debug!("Command successful with no response"),
        Ok(Some(output)) | Err(output) => self.handle_response(output).await.unwrap_or_else(default_log),
    }
}

async fn process_command(command: CommandInput) -> ProcessorResult {
    let handle = &handle_read().await.expect("Admin module is not loaded");

    let services = self
        .services
        .services
        .read()
        .expect("locked")
        .as_ref()
        .and_then(Weak::upgrade)
        .expect("Services self-reference not initialized.");

    handle(services, command).await
}

// Parse and process a message from the admin room
async fn process_admin_message(room_message: String) -> RoomMessageEventContent {
    let mut lines = room_message.lines().filter(|l| !l.trim().is_empty());
    let command_line = lines.next().expect("each string has at least one line");
    let body: Vec<_> = lines.collect();
    let conf = crate::config::get();

    let admin_command = match parse_admin_command(command_line) {
        Ok(command) => command,
        Err(error) => {
            let server_name = &conf.server_name;
            let message = error.replace("server.name", server_name.as_str());
            let html_message = usage_to_html(&message, server_name);

            return RoomMessageEventContent::text_html(message, html_message);
        }
    };

    match process_admin_command(admin_command, body).await {
        Ok(reply_message) => reply_message,
        Err(error) => {
            let markdown_message = format!(
                "Encountered an error while handling the command:\n\
                    ```\n{error}\n```",
            );
            let html_message = format!(
                "Encountered an error while handling the command:\n\
                    <pre>\n{error}\n</pre>",
            );

            RoomMessageEventContent::text_html(markdown_message, html_message)
        }
    }
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

async fn process_admin_command(command: AdminCommand, body: Vec<&str>) -> AppResult<RoomMessageEventContent> {
    let conf = crate::config::get();
    let reply_message_content = match command {
        AdminCommand::RegisterAppservice => {
            if body.len() > 2 && body[0].trim() == "```" && body.last().unwrap().trim() == "```" {
                let appservice_conf = body[1..body.len() - 1].join("\n");
                let parsed_conf = serde_yaml::from_str::<Registration>(&appservice_conf);
                match parsed_conf {
                    Ok(yaml) => match crate::appservice::register_appservice(yaml) {
                        Ok(id) => RoomMessageEventContent::text_plain(format!("Appservice registered with ID: {id}.")),
                        Err(e) => RoomMessageEventContent::text_plain(format!("Failed to register appservice: {e}")),
                    },
                    Err(e) => RoomMessageEventContent::text_plain(format!("Could not parse appservice config: {e}")),
                }
            } else {
                RoomMessageEventContent::text_plain("Expected code block in command body. Add --help for details.")
            }
        }
        AdminCommand::UnregisterAppservice { appservice_identifier } => {
            match crate::appservice::unregister_appservice(&appservice_identifier) {
                Ok(()) => RoomMessageEventContent::text_plain("Appservice unregistered."),
                Err(e) => RoomMessageEventContent::text_plain(format!("Failed to unregister appservice: {e}")),
            }
        }
        AdminCommand::ListAppservices => {
            if let Ok(appservices) = crate::appservice::all() {
                let count = appservices.len();
                let output = format!(
                    "Appservices ({}): {}",
                    count,
                    appservices.keys().map(|s| &**s).collect::<Vec<_>>().join(", ")
                );
                RoomMessageEventContent::text_plain(output)
            } else {
                RoomMessageEventContent::text_plain("Failed to get appservices.")
            }
        }
        AdminCommand::ListRooms => {
            let room_ids = rooms::table
                .order_by(rooms::id.desc())
                .select(rooms::id)
                .load::<OwnedRoomId>(&mut connect()?)?;
            let mut items = Vec::with_capacity(room_ids.len());
            for room_id in room_ids {
                let members = room::joined_member_count(&room_id)?;
                items.push(format!("members: {} \t\tin room: {}", members, room_id));
            }
            let output = format!("Rooms:\n{}", items.join("\n"));
            RoomMessageEventContent::text_plain(output)
        }
        AdminCommand::ListLocalUsers => match data::user::list_local_users() {
            Ok(users) => {
                let mut msg: String = format!("Found {} local user account(s):\n", users.len());
                msg += &users.into_iter().map(|u| u.to_string()).collect::<Vec<_>>().join("\n");
                RoomMessageEventContent::text_plain(&msg)
            }
            Err(e) => RoomMessageEventContent::text_plain(e.to_string()),
        },
        AdminCommand::IncomingFederation => {
            let map = crate::ROOM_ID_FEDERATION_HANDLE_TIME.read().unwrap();
            let mut msg: String = format!("Handling {} incoming pdus:\n", map.len());

            for (r, (e, i)) in map.iter() {
                let elapsed = i.elapsed();
                msg += &format!("{} {}: {}m{}s\n", r, e, elapsed.as_secs() / 60, elapsed.as_secs() % 60);
            }
            RoomMessageEventContent::text_plain(&msg)
        }
        AdminCommand::GetAuthChain { event_id } => {
            let event_id = Arc::<EventId>::from(event_id);
            if let Some(event) = timeline::get_pdu_json(&event_id)? {
                let room_id_str = event
                    .get("room_id")
                    .and_then(|val| val.as_str())
                    .ok_or_else(|| AppError::internal("Invalid event in database"))?;

                let room_id = <&RoomId>::try_from(room_id_str)
                    .map_err(|_| AppError::internal("Invalid room id field in event in database"))?;
                let start = Instant::now();
                let count = room::auth_chain::get_auth_chain_sns(room_id, [&*event_id].into_iter())?.len();
                let elapsed = start.elapsed();
                RoomMessageEventContent::text_plain(format!("Loaded auth chain with length {count} in {elapsed:?}"))
            } else {
                RoomMessageEventContent::text_plain("event not found")
            }
        }
        AdminCommand::ParsePdu => {
            if body.len() > 2 && body[0].trim() == "```" && body.last().unwrap().trim() == "```" {
                let string = body[1..body.len() - 1].join("\n");
                match serde_json::from_str(&string) {
                    Ok(value) => match crate::core::signatures::reference_hash(&value, &RoomVersionId::V6) {
                        Ok(hash) => {
                            let event_id = EventId::parse(format!("${hash}"));

                            match serde_json::from_value::<PduEvent>(
                                serde_json::to_value(value).expect("value is json"),
                            ) {
                                Ok(pdu) => {
                                    RoomMessageEventContent::text_plain(format!("EventId: {event_id:?}\n{pdu:#?}"))
                                }
                                Err(e) => RoomMessageEventContent::text_plain(format!(
                                    "EventId: {event_id:?}\nCould not parse event: {e}"
                                )),
                            }
                        }
                        Err(e) => RoomMessageEventContent::text_plain(format!("Could not parse PDU JSON: {e:?}")),
                    },
                    Err(e) => RoomMessageEventContent::text_plain(format!("Invalid json in command body: {e}")),
                }
            } else {
                RoomMessageEventContent::text_plain("Expected code block in command body.")
            }
        }
        AdminCommand::GetPdu { event_id } => {
            let mut outlier = false;
            let mut pdu_json = timeline::get_pdu_json(&event_id)?;
            if pdu_json.is_none() {
                outlier = true;
                pdu_json = timeline::get_pdu_json(&event_id)?;
            }
            match pdu_json {
                Some(json) => {
                    let json_text = serde_json::to_string_pretty(&json).expect("canonical json is valid json");
                    RoomMessageEventContent::text_html(
                        format!(
                            "{}\n```json\n{}\n```",
                            if outlier { "PDU is outlier" } else { "PDU was accepted" },
                            json_text
                        ),
                        format!(
                            "<p>{}</p>\n<pre><code class=\"language-json\">{}\n</code></pre>\n",
                            if outlier { "PDU is outlier" } else { "PDU was accepted" },
                            HtmlEscape(&json_text)
                        ),
                    )
                }
                None => RoomMessageEventContent::text_plain("PDU not found."),
            }
        }
        AdminCommand::ShowConfig => {
            // Construct and send the response
            RoomMessageEventContent::text_plain(format!("{}", conf))
        }
        AdminCommand::ResetPassword { username } => {
            let user_id = match UserId::parse_with_server_name(username.as_str().to_lowercase(), &conf.server_name) {
                Ok(id) => id,
                Err(e) => {
                    return Ok(RoomMessageEventContent::text_plain(format!(
                        "The supplied username is not a valid username: {e}"
                    )));
                }
            };

            // Check if the specified user is valid
            if !data::user::user_exists(&user_id)?
                || user_id == UserId::parse_with_server_name("palpo", &conf.server_name).expect("palpo user exists")
            {
                return Ok(RoomMessageEventContent::text_plain(
                    "The specified user does not exist!",
                ));
            }

            let new_password = utils::random_string(AUTO_GEN_PASSWORD_LENGTH);

            match crate::user::set_password(&user_id, &new_password) {
                Ok(()) => RoomMessageEventContent::text_plain(format!(
                    "Successfully reset the password for user {user_id}: {new_password}"
                )),
                Err(e) => {
                    RoomMessageEventContent::text_plain(format!("Couldn't reset the password for user {user_id}: {e}"))
                }
            }
        }
        AdminCommand::CreateUser { username, password } => {
            let password = password.unwrap_or_else(|| utils::random_string(AUTO_GEN_PASSWORD_LENGTH));
            // Validate user id
            let user_id = match UserId::parse_with_server_name(username.as_str().to_lowercase(), &conf.server_name) {
                Ok(id) => id,
                Err(e) => {
                    return Ok(RoomMessageEventContent::text_plain(format!(
                        "The supplied username is not a valid username: {e}"
                    )));
                }
            };
            if user_id.is_historical() {
                return Ok(RoomMessageEventContent::text_plain(format!(
                    "Userid {user_id} is not allowed due to historical"
                )));
            }
            if data::user::user_exists(&user_id)? {
                return Ok(RoomMessageEventContent::text_plain(format!(
                    "Userid {user_id} already exists"
                )));
            }
            // Create user
            crate::user::create_user(&user_id, Some(password.as_str()))?;

            // Default to pretty display_name
            let display_name = user_id.localpart().to_owned();

            // // If enabled append lightning bolt to display name (default false)
            // if conf.enable_lightning_bolt {
            //     display_name.push_str(" ⚡️");
            // }

            data::user::set_display_name(&user_id, Some(&*display_name))?;

            // Initial account data
            data::user::set_data(
                &user_id,
                None,
                &crate::core::events::GlobalAccountDataEventType::PushRules.to_string(),
                serde_json::to_value(crate::core::events::push_rules::PushRulesEventContent {
                    global: crate::core::push::Ruleset::server_default(&user_id),
                })
                .expect("to json value always works"),
            )?;

            if !conf.auto_join_rooms.is_empty() {
                let db_user = data::user::get_user(&user_id)?;
                for room in &conf.auto_join_rooms {
                    let Ok(room_id) = crate::room::alias::resolve(room).await else {
                        error!(
                            %user_id,
                            "failed to resolve room alias to room ID when attempting to auto join {room}, skipping"
                        );
                        continue;
                    };

                    if !room::is_server_joined(&conf.server_name, &room_id)? {
                        warn!("skipping room {room} to automatically join as we have never joined before.");
                        continue;
                    }

                    if let Ok(room_server_name) = room.server_name() {
                        match membership::join_room(
                            &db_user,
                            None,
                            &room_id,
                            Some("automatically joining this room upon registration".to_owned()),
                            &[conf.server_name.clone(), room_server_name.to_owned()],
                            None,
                            None,
                            Default::default(),
                        )
                        .await
                        {
                            Ok(_response) => {
                                info!("automatically joined room {room} for user {user_id}");
                            }
                            Err(e) => {
                                // don't return this error so we don't fail registrations
                                error!("failed to automatically join room {room} for user {user_id}: {e}");
                                send_text(&format!(
                                    "failed to automatically join room {room} for user {user_id}: \
								 {e}"
                                ))
                                .await
                                .ok();
                            }
                        }
                    }
                }
            }
            // we dont add a device since we're not the user, just the creator

            // Inhibit login does not work for guests
            RoomMessageEventContent::text_plain(format!(
                "Created user with user_id: {user_id} and password: {password}"
            ))
        }
        AdminCommand::DisableRoom { room_id } => {
            room::disable_room(&room_id, true)?;
            RoomMessageEventContent::text_plain("Room disabled.")
        }
        AdminCommand::EnableRoom { room_id } => {
            room::disable_room(&room_id, false)?;
            RoomMessageEventContent::text_plain("Room enabled.")
        }
        AdminCommand::DeactivateUser { leave_rooms, user_id } => {
            let user_id = Arc::<UserId>::from(user_id);
            if data::user::user_exists(&user_id)? {
                RoomMessageEventContent::text_plain(format!("Making {user_id} leave all rooms before deactivation..."));

                data::user::deactivate(&user_id)?;

                if leave_rooms {
                    crate::membership::leave_all_rooms(&user_id).await?;
                }

                RoomMessageEventContent::text_plain(format!("User {user_id} has been deactivated"))
            } else {
                RoomMessageEventContent::text_plain(format!("User {user_id} doesn't exist on this server"))
            }
        }
        AdminCommand::DeactivateAll { leave_rooms, force } => {
            if body.len() > 2 && body[0].trim() == "```" && body.last().unwrap().trim() == "```" {
                let usernames = body.clone().drain(1..body.len() - 1).collect::<Vec<_>>();

                let mut user_ids: Vec<OwnedUserId> = Vec::new();

                for &username in &usernames {
                    match <&UserId>::try_from(username) {
                        Ok(user_id) => user_ids.push(user_id.to_owned()),
                        Err(_) => {
                            return Ok(RoomMessageEventContent::text_plain(format!(
                                "{username} is not a valid username"
                            )));
                        }
                    }
                }

                let mut deactivation_count = 0;
                let mut admins = Vec::new();

                if !force {
                    user_ids = users::table
                        .filter(users::id.eq_any(user_ids))
                        .filter(users::is_admin.eq(false))
                        .select(users::id)
                        .load::<OwnedUserId>(&mut connect()?)?;
                    admins = users::table
                        .filter(users::id.eq_any(&user_ids))
                        .filter(users::is_admin.eq(false))
                        .select(users::id)
                        .load::<String>(&mut connect()?)?;
                }

                for user_id in &user_ids {
                    if data::user::deactivate(user_id).is_ok() {
                        deactivation_count += 1
                    }
                }

                if leave_rooms {
                    for user_id in &user_ids {
                        crate::membership::leave_all_rooms(user_id).await.ok();
                    }
                }

                if admins.is_empty() {
                    RoomMessageEventContent::text_plain(format!("Deactivated {deactivation_count} accounts."))
                } else {
                    RoomMessageEventContent::text_plain(format!(
                        "Deactivated {} accounts.\nSkipped admin accounts: {:?}. Use --force to deactivate admin accounts",
                        deactivation_count,
                        admins.join(", ")
                    ))
                }
            } else {
                RoomMessageEventContent::text_plain("Expected code block in command body. Add --help for details.")
            }
        }
        AdminCommand::SignJson => {
            if body.len() > 2 && body[0].trim() == "```" && body.last().unwrap().trim() == "```" {
                let string = body[1..body.len() - 1].join("\n");
                match serde_json::from_str(&string) {
                    Ok(mut value) => {
                        crate::core::signatures::sign_json(
                            config::get().server_name.as_str(),
                            config::keypair(),
                            &mut value,
                        )
                        .expect("our request json is what palpo expects");
                        let json_text = serde_json::to_string_pretty(&value).expect("canonical json is valid json");
                        RoomMessageEventContent::text_plain(json_text)
                    }
                    Err(e) => RoomMessageEventContent::text_plain(format!("Invalid json: {e}")),
                }
            } else {
                RoomMessageEventContent::text_plain("Expected code block in command body. Add --help for details.")
            }
        }
        _ => RoomMessageEventContent::text_plain("Command not implemented."), // AdminCommand::VerifyJson => {
                                                                              //     if body.len() > 2 && body[0].trim() == "```" && body.last().unwrap().trim() == "```" {
                                                                              //         let string = body[1..body.len() - 1].join("\n");
                                                                              //         match serde_json::from_str(&string) {
                                                                              //             Ok(value) => {
                                                                              //                 let pub_key_map = RwLock::new(BTreeMap::new());

                                                                              //                 // Generally we shouldn't be checking against expired keys unless required, so in the admin
                                                                              //                 // room it might be best to not allow expired keys
                                                                              //                 // handler::fetch_required_signing_keys(&value, &pub_key_map).await?;

                                                                              //                 let mut expired_key_map = BTreeMap::new();
                                                                              //                 let mut valid_key_map = BTreeMap::new();

                                                                              //                 for (server, keys) in pub_key_map.into_inner().into_iter() {
                                                                              //                     if keys.valid_until_ts > UnixMillis::now() {
                                                                              //                         valid_key_map.insert(
                                                                              //                             server,
                                                                              //                             keys.verify_keys.into_iter().map(|(id, key)| (id, key.key)).collect(),
                                                                              //                         );
                                                                              //                     } else {
                                                                              //                         expired_key_map.insert(
                                                                              //                             server,
                                                                              //                             keys.verify_keys.into_iter().map(|(id, key)| (id, key.key)).collect(),
                                                                              //                         );
                                                                              //                     }
                                                                              //                 }
                                                                              //                 if crate::core::signatures::verify_json(&valid_key_map, &value).is_ok() {
                                                                              //                     RoomMessageEventContent::text_plain("Signature correct")
                                                                              //                 } else if let Err(e) = crate::core::signatures::verify_json(&expired_key_map, &value) {
                                                                              //                     RoomMessageEventContent::text_plain(format!("Signature verification failed: {e}"))
                                                                              //                 } else {
                                                                              //                     RoomMessageEventContent::text_plain("Signature correct (with expired keys)")
                                                                              //                 }
                                                                              //             }
                                                                              //             Err(e) => RoomMessageEventContent::text_plain(format!("Invalid json: {e}")),
                                                                              //         }
                                                                              //     } else {
                                                                              //         RoomMessageEventContent::text_plain("Expected code block in command body. Add --help for details.")
                                                                              //     }
                                                                              // }
    };

    Ok(reply_message_content)
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

    let response_sender = if crate::room::is_admin_room(pdu.room_id()) {
        &self.services.globals.server_user
    } else {
        pdu.sender()
    };

    respond_to_room(content, pdu.room_id(), response_sender).await
}

async fn respond_to_room(content: RoomMessageEventContent, room_id: &RoomId, user_id: &UserId) -> AppResult<()> {
    assert!(crate::room::is_admin_room(room_id), "sender is not admin");

    let state_lock = crate::room::lock_state(&room_id).await;

    if let Err(e) = timeline::build_and_append_pdu(PduBuilder::timeline(&content), user_id, room_id, &state_lock) {
        self.handle_response_error(e, room_id, user_id, &state_lock)
            .await
            .unwrap_or_else(default_log);
    }

    Ok(())
}

async fn handle_response_error(
    e: Error,
    room_id: &RoomId,
    user_id: &UserId,
    state_lock: &RoomMutexGuard,
) -> AppResult<()> {
    error!("Failed to build and append admin room response PDU: \"{e}\"");
    let content = RoomMessageEventContent::text_plain(format!(
        "Failed to build and append admin room PDU: \"{e}\"\n\nThe original admin command \
			 may have finished successfully, but we could not return the output."
    ));

    timeline::build_and_append_pdu(PduBuilder::timeline(&content), user_id, room_id, state_lock).await?;

    Ok(())
}
