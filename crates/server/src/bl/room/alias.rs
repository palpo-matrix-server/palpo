use diesel::prelude::*;
use rand::seq::SliceRandom;
use serde_json::value::to_raw_value;

use crate::core::client::room::AliasResBody;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use crate::core::events::TimelineEventType;
use crate::core::federation::query::directory_request;
use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::room::StateEventType;
use crate::schema::*;
use crate::user::DbUser;
use crate::{db, diesel_exists, AppError, AppResult, MatrixError, PduBuilder};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_aliases, primary_key(alias_id))]
pub struct DbRoomAlias {
    pub alias_id: OwnedRoomAliasId,
    pub room_id: OwnedRoomId,
    pub created_by: OwnedUserId,
    pub created_at: UnixMillis,
}

pub fn local_aliases_for_room(room_id: &RoomId) -> AppResult<Vec<OwnedRoomAliasId>> {
    room_aliases::table
        .filter(room_aliases::room_id.eq(room_id))
        .select(room_aliases::alias_id)
        .load::<OwnedRoomAliasId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn resolve_local_alias(alias_id: &RoomAliasId) -> AppResult<Option<OwnedRoomId>> {
    room_aliases::table
        .filter(room_aliases::alias_id.eq(alias_id))
        .select(room_aliases::room_id)
        .first::<String>(&mut *db::connect()?)
        .optional()?
        .map(|room_id| RoomId::parse(room_id).map_err(|_| AppError::public("Room ID is invalid.")))
        .transpose()
}

pub fn set_alias(
    room_id: impl Into<OwnedRoomId>,
    alias_id: impl Into<OwnedRoomAliasId>,
    created_by: impl Into<OwnedUserId>,
) -> AppResult<()> {
    diesel::insert_into(room_aliases::table)
        .values(DbRoomAlias {
            alias_id: alias_id.into(),
            room_id: room_id.into(),
            created_by: created_by.into(),
            created_at: UnixMillis::now(),
        })
        .execute(&mut db::connect()?)
        .map(|_| ())
        .map_err(Into::into)
}

pub async fn get_alias_response(room_alias: OwnedRoomAliasId) -> AppResult<AliasResBody> {
    if room_alias.server_name() != crate::server_name() {
        let mut body: AliasResBody = directory_request(&room_alias)?.send().await?;

        body.servers.shuffle(&mut rand::thread_rng());

        return Ok(body);
    }

    let mut room_id = None;
    match crate::room::resolve_local_alias(&room_alias)? {
        Some(r) => room_id = Some(r),
        None => {
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
                        crate::room::resolve_local_alias(&room_alias)?
                            .ok_or_else(|| AppError::public("Appservice lied to us. Room does not exist."))?,
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
    let Some(room_id) = crate::room::resolve_local_alias(alias_id)? else {
        return Err(MatrixError::not_found("Alias not found.").into());
    };
    if user_can_remove_alias(alias_id, user)? {
        let state_alias = crate::room::state::get_state(&room_id, &StateEventType::RoomCanonicalAlias, "")?;
        diesel::delete(room_aliases::table.find(&alias_id)).execute(&mut *db::connect()?)?;

        if state_alias.is_some() {
            crate::room::timeline::build_and_append_pdu(
                PduBuilder {
                    event_type: TimelineEventType::RoomCanonicalAlias,
                    content: to_raw_value(&RoomCanonicalAliasEventContent {
                        alias: None,
                        alt_aliases: vec![], // TODO
                    })
                    .expect("We checked that alias earlier, it must be fine"),
                    unsigned: None,
                    state_key: Some("".to_owned()),
                    redacts: None,
                },
                &user.id,
                &room_id,
            )
            .ok();
        }

        Ok(())
    } else {
        Err(MatrixError::forbidden("User is not permitted to remove this alias.").into())
    }
}
#[tracing::instrument]
fn user_can_remove_alias(alias_id: &RoomAliasId, user: &DbUser) -> AppResult<bool> {
    let Some(room_id) = crate::room::resolve_local_alias(alias_id)? else {
        return Err(MatrixError::not_found("Alias not found.").into());
    };

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
    } else if let Some(event) = crate::room::state::get_state(&room_id, &StateEventType::RoomPowerLevels, "")? {
        serde_json::from_str(event.content.get())
            .map_err(|_| AppError::public("Invalid event content for m.room.power_levels"))
            .map(|content: RoomPowerLevelsEventContent| {
                RoomPowerLevels::from(content).user_can_send_state(&user.id, StateEventType::RoomCanonicalAlias)
            })
    // If there is no power levels event, only the room creator can change canonical aliases
    } else if let Some(event) = crate::room::state::get_state(&room_id, &StateEventType::RoomCreate, "")? {
        Ok(event.sender == user.id)
    } else {
        error!("Room {} has no m.room.create event (VERY BAD)!", room_id);
        Err(AppError::public("Room has no m.room.create event"))
    }
}
