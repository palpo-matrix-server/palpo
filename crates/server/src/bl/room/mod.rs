mod alias;
pub use alias::*;
pub mod auth_chain;
mod current;
pub mod directory;
pub mod lazy_loading;
pub mod pdu_metadata;
pub mod receipt;
pub mod space;
pub mod state;
pub mod timeline;
pub mod typing;
pub mod user;
pub use current::*;
pub use user::*;
pub mod thread;

use std::collections::HashMap;

use diesel::prelude::*;

use crate::appservice::RegistrationInfo;
use crate::core::directory::RoomTypeFilter;
use crate::core::events::direct::DirectEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::guest_access::{GuestAccess, RoomGuestAccessEventContent};
use crate::core::events::room::member::MembershipState;
use crate::core::events::{
    AnyStrippedStateEvent, AnySyncStateEvent, GlobalAccountDataEventType, RoomAccountDataEventType, StateEventType,
};
use crate::core::identifiers::*;
use crate::core::room::RoomType;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{Seqnum, UnixMillis};
use crate::schema::*;
use crate::{APPSERVICE_IN_ROOM_CACHE, AppError, AppResult, db, diesel_exists};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = rooms)]
pub struct DbRoom {
    pub id: OwnedRoomId,
    pub sn: Seqnum,
    pub version: String,
    pub is_public: bool,
    pub min_depth: i64,
    pub state_frame_id: Option<i64>,
    pub has_auth_chain_index: bool,
    pub disabled: bool,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = rooms)]
pub struct NewDbRoom {
    pub id: OwnedRoomId,
    pub version: String,
    pub is_public: bool,
    pub min_depth: i64,
    pub has_auth_chain_index: bool,
    pub created_at: UnixMillis,
}

#[derive(Insertable, Identifiable, Queryable, AsChangeset, Debug, Clone)]
#[diesel(table_name = stats_room_currents, primary_key(room_id))]
pub struct DbRoomCurrent {
    pub room_id: OwnedRoomId,
    pub state_events: i64,
    pub joined_members: i64,
    pub invited_members: i64,
    pub left_members: i64,
    pub banned_members: i64,
    pub knocked_members: i64,
    pub local_users_in_room: i64,
    pub completed_delta_stream_id: i64,
}

pub fn create_room(new_room: NewDbRoom) -> AppResult<OwnedRoomId> {
    diesel::insert_into(rooms::table)
        .values(&new_room)
        .execute(&mut db::connect()?)?;
    Ok(new_room.id)
}

pub fn ensure_room(id: &RoomId, room_version_id: &RoomVersionId) -> AppResult<OwnedRoomId> {
    if room_exists(id)? {
        Ok(id.to_owned())
    } else {
        create_room(NewDbRoom {
            id: id.to_owned(),
            version: room_version_id.to_string(),
            is_public: false,
            min_depth: 0,
            has_auth_chain_index: false,
            created_at: UnixMillis::now(),
        })
        .map_err(Into::into)
    }
}

/// Checks if a room exists.
pub fn room_exists(room_id: &RoomId) -> AppResult<bool> {
    diesel_exists!(rooms::table.filter(rooms::id.eq(room_id)), &mut *db::connect()?).map_err(Into::into)
}

pub fn get_room_sn(room_id: &RoomId) -> AppResult<Seqnum> {
    let room_sn = rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::sn)
        .first::<Seqnum>(&mut *db::connect()?)?;
    Ok(room_sn)
}

