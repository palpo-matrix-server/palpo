use std::{collections::BTreeMap, fmt::Write as _};

use futures_util::{FutureExt, StreamExt};

use crate::admin::{get_room_info, parse_local_user_id};
use crate::core::{
    OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId, UserId,
    events::{
        RoomAccountDataEventType, StateEventType,
        room::{
            power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
            redaction::RoomRedactionEventContent,
        },
        tag::{TagEvent, TagEventContent, TagInfo},
    },
};
use crate::membership::leave_all_rooms;
use crate::user::full_user_deactivate;
use crate::{AppError, AppResult, IsRemoteOrLocal, membership, PduBuilder, config, utils};

const AUTO_GEN_PASSWORD_LENGTH: usize = 25;
const BULK_JOIN_REASON: &str = "Bulk force joining this room as initiated by the server admin.";

#[admin_command]
pub(super) async fn list_users(&self) -> AppResult<()> {
    let users: Vec<_> = self
        .services
        .users
        .list_local_users()
        .map(ToString::to_string)
        .collect()
        .await;

    let mut plain_msg = format!("Found {} local user account(s):\n```\n", users.len());
    plain_msg += users.join("\n").as_str();
    plain_msg += "\n```";

    self.write_str(&plain_msg).await
}

#[admin_command]
pub(super) async fn create_user(&self, username: String, password: Option<String>) -> AppResult<()> {
    // Validate user id
    let user_id = parse_local_user_id(&username)?;

    if let Err(e) = user_id.validate_strict() {
        if self.services.config.emergency_password.is_none() {
            return Err(AppError::public(format!(
                "Username {user_id} contains disallowed characters or spaces: {e}"
            )));
        }
    }

    if self.services.users.exists(&user_id).await {
        return Err(AppError::public(format!("User {user_id} already exists")));
    }

    let password = password.unwrap_or_else(|| utils::random_string(AUTO_GEN_PASSWORD_LENGTH));

    // Create user
    self.services
        .users
        .create(&user_id, Some(password.as_str()), None)
        .await?;

    // Default to pretty displayname
    let mut displayname = user_id.localpart().to_owned();

    // If `new_user_displayname_suffix` is set, registration will push whatever
    // content is set to the user's display name with a space before it
    if !self.services.server.config.new_user_displayname_suffix.is_empty() {
        write!(
            displayname,
            " {}",
            self.services.server.config.new_user_displayname_suffix
        )?;
    }

    self.services.users.set_displayname(&user_id, Some(displayname));

    // Initial account data
    self.services
        .account_data
        .update(
            None,
            &user_id,
            crate::core::events::GlobalAccountDataEventType::PushRules
                .to_string()
                .into(),
            &serde_json::to_value(crate::core::events::push_rules::PushRulesEvent {
                content: crate::core::events::push_rules::PushRulesEventContent {
                    global: crate::core::push::Ruleset::server_default(&user_id),
                },
            })?,
        )
        .await?;

    if !self.services.server.config.auto_join_rooms.is_empty() {
        for room in &self.services.server.config.auto_join_rooms {
            let Ok(room_id) = self.services.rooms.alias.resolve(room).await else {
                error!(
                    %user_id,
                    "Failed to resolve room alias to room ID when attempting to auto join {room}, skipping"
                );
                continue;
            };

            if !self
                .services
                .rooms
                .state_cache
                .server_in_room(config::server_name(), &room_id)
                .await
            {
                warn!("Skipping room {room} to automatically join as we have never joined before.");
                continue;
            }

            if let Some(room_server_name) = room.server_name() {
                match membership::join_room(
                    &user_id,
                    &room_id,
                    Some("Automatically joining this room upon registration".to_owned()),
                    &[config::server_name().to_owned(), room_server_name.to_owned()],
                    None,
                    &None,
                )
                .await
                {
                    Ok(_response) => {
                        info!("Automatically joined room {room} for user {user_id}");
                    }
                    Err(e) => {
                        // don't return this error so we don't fail registrations
                        error!("Failed to automatically join room {room} for user {user_id}: {e}");
                        self.services
                            .admin
                            .send_text(&format!(
                                "Failed to automatically join room {room} for user {user_id}: \
								 {e}"
                            ))
                            .await;
                    }
                }
            }
        }
    }

    // we dont add a device since we're not the user, just the creator

    // if this account creation is from the CLI / --execute, invite the first user
    // to admin room
    if let Ok(admin_room) = self.services.admin.get_admin_room().await {
        if self
            .services
            .rooms
            .state_cache
            .room_joined_count(&admin_room)
            .await
            .is_ok_and(is_equal_to!(1))
        {
            self.services.admin.make_user_admin(&user_id).await?;
            warn!("Granting {user_id} admin privileges as the first user");
        }
    } else {
        debug!("create_user admin command called without an admin room being available");
    }

    self.write_str(&format!(
        "Created user with user_id: {user_id} and password: `{password}`"
    ))
    .await
}

