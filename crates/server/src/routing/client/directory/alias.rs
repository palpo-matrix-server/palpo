use diesel::prelude::*;
use rand::seq::SliceRandom;
use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::client::room::{AliasResBody, SetAliasReqBody};
use crate::core::federation::query::directory_request;
use crate::core::federation::query::RoomInfoResBody;
use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::exts::*;
use crate::room::DbRoomAlias;
use crate::schema::*;
use crate::{db, diesel_exists, empty_ok, json_ok, AppError, AuthArgs, EmptyResult, JsonResult, MatrixError};

/// #GET /_matrix/client/r0/directory/room/{room_alias}
/// Resolve an alias locally or over federation.
///
/// - TODO: Suggest more servers to join via
#[endpoint]
pub(super) async fn get_alias(_aa: AuthArgs, room_alias: PathParam<OwnedRoomAliasId>) -> JsonResult<AliasResBody> {
    let room_alias = room_alias.into_inner();
    if room_alias.is_remote() {
        let response = directory_request(&room_alias.server_name().origin().await, &room_alias)?
            .send::<RoomInfoResBody>()
            .await?;

        let mut servers = response.servers;
        servers.shuffle(&mut rand::thread_rng());

        return json_ok(AliasResBody::new(response.room_id, servers));
    }

    let mut room_id = None;
    if let Some(r) = crate::room::resolve_local_alias(&room_alias)? {
        room_id = Some(r);
    } else {
        for (_id, appservice) in crate::appservice::all()? {
            if appservice.aliases.is_match(room_alias.as_str())
                && crate::sending::get(
                    appservice
                        .registration
                        .build_url(&format!("app/v1/rooms/{}", room_alias))?,
                )
                .send::<RoomInfoResBody>()
                .await
                .is_ok()
            {
                room_id = Some(
                    crate::room::resolve_local_alias(&room_alias)?
                        .ok_or_else(|| AppError::public("Appservice lied to us. Room does not exist."))?,
                );
                break;
            }
        }
    }

    let Some(room_id) = room_id else {
        return Err(MatrixError::not_found("Room with alias not found.").into());
    };
    json_ok(AliasResBody::new(room_id, vec![crate::config().server_name.to_owned()]))
}

/// #PUT /_matrix/client/r0/directory/room/{room_alias}
/// Creates a new room alias on this server.
#[endpoint]
pub(super) async fn upsert_alias(
    _aa: AuthArgs,
    room_alias: PathParam<OwnedRoomAliasId>,
    body: JsonBody<SetAliasReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;
    let alias_id = room_alias.into_inner();
    if alias_id.is_remote() {
        return Err(MatrixError::invalid_param("Alias is from another server.").into());
    }

    let query = room_aliases::table
        .filter(room_aliases::alias_id.eq(&alias_id))
        .filter(room_aliases::room_id.ne(&body.room_id));
    if diesel_exists!(query, &mut *db::connect()?)? {
        return Err(StatusError::conflict()
            .brief("A room alias with that name already exists.")
            .into());
    }
    diesel::insert_into(room_aliases::table)
        .values(DbRoomAlias {
            alias_id,
            room_id: body.room_id.clone(),
            created_by: authed.user_id().to_owned(),
            created_at: UnixMillis::now(),
        })
        .on_conflict_do_nothing()
        .execute(&mut *db::connect()?)?;

    empty_ok()
}

/// #DELETE /_matrix/client/r0/directory/room/{room_alias}
/// Deletes a room alias from this server.
///
/// - TODO: additional access control checks
/// - TODO: Update canonical alias event
#[endpoint]
pub(super) async fn delete_alias(
    _aa: AuthArgs,
    room_alias: PathParam<OwnedRoomAliasId>,
    depot: &mut Depot,
) -> EmptyResult {
    let authed = depot.authed_info()?;

    let alias = room_alias.into_inner();
    if alias.is_remote() {
        return Err(MatrixError::invalid_param("Alias is from another server.").into());
    }

    crate::room::remove_alias(&alias, authed.user())?;

    // TODO: update alt_aliases?

    empty_ok()
}