pub fn is_disabled(room_id: &RoomId) -> AppResult<bool> {
    rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::disabled)
        .first(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn disable_room(room_id: &RoomId, disabled: bool) -> AppResult<()> {
    diesel::update(rooms::table.filter(rooms::id.eq(room_id)))
        .set(rooms::disabled.eq(disabled))
        .execute(&mut db::connect()?)
        .map(|_| ())
        .map_err(Into::into)
}

pub fn guest_can_join(room_id: &RoomId) -> AppResult<bool> {
    self::state::get_room_state_content::<RoomGuestAccessEventContent>(&room_id, &StateEventType::RoomGuestAccess, "")
        .map(|c| c.guest_access == GuestAccess::CanJoin)
}

/// Update current membership data.
#[tracing::instrument(skip(last_state))]
pub fn update_membership(
    event_id: &EventId,
    event_sn: i64,
    room_id: &RoomId,
    user_id: &UserId,
    membership: MembershipState,
    sender_id: &UserId,
    last_state: Option<Vec<RawJson<AnyStrippedStateEvent>>>,
) -> AppResult<()> {
    let conf = crate::config();
    // Keep track what remote users exist by adding them as "deactivated" users
    if user_id.server_name() != &conf.server_name && !crate::user::user_exists(user_id)? {
        crate::user::create_user(user_id, None)?;
        // TODO: display_name, avatar url
    }

    let state_data = if let Some(last_state) = last_state {
        Some(serde_json::to_value(last_state)?)
    } else {
        None
    };

    match &membership {
        MembershipState::Join => {
            // Check if the user never joined this room
            if !once_joined(user_id, room_id)? {
                // Add the user ID to the join list then
                // db::mark_as_once_joined(user_id, room_id)?;

                // Check if the room has a predecessor
                if let Ok(Some(predecessor)) = crate::room::state::get_room_state_content::<RoomCreateEventContent>(
                    room_id,
                    &StateEventType::RoomCreate,
                    "",
                )
                .map(|c| c.predecessor)
                {
                    // Copy user settings from predecessor to the current room:
                    // - Push rules
                    //
                    // TODO: finish this once push rules are implemented.
                    //
                    // let mut push_rules_event_content: PushRulesEvent = account_data
                    //     .get(
                    //         None,
                    //         user_id,
                    //         EventType::PushRules,
                    //     )?;
                    //
                    // NOTE: find where `predecessor.room_id` match
                    //       and update to `room_id`.
                    //
                    // account_data
                    //     .update(
                    //         None,
                    //         user_id,
                    //         EventType::PushRules,
                    //         &push_rules_event_content,
                    //         globals,
                    //     )
                    //     .ok();

                    // Copy old tags to new room
                    if let Some(tag_event_content) = crate::user::get_room_data::<JsonValue>(
                        user_id,
                        &predecessor.room_id,
                        &RoomAccountDataEventType::Tag.to_string(),
                    )? {
                        crate::user::set_data(
                            user_id,
                            Some(room_id.to_owned()),
                            &RoomAccountDataEventType::Tag.to_string(),
                            tag_event_content,
                        )
                        .ok();
                    };

                    // Copy direct chat flag
                    if let Some(mut direct_event_content) = crate::user::get_data::<DirectEventContent>(
                        user_id,
                        None,
                        &GlobalAccountDataEventType::Direct.to_string(),
                    )? {
                        let mut room_ids_updated = false;

                        for room_ids in direct_event_content.0.values_mut() {
                            if room_ids.iter().any(|r| r == &predecessor.room_id) {
                                room_ids.push(room_id.to_owned());
                                room_ids_updated = true;
                            }
                        }

                        if room_ids_updated {
                            crate::user::set_data(
                                user_id,
                                None,
                                &GlobalAccountDataEventType::Direct.to_string(),
                                serde_json::to_value(&direct_event_content)?,
                            )?;
                        }
                    };
                }
            }
            db::connect()?.transaction::<_, AppError, _>(|conn| {
                // let forgotten = room_users::table
                //     .filter(room_users::room_id.eq(room_id))
                //     .filter(room_users::user_id.eq(user_id))
                //     .select(room_users::forgotten)
                //     .first::<bool>(conn)
                //     .optional()?
                //     .unwrap_or_default();
                diesel::delete(
                    room_users::table
                        .filter(room_users::room_id.eq(room_id))
                        .filter(room_users::user_id.eq(user_id)),
                )
                .execute(conn)?;
                diesel::insert_into(room_users::table)
                    .values(&NewDbRoomUser {
                        room_id: room_id.to_owned(),
                        room_server_id: room_id
                            .server_name()
                            .map_err(|s| AppError::public(format!("bad room server name: {}", s)))?
                            .to_owned(),
                        user_id: user_id.to_owned(),
                        user_server_id: user_id.server_name().to_owned(),
                        event_id: event_id.to_owned(),
                        event_sn,
                        sender_id: sender_id.to_owned(),
                        membership: membership.to_string(),
                        forgotten: false,
                        display_name: None,
                        avatar_url: None,
                        state_data,
                        created_at: UnixMillis::now(),
                    })
                    .execute(conn)?;
                Ok(())
            })?;
        }
        MembershipState::Invite => {
            // We want to know if the sender is ignored by the receiver
            if crate::user::user_is_ignored(sender_id, user_id) {
                return Ok(());
            }

            db::connect()?.transaction::<_, AppError, _>(|conn| {
                // let forgotten = room_users::table
                //     .filter(room_users::room_id.eq(room_id))
                //     .filter(room_users::user_id.eq(user_id))
                //     .select(room_users::forgotten)
                //     .first::<bool>(conn)
                //     .optional()?
                //     .unwrap_or_default();
                diesel::delete(
                    room_users::table
                        .filter(room_users::room_id.eq(room_id))
                        .filter(room_users::user_id.eq(user_id)),
                )
                .execute(conn)?;
                diesel::insert_into(room_users::table)
                    .values(&NewDbRoomUser {
                        room_id: room_id.to_owned(),
                        room_server_id: room_id
                            .server_name()
                            .map_err(|s| AppError::public(format!("bad room server name: {}", s)))?
                            .to_owned(),
                        user_id: user_id.to_owned(),
                        user_server_id: user_id.server_name().to_owned(),
                        event_id: event_id.to_owned(),
                        event_sn,
                        sender_id: sender_id.to_owned(),
                        membership: membership.to_string(),
                        forgotten: false,
                        display_name: None,
                        avatar_url: None,
                        state_data,
                        created_at: UnixMillis::now(),
                    })
                    .execute(conn)?;
                Ok(())
            })?;
        }
        MembershipState::Leave | MembershipState::Ban => {
            db::connect()?.transaction::<_, AppError, _>(|conn| {
                // let forgotten = room_users::table
                //     .filter(room_users::room_id.eq(room_id))
                //     .filter(room_users::user_id.eq(user_id))
                //     .select(room_users::forgotten)
                //     .first::<bool>(conn)
                //     .optional()?
                //     .unwrap_or_default();
                diesel::delete(
                    room_users::table
                        .filter(room_users::room_id.eq(room_id))
                        .filter(room_users::user_id.eq(user_id)),
                )
                .execute(conn)?;
                diesel::insert_into(room_users::table)
                    .values(&NewDbRoomUser {
                        room_id: room_id.to_owned(),
                        room_server_id: room_id
                            .server_name()
                            .map_err(|s| AppError::public(format!("bad room server name: {}", s)))?
                            .to_owned(),
                        user_id: user_id.to_owned(),
                        user_server_id: user_id.server_name().to_owned(),
                        event_id: event_id.to_owned(),
                        event_sn,
                        sender_id: sender_id.to_owned(),
                        membership: membership.to_string(),
                        forgotten: false,
                        display_name: None,
                        avatar_url: None,
                        state_data,
                        created_at: UnixMillis::now(),
                    })
                    .execute(conn)?;
                Ok(())
            })?;
        }
        _ => {}
    }
    update_room_servers(room_id)?;
    update_room_currents(room_id)?;
    Ok(())
}

pub fn update_room_currents(room_id: &RoomId) -> AppResult<()> {
    let joined_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .count()
        .get_result::<i64>(&mut *db::connect()?)?;
    let invited_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("invite"))
        .count()
        .get_result::<i64>(&mut *db::connect()?)?;
    let left_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("leave"))
        .count()
        .get_result::<i64>(&mut *db::connect()?)?;
    let banned_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("banned"))
        .count()
        .get_result::<i64>(&mut *db::connect()?)?;
    let knocked_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("knocked"))
        .count()
        .get_result::<i64>(&mut *db::connect()?)?;

    let current = DbRoomCurrent {
        room_id: room_id.to_owned(),
        state_events: 0, //TODO: fixme
        joined_members,
        invited_members,
        left_members,
        banned_members,
        knocked_members,
        local_users_in_room: 0,       //TODO: fixme
        completed_delta_stream_id: 0, //TODO: fixme
    };
    diesel::insert_into(stats_room_currents::table)
        .values(&current)
        .on_conflict(stats_room_currents::room_id)
        .do_update()
        .set(&current)
        .execute(&mut db::connect()?)?;

    Ok(())
}