#[admin_command]
pub(super) async fn deactivate(&self, no_leave_rooms: bool, user_id: String) -> AppResult<()> {
    // Validate user id
    let user_id = parse_local_user_id(&user_id)?;

    // don't deactivate the server service account
    if user_id == config::server_user() {
        return Err(AppError::public(
            "Not allowed to deactivate the server service account.",
        ));
    }

    self.services.users.deactivate_account(&user_id).await?;

    if !no_leave_rooms {
        crate::admin::send_text(&format!("Making {user_id} leave all rooms after deactivation..."))
            .await;

        let all_joined_rooms: Vec<OwnedRoomId> = self
            .services
            .rooms
            .state_cache
            .rooms_joined(&user_id)
            .map(Into::into)
            .collect()
            .await;

        full_user_deactivate(&user_id, &all_joined_rooms)
            .boxed()
            .await?;

        update_displayname( &user_id, None, &all_joined_rooms).await;
        update_avatar_url( &user_id, None, None, &all_joined_rooms).await;
        leave_all_rooms(&user_id).await;
    }

    self.write_str(&format!("User {user_id} has been deactivated")).await
}

#[admin_command]
pub(super) async fn reset_password(&self, username: String, password: Option<String>) -> AppResult<()> {
    let user_id = parse_local_user_id(&username)?;

    if user_id == config::server_user() {
        return Err(AppError::public(
            "Not allowed to set the password for the server account. Please use the emergency password config option.",
        ));
    }

    let new_password = password.unwrap_or_else(|| utils::random_string(AUTO_GEN_PASSWORD_LENGTH));

    match self
        .services
        .users
        .set_password(&user_id, Some(new_password.as_str()))
        .await
    {
        Err(e) => {
            return Err(AppError::public(format!(
                "Couldn't reset the password for user {user_id}: {e}"
            )));
        }
        Ok(()) => write!(
            self,
            "Successfully reset the password for user {user_id}: `{new_password}`"
        ),
    }
    .await
}

#[admin_command]
pub(super) async fn deactivate_all(&self, no_leave_rooms: bool, force: bool) -> AppResult<()> {
    if self.body.len() < 2 || !self.body[0].trim().starts_with("```") || self.body.last().unwrap_or(&"").trim() != "```"
    {
        return Err(AppError::public(
            "Expected code block in command body. Add --help for details.",
        ));
    }

    let usernames = self
        .body
        .to_vec()
        .drain(1..self.body.len().saturating_sub(1))
        .collect::<Vec<_>>();

    let mut user_ids: Vec<OwnedUserId> = Vec::with_capacity(usernames.len());
    let mut admins = Vec::new();

    for username in usernames {
        match parse_active_local_user_id(username).await {
            Err(e) => {
                self.services
                    .admin
                    .send_text(&format!("{username} is not a valid username, skipping over: {e}"))
                    .await;

                continue;
            }
            Ok(user_id) => {
                if self.services.users.is_admin(&user_id).await && !force {
                    self.services
                        .admin
                        .send_text(&format!("{username} is an admin and --force is not set, skipping over"))
                        .await;

                    admins.push(username);
                    continue;
                }

                // don't deactivate the server service account
                if user_id == config::server_user() {
                    self.services
                        .admin
                        .send_text(&format!("{username} is the server service account, skipping over"))
                        .await;

                    continue;
                }

                user_ids.push(user_id);
            }
        }
    }

    let mut deactivation_count: usize = 0;

    for user_id in user_ids {
        match self.services.users.deactivate_account(&user_id).await {
            Err(e) => {
                self.services
                    .admin
                    .send_text(&format!("Failed deactivating user: {e}"))
                    .await;
            }
            Ok(()) => {
                deactivation_count = deactivation_count.saturating_add(1);
                if !no_leave_rooms {
                    info!("Forcing user {user_id} to leave all rooms apart of deactivate-all");
                    let all_joined_rooms: Vec<OwnedRoomId> = self
                        .services
                        .rooms
                        .state_cache
                        .rooms_joined(&user_id)
                        .map(Into::into)
                        .collect()
                        .await;

                    full_user_deactivate(&user_id, &all_joined_rooms)
                        .boxed()
                        .await?;

                    update_displayname(&user_id, None, &all_joined_rooms).await;
                    update_avatar_url( &user_id, None, None, &all_joined_rooms).await;
                    leave_all_rooms( &user_id).await;
                }
            }
        }
    }

    if admins.is_empty() {
        write!(self, "Deactivated {deactivation_count} accounts.")
    } else {
        write!(
            self,
            "Deactivated {deactivation_count} accounts.\nSkipped admin accounts: {}. Use \
			 --force to deactivate admin accounts",
            admins.join(", ")
        )
    }
    .await
}

