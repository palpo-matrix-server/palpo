use diesel::prelude::*;
use rand::seq::SliceRandom;
use serde_json::value::to_raw_value;

use crate::bl::exts::*;
use crate::core::UnixMillis;
use crate::core::client::room::AliasResBody;
use crate::core::events::TimelineEventType;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use crate::core::federation::query::directory_request;
use crate::core::identifiers::*;
use crate::room::StateEventType;
use crate::user::DbUser;
use crate::{AppError, AppResult, MatrixError, PduBuilder, db};
use crate::{GetUrlOrigin, schema::*};

mod remote;
use remote::remote_resolve;

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_aliases, primary_key(alias_id))]
pub struct DbRoomAlias {
    pub alias_id: OwnedRoomAliasId,
    pub room_id: OwnedRoomId,
    pub created_by: OwnedUserId,
    pub created_at: UnixMillis,
}

#[inline]
pub async fn resolve(room: &RoomOrAliasId) -> AppResult<OwnedRoomId> {
    resolve_with_servers(room, None).await.map(|(room_id, _)| room_id)
}

pub async fn resolve_with_servers(
    room: &RoomOrAliasId,
    servers: Option<Vec<OwnedServerName>>,
) -> AppResult<(OwnedRoomId, Vec<OwnedServerName>)> {
    if room.is_room_id() {
        let room_id: &RoomId = room.try_into().expect("valid RoomId");
        Ok((room_id.to_owned(), servers.unwrap_or_default()))
    } else {
        let alias: &RoomAliasId = room.try_into().expect("valid RoomAliasId");
        resolve_alias(alias, servers).await
    }
}

#[tracing::instrument(name = "resolve")]
pub async fn resolve_alias(
    room_alias: &RoomAliasId,
    servers: Option<Vec<OwnedServerName>>,
) -> AppResult<(OwnedRoomId, Vec<OwnedServerName>)> {
    let server_name = room_alias.server_name();
    let is_local_server = server_name.is_local();
    let servers_contains_local = || {
        servers
            .as_ref()
            .is_some_and(|servers| servers.contains(&server_name.to_owned()))
    };

    if !is_local_server && !servers_contains_local() {
        return remote_resolve(room_alias, servers.unwrap_or_default()).await;
    }

    let room_id = match resolve_local_alias(room_alias) {
        Ok(r) => r,
        Err(_) => resolve_appservice_alias(room_alias).await?,
    };

    Ok((room_id, Vec::new()))
}

#[tracing::instrument(level = "debug")]
pub fn resolve_local_alias(alias_id: &RoomAliasId) -> AppResult<OwnedRoomId> {
    let room_id = room_aliases::table
        .filter(room_aliases::alias_id.eq(alias_id))
        .select(room_aliases::room_id)
        .first::<String>(&mut *db::connect()?)?;

    println!("======resolve_local_alias==========alias id: {alias_id}  room_id:  {room_id:?}");
    RoomId::parse(room_id).map_err(|_| AppError::public("Room ID is invalid."))
}

async fn resolve_appservice_alias(room_alias: &RoomAliasId) -> AppResult<OwnedRoomId> {
    for appservice in crate::appservice::all()?.values() {
        let url = appservice
            .registration
            .build_url(&format!("app/v1/rooms/{}", room_alias))?;
        if appservice.aliases.is_match(room_alias.as_str())
            && matches!(crate::sending::post(url).send::<Option<()>>().await, Ok(_opt_result))
        {
            return resolve_local_alias(room_alias).map_err(|_| MatrixError::not_found("Room does not exist.").into());
        }
    }

    Err(MatrixError::not_found("resolve appservice alias not found").into())
}