pub fn update_room_servers(room_id: &RoomId) -> AppResult<()> {
    let joined_servers = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::user_id)
        .distinct()
        .load::<OwnedUserId>(&mut *db::connect()?)?
        .into_iter()
        .map(|user_id| user_id.server_name().to_owned())
        .collect::<Vec<OwnedServerName>>();

    diesel::delete(
        room_servers::table
            .filter(room_servers::room_id.eq(room_id))
            .filter(room_servers::server_id.ne_all(&joined_servers)),
    )
    .execute(&mut db::connect()?)?;

    for joined_server in joined_servers {
        diesel::insert_into(room_servers::table)
            .values((
                room_servers::room_id.eq(room_id),
                room_servers::server_id.eq(&joined_server),
            ))
            .on_conflict_do_nothing()
            .execute(&mut db::connect()?)?;
    }

    Ok(())
}
pub fn get_our_real_users(room_id: &RoomId) -> AppResult<Vec<OwnedUserId>> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::user_id)
        .load::<OwnedUserId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn appservice_in_room(room_id: &RoomId, appservice: &RegistrationInfo) -> AppResult<bool> {
    let maybe = APPSERVICE_IN_ROOM_CACHE
        .read()
        .unwrap()
        .get(room_id)
        .and_then(|map| map.get(&appservice.registration.id))
        .copied();

    if let Some(b) = maybe {
        Ok(b)
    } else {
        let bridge_user_id =
            UserId::parse_with_server_name(appservice.registration.sender_localpart.as_str(), crate::server_name())
                .ok();

        let in_room = bridge_user_id.map_or(false, |id| is_joined(&id, room_id).unwrap_or(false)) || {
            let user_ids = room_users::table
                .filter(room_users::room_id.eq(room_id))
                .select(room_users::user_id)
                .load::<String>(&mut *db::connect()?)?;
            user_ids
                .iter()
                .any(|user_id| appservice.users.is_match(user_id.as_str()))
        };

        APPSERVICE_IN_ROOM_CACHE
            .write()
            .unwrap()
            .entry(room_id.to_owned())
            .or_default()
            .insert(appservice.registration.id.clone(), in_room);

        Ok(in_room)
    }
}
pub fn is_room_exists(room_id: &RoomId) -> AppResult<bool> {
    diesel_exists!(
        rooms::table.filter(rooms::id.eq(room_id)).select(rooms::id),
        &mut *db::connect()?
    )
    .map_err(Into::into)
}
pub fn is_server_in_room(server: &ServerName, room_id: &RoomId) -> AppResult<bool> {
    if server
        == room_id
            .server_name()
            .map_err(|_| AppError::internal("bad room server name."))?
    {
        return Ok(true);
    }
    let query = room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .filter(room_servers::server_id.eq(server));
    diesel_exists!(query, &mut *db::connect()?).map_err(Into::into)
}
pub fn room_servers(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .select(room_servers::server_id)
        .load::<OwnedServerName>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn joined_member_count(room_id: &RoomId) -> AppResult<u64> {
    stats_room_currents::table
        .find(room_id)
        .select(stats_room_currents::joined_members)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map(|c| c.unwrap_or_default() as u64)
        .map_err(Into::into)
}

#[tracing::instrument]
pub fn invited_member_count(room_id: &RoomId) -> AppResult<u64> {
    stats_room_currents::table
        .find(room_id)
        .select(stats_room_currents::invited_members)
        .first::<i64>(&mut *db::connect()?)
        .optional()
        .map(|c| c.unwrap_or_default() as u64)
        .map_err(Into::into)
}

/// Returns an iterator over all rooms a user left.
#[tracing::instrument]
pub fn rooms_left(user_id: &UserId) -> AppResult<HashMap<OwnedRoomId, Vec<RawJson<AnySyncStateEvent>>>> {
    let room_event_ids = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .filter(room_users::membership.eq_any(vec![
            MembershipState::Leave.to_string(),
            MembershipState::Ban.to_string(),
        ]))
        .select((room_users::room_id, room_users::event_id))
        .load::<(OwnedRoomId, OwnedEventId)>(&mut *db::connect()?)
        .map(|rows| {
            let mut map: HashMap<OwnedRoomId, Vec<OwnedEventId>> = HashMap::new();
            for (room_id, event_id) in rows {
                map.entry(room_id).or_default().push(event_id);
            }
            map
        })?;
    let mut room_events = HashMap::new();
    for (room_id, event_ids) in room_event_ids {
        let events = event_datas::table
            .filter(event_datas::event_id.eq_any(&event_ids))
            .select(event_datas::json_data)
            .load::<JsonValue>(&mut *db::connect()?)?
            .into_iter()
            .filter_map(|value| RawJson::<AnySyncStateEvent>::from_value(&value).ok())
            .collect::<Vec<_>>();
        room_events.insert(room_id, events);
    }
    Ok(room_events)
}

pub fn get_joined_users(room_id: &RoomId, until_sn: Option<i64>) -> AppResult<Vec<OwnedUserId>> {
    if let Some(until_sn) = until_sn {
        room_users::table
            .filter(room_users::event_sn.le(until_sn))
            .filter(room_users::room_id.eq(room_id))
            .filter(room_users::membership.eq(MembershipState::Join.to_string()))
            .select(room_users::user_id)
            .load(&mut db::connect()?)
            .map_err(Into::into)
    } else {
        room_users::table
            .filter(room_users::room_id.eq(room_id))
            .filter(room_users::membership.eq(MembershipState::Join.to_string()))
            .select(room_users::user_id)
            .load(&mut db::connect()?)
            .map_err(Into::into)
    }
}

/// Returns an iterator of all servers participating in this room.
pub fn participating_servers(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .select(room_servers::server_id)
        .load(&mut db::connect()?)
        .map_err(Into::into)
}

pub fn public_room_ids() -> AppResult<Vec<OwnedRoomId>> {
    rooms::table
        .filter(rooms::is_public.eq(true))
        .select(rooms::id)
        .load(&mut db::connect()?)
        .map_err(Into::into)
}

pub fn server_rooms(server_name: &ServerName) -> AppResult<Vec<OwnedRoomId>> {
    room_servers::table
        .filter(room_servers::server_id.eq(server_name))
        .select(room_servers::room_id)
        .load::<OwnedRoomId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn room_version(room_id: &RoomId) -> AppResult<RoomVersionId> {
    let room_version = rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::version)
        .first::<String>(&mut *db::connect()?)?;
    Ok(RoomVersionId::try_from(room_version)?)
}
// pub fn get_room_sn(room_id: &RoomId) -> AppResult<Seqnum> {
//     let room_sn = rooms::table
//         .filter(rooms::id.eq(room_id))
//         .select(rooms::sn)
//         .first::<Seqnum>(&mut *db::connect()?)?;
//     Ok(room_sn)
// }

pub fn filter_rooms<'a>(rooms: &[&'a RoomId], filter: &[RoomTypeFilter], negate: bool) -> Vec<&'a RoomId> {
    rooms
        .iter()
        .filter_map(|r| {
            let r = *r;
            let room_type = state::get_room_type(r).ok()?;
            let room_type_filter = RoomTypeFilter::from(room_type);

            let include = if negate {
                !filter.contains(&room_type_filter)
            } else {
                filter.is_empty() || filter.contains(&room_type_filter)
            };

            include.then_some(r)
        })
        .collect()
}