#[admin_command]
pub(super) async fn list_joined_rooms(&self, user_id: String) -> AppResult<()> {
    // Validate user id
    let user_id = parse_local_user_id(&user_id)?;

    let mut rooms: Vec<(OwnedRoomId, u64, String)> = self
        .services
        .rooms
        .state_cache
        .rooms_joined(&user_id)
        .then(|room_id| get_room_info(room_id))
        .collect()
        .await;

    if rooms.is_empty() {
        return Err(AppError::public("User is not in any rooms."));
    }

    rooms.sort_by_key(|r| r.1);
    rooms.reverse();

    let body = rooms
        .iter()
        .map(|(id, members, name)| format!("{id}\tMembers: {members}\tName: {name}"))
        .collect::<Vec<_>>()
        .join("\n");

    self.write_str(&format!("Rooms {user_id} Joined ({}):\n```\n{body}\n```", rooms.len(),))
        .await
}

#[admin_command]
pub(super) async fn force_join_list_of_local_users(
    &self,
    room_id: OwnedRoomOrAliasId,
    yes_i_want_to_do_this: bool,
) -> AppResult<()> {
    if self.body.len() < 2 || !self.body[0].trim().starts_with("```") || self.body.last().unwrap_or(&"").trim() != "```"
    {
        return Err(AppError::public(
            "Expected code block in command body. Add --help for details.",
        ));
    }

    if !yes_i_want_to_do_this {
        return Err(AppError::public(
            "You must pass the --yes-i-want-to-do-this-flag to ensure you really want to force \
			 bulk join all specified local users.",
        ));
    }

    let Ok(admin_room) = self.services.admin.get_admin_room().await else {
        return Err(AppError::public(
            "There is not an admin room to check for server admins.",
        ));
    };

    let (room_id, servers) = self.services.rooms.alias.resolve_with_servers(&room_id, None).await?;

    if !self
        .services
        .rooms
        .state_cache
        .server_in_room(config::server_name(), &room_id)
        .await
    {
        return Err(AppError::public("We are not joined in this room."));
    }

    let server_admins: Vec<_> = self
        .services
        .rooms
        .state_cache
        .active_local_users_in_room(&admin_room)
        .map(ToOwned::to_owned)
        .collect()
        .await;

    if !self
        .services
        .rooms
        .state_cache
        .room_members(&room_id)
        .ready_any(|user_id| server_admins.contains(&user_id.to_owned()))
        .await
    {
        return Err(AppError::public("There is not a single server admin in the room."));
    }

    let usernames = self
        .body
        .to_vec()
        .drain(1..self.body.len().saturating_sub(1))
        .collect::<Vec<_>>();

    let mut user_ids: Vec<OwnedUserId> = Vec::with_capacity(usernames.len());

    for username in usernames {
        match parse_active_local_user_id(username).await {
            Ok(user_id) => {
                // don't make the server service account join
                if user_id == config::server_user(){
                    self.services
                        .admin
                        .send_text(&format!("{username} is the server service account, skipping over"))
                        .await;

                    continue;
                }

                user_ids.push(user_id);
            }
            Err(e) => {
                self.services
                    .admin
                    .send_text(&format!("{username} is not a valid username, skipping over: {e}"))
                    .await;

                continue;
            }
        }
    }

    let mut failed_joins: usize = 0;
    let mut successful_joins: usize = 0;

    for user_id in user_ids {
        match membership::join_room(
            &user_id,
            &room_id,
            Some(String::from(BULK_JOIN_REASON)),
            &servers,
            None,
            &None,
        )
        .await
        {
            Ok(_res) => {
                successful_joins = successful_joins.saturating_add(1);
            }
            Err(e) => {
                debug_warn!("Failed force joining {user_id} to {room_id} during bulk join: {e}");
                failed_joins = failed_joins.saturating_add(1);
            }
        }
    }

    self.write_str(&format!(
        "{successful_joins} local users have been joined to {room_id}. {failed_joins} joins \
		 failed.",
    ))
    .await
}

