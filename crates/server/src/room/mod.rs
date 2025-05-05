pub mod alias;
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
use rand::seq::SliceRandom;

use crate::appservice::RegistrationInfo;
use crate::core::directory::RoomTypeFilter;
use crate::core::events::room::guest_access::{GuestAccess, RoomGuestAccessEventContent};
use crate::core::events::room::member::MembershipState;
use crate::core::events::{AnySyncStateEvent, StateEventType};
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{Seqnum, UnixMillis};
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::{APPSERVICE_IN_ROOM_CACHE, AppResult, IsRemoteOrLocal, config};

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
        .execute(&mut connect()?)?;
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
    diesel_exists!(rooms::table.filter(rooms::id.eq(room_id)), &mut connect()?).map_err(Into::into)
}

pub fn get_room_sn(room_id: &RoomId) -> AppResult<Seqnum> {
    let room_sn = rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::sn)
        .first::<Seqnum>(&mut connect()?)?;
    Ok(room_sn)
}

pub fn is_disabled(room_id: &RoomId) -> AppResult<bool> {
    rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::disabled)
        .first(&mut connect()?)
        .map_err(Into::into)
}

pub fn disable_room(room_id: &RoomId, disabled: bool) -> AppResult<()> {
    diesel::update(rooms::table.filter(rooms::id.eq(room_id)))
        .set(rooms::disabled.eq(disabled))
        .execute(&mut connect()?)
        .map(|_| ())
        .map_err(Into::into)
}

pub fn guest_can_join(room_id: &RoomId) -> AppResult<bool> {
    self::state::get_room_state_content::<RoomGuestAccessEventContent>(&room_id, &StateEventType::RoomGuestAccess, "", None)
        .map(|c| c.guest_access == GuestAccess::CanJoin)
}

pub fn update_room_currents(room_id: &RoomId) -> AppResult<()> {
    let joined_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .count()
        .get_result::<i64>(&mut connect()?)?;
    let invited_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("invite"))
        .count()
        .get_result::<i64>(&mut connect()?)?;
    let left_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("leave"))
        .count()
        .get_result::<i64>(&mut connect()?)?;
    let banned_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("banned"))
        .count()
        .get_result::<i64>(&mut connect()?)?;
    let knocked_members = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("knocked"))
        .count()
        .get_result::<i64>(&mut connect()?)?;

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
        .execute(&mut connect()?)?;

    Ok(())
}

pub fn update_room_servers(room_id: &RoomId) -> AppResult<()> {
    let joined_servers = room_users::table
        .filter(room_users::room_id.eq(room_id))
        .filter(room_users::membership.eq("join"))
        .select(room_users::user_id)
        .distinct()
        .load::<OwnedUserId>(&mut connect()?)?
        .into_iter()
        .map(|user_id| user_id.server_name().to_owned())
        .collect::<Vec<OwnedServerName>>();

    diesel::delete(
        room_servers::table
            .filter(room_servers::room_id.eq(room_id))
            .filter(room_servers::server_id.ne_all(&joined_servers)),
    )
    .execute(&mut connect()?)?;

    for joined_server in joined_servers {
        diesel::insert_into(room_servers::table)
            .values((
                room_servers::room_id.eq(room_id),
                room_servers::server_id.eq(&joined_server),
            ))
            .on_conflict_do_nothing()
            .execute(&mut connect()?)?;
    }

    Ok(())
}
pub fn get_our_real_users(room_id: &RoomId) -> AppResult<Vec<OwnedUserId>> {
    room_users::table
        .filter(room_users::room_id.eq(room_id))
        .select(room_users::user_id)
        .load::<OwnedUserId>(&mut connect()?)
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
            UserId::parse_with_server_name(appservice.registration.sender_localpart.as_str(), config::server_name())
                .ok();

        let in_room = bridge_user_id.map_or(false, |id| is_joined(&id, room_id).unwrap_or(false)) || {
            let user_ids = room_users::table
                .filter(room_users::room_id.eq(room_id))
                .select(room_users::user_id)
                .load::<String>(&mut connect()?)?;
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
        &mut connect()?
    )
    .map_err(Into::into)
}
pub fn local_work_for_room(room_id: &RoomId, servers: &[OwnedServerName]) -> AppResult<bool> {
    let local = is_server_in_room(config::server_name(), room_id)?
        || servers.is_empty()
        || (servers.len() == 1 && servers[0].is_local());
    Ok(local)
}
pub fn is_server_in_room(server: &ServerName, room_id: &RoomId) -> AppResult<bool> {
    // if server
    //     == room_id
    //         .server_name()
    //         .map_err(|_| AppError::internal("bad room server name."))?
    // {
    //     return Ok(true);
    // }
    let query = room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .filter(room_servers::server_id.eq(server));
    diesel_exists!(query, &mut connect()?).map_err(Into::into)
}
pub fn room_servers(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .select(room_servers::server_id)
        .load::<OwnedServerName>(&mut connect()?)
        .map_err(Into::into)
}