pub fn local_aliases_for_room(room_id: &RoomId) -> AppResult<Vec<OwnedRoomAliasId>> {
    room_aliases::table
        .filter(room_aliases::room_id.eq(room_id))
        .select(room_aliases::alias_id)
        .load::<OwnedRoomAliasId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn is_admin_room(room_id: &RoomId) -> bool {
    admin_room_id().map_or(false, |admin_room_id| admin_room_id == room_id)
}

pub fn admin_room_id() -> AppResult<OwnedRoomId> {
    let server_name = crate::server_name();
    crate::room::resolve_local_alias(
        <&RoomAliasId>::try_from(format!("#admins:{}", server_name).as_str())
            .expect("#admins:server_name is a valid room alias"),
    )
}

pub fn set_alias(
    room_id: impl Into<OwnedRoomId>,
    alias_id: impl Into<OwnedRoomAliasId>,
    created_by: impl Into<OwnedUserId>,
) -> AppResult<()> {
    let alias_id = alias_id.into();
    let room_id = room_id.into();

    diesel::insert_into(room_aliases::table)
        .values(DbRoomAlias {
            alias_id,
            room_id,
            created_by: created_by.into(),
            created_at: UnixMillis::now(),
        })
        .on_conflict_do_nothing()
        .execute(&mut db::connect()?)
        .map(|_| ())
        .map_err(Into::into)
}

pub async fn get_alias_response(room_alias: OwnedRoomAliasId) -> AppResult<AliasResBody> {
    if room_alias.server_name() != crate::server_name() {
        let request = directory_request(&room_alias.server_name().origin().await, &room_alias)?.into_inner();
        let mut body = crate::sending::send_federation_request(room_alias.server_name(), request)
            .await?
            .json::<AliasResBody>()
            .await?;

        body.servers.shuffle(&mut rand::thread_rng());

        return Ok(body);
    }

    let mut room_id = None;
    match crate::room::resolve_local_alias(&room_alias) {
        Ok(r) => room_id = Some(r),
        Err(_) => {
            for appservice in crate::appservice::all()?.values() {
                let url = appservice
                    .registration
                    .build_url(&format!("app/v1/rooms/{}", room_alias))?;
                if appservice.aliases.is_match(room_alias.as_str())
                    && matches!(
                        crate::sending::post(url).send::<Option<()>>().await,
                        Ok(Some(_opt_result))
                    )
                {
                    room_id = Some(
                        crate::room::resolve_local_alias(&room_alias)
                            .map_err(|_| AppError::public("Appservice lied to us. Room does not exist."))?,
                    );
                    break;
                }
            }
        }
    };

    let room_id = match room_id {
        Some(room_id) => room_id,
        None => return Err(MatrixError::not_found("Room with alias not found.").into()),
    };

    Ok(AliasResBody::new(room_id, vec![crate::server_name().to_owned()]))
}

#[tracing::instrument]
pub fn remove_alias(alias_id: &RoomAliasId, user: &DbUser) -> AppResult<()> {
    let room_id = crate::room::resolve_local_alias(alias_id)?;
    if user_can_remove_alias(alias_id, user)? {
        let state_alias = crate::room::state::get_canonical_alias(&room_id);

        if state_alias.is_ok() {
            crate::room::timeline::build_and_append_pdu(
                PduBuilder {
                    event_type: TimelineEventType::RoomCanonicalAlias,
                    content: to_raw_value(&RoomCanonicalAliasEventContent {
                        alias: None,
                        alt_aliases: vec![], // TODO
                    })
                    .expect("We checked that alias earlier, it must be fine"),
                    state_key: Some("".to_owned()),
                    ..Default::default()
                },
                &user.id,
                &room_id,
            )
            .ok();
        }
        diesel::delete(room_aliases::table.filter(room_aliases::alias_id.eq(alias_id))).execute(&mut *db::connect()?)?;
        
        Ok(())
    } else {
        Err(MatrixError::forbidden("User is not permitted to remove this alias.").into())
    }
}
#[tracing::instrument]
fn user_can_remove_alias(alias_id: &RoomAliasId, user: &DbUser) -> AppResult<bool> {
    let room_id = crate::room::resolve_local_alias(alias_id)?;

    let alias = room_aliases::table
        .find(alias_id)
        .first::<DbRoomAlias>(&mut *db::connect()?)?;

    // The creator of an alias can remove it
    if alias.created_by == user.id
        // Server admins can remove any local alias
        || user.is_admin
        // Always allow the Palpo user to remove the alias, since there may not be an admin room
        || crate::server_user()== user.id
    {
        Ok(true)
        // Checking whether the user is able to change canonical aliases of the room
    } else if let Ok(content) = crate::room::state::get_room_state_content::<RoomPowerLevelsEventContent>(
        &room_id,
        &StateEventType::RoomPowerLevels,
        "",
    ) {
        Ok(RoomPowerLevels::from(content).user_can_send_state(&user.id, StateEventType::RoomCanonicalAlias))
    // If there is no power levels event, only the room creator can change canonical aliases
    } else if let Ok(event) = crate::room::state::get_room_state(&room_id, &StateEventType::RoomCreate, "") {
        Ok(event.sender == user.id)
    } else {
        error!("Room {} has no m.room.create event (VERY BAD)!", room_id);
        Err(AppError::public("Room has no m.room.create event"))
    }
}