#[admin_command]
pub(super) async fn force_join_all_local_users(
    &self,
    room_id: OwnedRoomOrAliasId,
    yes_i_want_to_do_this: bool,
) -> AppResult<()> {
    if !yes_i_want_to_do_this {
        return Err(AppError::public(
            "you must pass the --yes-i-want-to-do-this-flag to ensure you really want to force \
			 bulk join all local users.",
        ));
    }

    let Ok(admin_room) = self.services.admin.get_admin_room().await else {
        return Err(AppError::public(
            "There is not an admin room to check for server admins.",
        ));
    };

    let (room_id, servers) = self.services.rooms.alias.resolve_with_servers(&room_id, None).await?;

    if !self
        .services
        .rooms
        .state_cache
        .server_in_room(config::server_name(), &room_id)
        .await
    {
        return Err(AppError::public("we are not joined in this room"));
    }

    let server_admins: Vec<_> = self
        .services
        .rooms
        .state_cache
        .active_local_users_in_room(&admin_room)
        .map(ToOwned::to_owned)
        .collect()
        .await;

    if !self
        .services
        .rooms
        .state_cache
        .room_members(&room_id)
        .ready_any(|user_id| server_admins.contains(&user_id.to_owned()))
        .await
    {
        return Err(AppError::public("there is not a single server admin in the room."));
    }

    let mut failed_joins: usize = 0;
    let mut successful_joins: usize = 0;

    for user_id in &self
        .services
        .users
        .list_local_users()
        .map(UserId::to_owned)
        .collect::<Vec<_>>()
        .await
    {
        match join_room(
            user_id,
            &room_id,
            Some(String::from(BULK_JOIN_REASON)),
            &servers,
            None,
            &None,
        )
        .await
        {
            Ok(_res) => {
                successful_joins = successful_joins.saturating_add(1);
            }
            Err(e) => {
                warn!("Failed force joining {user_id} to {room_id} during bulk join: {e}");
                failed_joins = failed_joins.saturating_add(1);
            }
        }
    }

    self.write_str(&format!(
        "{successful_joins} local users have been joined to {room_id}. {failed_joins} joins \
		 failed.",
    ))
    .await
}

#[admin_command]
pub(super) async fn force_join_room(&self, user_id: String, room_id: OwnedRoomOrAliasId) -> AppResult<()> {
    let user_id = parse_local_user_id(&user_id)?;
    let (room_id, servers) = self.services.rooms.alias.resolve_with_servers(&room_id, None).await?;

    assert!(user_id.is_local(), "parsed user_id must be a local user");
    membership::join_room(&user_id, &room_id, None, &servers, None, &None).await?;

    self.write_str(&format!("{user_id} has been joined to {room_id}.",))
        .await
}

#[admin_command]
pub(super) async fn force_leave_room(&self, user_id: String, room_id: OwnedRoomOrAliasId) -> AppResult<()> {
    let user_id = parse_local_user_id(&user_id)?;
    let room_id = self.services.rooms.alias.resolve(&room_id).await?;

    assert!(user_id.is_local(), "parsed user_id must be a local user");

    if !self.services.rooms.state_cache.is_joined(&user_id, &room_id).await {
        return Err(AppError::public("{user_id} is not joined in the room"));
    }

    leave_room(&user_id, &room_id, None).boxed().await?;

    self.write_str(&format!("{user_id} has left {room_id}.",)).await
}

#[admin_command]
pub(super) async fn force_demote(&self, user_id: String, room_id: OwnedRoomOrAliasId) -> AppResult<()> {
    let user_id = parse_local_user_id(&user_id)?;
    let room_id = self.services.rooms.alias.resolve(&room_id).await?;

    assert!(user_id.is_local(), "parsed user_id must be a local user");

    let state_lock = self.services.rooms.state.mutex.lock(&room_id).await;

    let room_power_levels: Option<RoomPowerLevelsEventContent> = self
        .services
        .rooms
        .state_accessor
        .room_state_get_content(&room_id, &StateEventType::RoomPowerLevels, "")
        .await
        .ok();

    let user_can_demote_self = room_power_levels.as_ref().is_some_and(|power_levels_content| {
        RoomPowerLevels::from(power_levels_content.clone()).user_can_change_user_power_level(&user_id, &user_id)
    }) || self
        .services
        .rooms
        .state_accessor
        .room_state_get(&room_id, &StateEventType::RoomCreate, "")
        .await
        .is_ok_and(|event| event.sender() == user_id);

    if !user_can_demote_self {
        return Err(AppError::public(
            "User is not allowed to modify their own power levels in the room.",
        ));
    }

    let mut power_levels_content = room_power_levels.unwrap_or_default();
    power_levels_content.users.remove(&user_id);

    let event_id = self
        .services
        .rooms
        .timeline
        .build_and_append_pdu(
            PduBuilder::state(String::new(), &power_levels_content),
            &user_id,
            &room_id,
            &state_lock,
        )
        .await?;

    self.write_str(&format!(
        "User {user_id} demoted themselves to the room default power level in {room_id} - \
		 {event_id}"
    ))
    .await
}