pub fn joined_member_count(room_id: &RoomId) -> AppResult<u64> {
    stats_room_currents::table
        .find(room_id)
        .select(stats_room_currents::joined_members)
        .first::<i64>(&mut connect()?)
        .optional()
        .map(|c| c.unwrap_or_default() as u64)
        .map_err(Into::into)
}

#[tracing::instrument]
pub fn invited_member_count(room_id: &RoomId) -> AppResult<u64> {
    stats_room_currents::table
        .find(room_id)
        .select(stats_room_currents::invited_members)
        .first::<i64>(&mut connect()?)
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
        .load::<(OwnedRoomId, OwnedEventId)>(&mut connect()?)
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
            .load::<JsonValue>(&mut connect()?)?
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
            .load(&mut connect()?)
            .map_err(Into::into)
    } else {
        room_users::table
            .filter(room_users::room_id.eq(room_id))
            .filter(room_users::membership.eq(MembershipState::Join.to_string()))
            .select(room_users::user_id)
            .load(&mut connect()?)
            .map_err(Into::into)
    }
}

/// Returns an iterator of all servers participating in this room.
pub fn participating_servers(room_id: &RoomId) -> AppResult<Vec<OwnedServerName>> {
    room_servers::table
        .filter(room_servers::room_id.eq(room_id))
        .select(room_servers::server_id)
        .load(&mut connect()?)
        .map_err(Into::into)
}

pub fn public_room_ids() -> AppResult<Vec<OwnedRoomId>> {
    rooms::table
        .filter(rooms::is_public.eq(true))
        .select(rooms::id)
        .load(&mut connect()?)
        .map_err(Into::into)
}

pub fn server_rooms(server_name: &ServerName) -> AppResult<Vec<OwnedRoomId>> {
    room_servers::table
        .filter(room_servers::server_id.eq(server_name))
        .select(room_servers::room_id)
        .load::<OwnedRoomId>(&mut connect()?)
        .map_err(Into::into)
}

pub fn room_version(room_id: &RoomId) -> AppResult<RoomVersionId> {
    let room_version = rooms::table
        .filter(rooms::id.eq(room_id))
        .select(rooms::version)
        .first::<String>(&mut connect()?)?;
    Ok(RoomVersionId::try_from(room_version)?)
}

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

pub async fn room_available_servers(
    room_id: &RoomId,
    room_alias: &RoomAliasId,
    pre_servers: Vec<OwnedServerName>,
) -> AppResult<Vec<OwnedServerName>> {
    // find active servers in room state cache to suggest
    let mut servers: Vec<OwnedServerName> = room_servers(room_id)?;

    // push any servers we want in the list already (e.g. responded remote alias
    // servers, room alias server itself)
    servers.extend(pre_servers);

    servers.sort_unstable();
    servers.dedup();

    // shuffle list of servers randomly after sort and dedupe
    servers.shuffle(&mut rand::rng());

    // insert our server as the very first choice if in list, else check if we can
    // prefer the room alias server first
    match servers.iter().position(|server_name| server_name.is_local()) {
        Some(server_index) => {
            servers.swap_remove(server_index);
            servers.insert(0, config::server_name().to_owned());
        }
        _ => match servers.iter().position(|server| server == room_alias.server_name()) {
            Some(alias_server_index) => {
                servers.swap_remove(alias_server_index);
                servers.insert(0, room_alias.server_name().into());
            }
            _ => {}
        },
    }

    Ok(servers)
}
