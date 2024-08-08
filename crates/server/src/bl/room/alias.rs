use diesel::prelude::*;
use rand::seq::SliceRandom;

use crate::core::identifiers::*;
use crate::core::UnixMillis;

use crate::core::client::room::AliasResBody;
use crate::schema::*;
use crate::{db, AppError, AppResult, MatrixError};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_aliases, primary_key(alias))]
pub struct RoomAlias {
    pub alias: OwnedRoomAliasId,
    pub room_id: OwnedRoomId,
    pub created_by: OwnedUserId,
    pub created_at: UnixMillis,
}

pub fn local_aliases_for_room(room_id: &RoomId) -> AppResult<Vec<OwnedRoomAliasId>> {
    room_aliases::table
        .filter(room_aliases::room_id.eq(room_id))
        .select(room_aliases::alias)
        .load::<OwnedRoomAliasId>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn resolve_local_alias(alias: &RoomAliasId) -> AppResult<Option<OwnedRoomId>> {
    room_aliases::table
        .filter(room_aliases::alias.eq(alias))
        .select(room_aliases::room_id)
        .first::<String>(&mut *db::connect()?)
        .optional()?
        .map(|room_id| RoomId::parse(room_id).map_err(|_| AppError::public("Room ID is invalid.")))
        .transpose()
}

pub fn set_alias(
    room_id: impl Into<OwnedRoomId>,
    alias: impl Into<OwnedRoomAliasId>,
    created_by: impl Into<OwnedUserId>,
) -> AppResult<()> {
    diesel::insert_into(room_aliases::table)
        .values(RoomAlias {
            alias: alias.into(),
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
        let url = room_alias
            .server_name()
            .build_url(&format!("federation/v1/query/directory?room_alias={}", room_alias))?;
        let mut body: AliasResBody = crate::sending::get(url).send().await?;

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