#[admin_command]
pub(super) async fn make_user_admin(&self, user_id: String) -> AppResult<()> {
    let user_id = parse_local_user_id(&user_id)?;
    assert!(user_id.is_local(), "Parsed user_id must be a local user");

    self.services.admin.make_user_admin(&user_id).await?;

    self.write_str(&format!("{user_id} has been granted admin privileges.",))
        .await
}

#[admin_command]
pub(super) async fn put_room_tag(&self, user_id: String, room_id: OwnedRoomId, tag: String) -> AppResult<()> {
    let user_id = parse_active_local_user_id(&user_id).await?;

    let mut tags_event = self
        .services
        .account_data
        .get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
        .await
        .unwrap_or(TagEvent {
            content: TagEventContent { tags: BTreeMap::new() },
        });

    tags_event.content.tags.insert(tag.clone().into(), TagInfo::new());

    self.services
        .account_data
        .update(
            Some(&room_id),
            &user_id,
            RoomAccountDataEventType::Tag,
            &serde_json::to_value(tags_event).expect("to json value always works"),
        )
        .await?;

    self.write_str(&format!(
        "Successfully updated room account data for {user_id} and room {room_id} with tag {tag}"
    ))
    .await
}

#[admin_command]
pub(super) async fn delete_room_tag(&self, user_id: String, room_id: OwnedRoomId, tag: String) -> AppResult<()> {
    let user_id = parse_active_local_user_id(&user_id).await?;

    let mut tags_event = self
        .services
        .account_data
        .get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
        .await
        .unwrap_or(TagEvent {
            content: TagEventContent { tags: BTreeMap::new() },
        });

    tags_event.content.tags.remove(&tag.clone().into());

    self.services
        .account_data
        .update(
            Some(&room_id),
            &user_id,
            RoomAccountDataEventType::Tag,
            &serde_json::to_value(tags_event).expect("to json value always works"),
        )
        .await?;

    self.write_str(&format!(
        "Successfully updated room account data for {user_id} and room {room_id}, deleting room \
		 tag {tag}"
    ))
    .await
}

#[admin_command]
pub(super) async fn get_room_tags(&self, user_id: String, room_id: OwnedRoomId) -> AppResult<()> {
    let user_id = parse_active_local_user_id(&user_id).await?;

    let tags_event = self
        .services
        .account_data
        .get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
        .await
        .unwrap_or(TagEvent {
            content: TagEventContent { tags: BTreeMap::new() },
        });

    self.write_str(&format!("```\n{:#?}\n```", tags_event.content.tags))
        .await
}

#[admin_command]
pub(super) async fn redact_event(&self, event_id: OwnedEventId) -> AppResult<()> {
    let Ok(event) = self.services.rooms.timeline.get_non_outlier_pdu(&event_id).await else {
        return Err(AppError::public("event does not exist in our database"));
    };

    if event.is_redacted() {
        return Err(AppError::public("event is already redacted"));
    }

    if !event.sender().is_local() {
        return Err(AppError::public("this command only works on local users"));
    }

    let reason = format!(
        "the administrator(s) of {} has redacted this user's message",
        config::server_name()
    );

    let redaction_event_id = {
        let state_lock = self.services.rooms.state.mutex.lock(event.room_id()).await;

        self.services
            .rooms
            .timeline
            .build_and_append_pdu(
                PduBuilder {
                    redacts: Some(event.event_id().to_owned()),
                    ..PduBuilder::timeline(&RoomRedactionEventContent {
                        redacts: Some(event.event_id().to_owned()),
                        reason: Some(reason),
                    })
                },
                event.sender(),
                event.room_id(),
                &state_lock,
            )
            .await?
    };

    self.write_str(&format!(
        "Successfully redacted event. Redaction event ID: {redaction_event_id}"
    ))
    .await
}
