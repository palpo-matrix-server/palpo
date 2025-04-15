use diesel::prelude::*;
use rand::seq::SliceRandom;
use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::client::room::{AliasResBody, SetAliasReqBody};
use crate::core::federation::query::{RoomInfoResBody, directory_request};
use crate::core::identifiers::*;
use crate::data::connect;
use crate::data::schema::*;
use crate::exts::*;
use crate::{AppError, AuthArgs, EmptyResult, JsonResult, MatrixError, data, diesel_exists, empty_ok, json_ok};

/// #GET /_matrix/client/r0/directory/room/{room_alias}
/// Resolve an alias locally or over federation.
///
/// - TODO: Suggest more servers to join via
#[endpoint]
pub(super) async fn get_alias(_aa: AuthArgs, room_alias: PathParam<OwnedRoomAliasId>) -> JsonResult<AliasResBody> {
    println!("=================1111===get alias: {room_alias}");
    let room_alias = room_alias.into_inner();
    println!("vMMMMMMM   0");
    let Ok((room_id, servers)) = crate::room::resolve_alias(&room_alias, None).await else {
        println!("vMMMMMMM   1");
        return Err(MatrixError::not_found("Room with alias not found.").into());
    };
    println!("vMMMMMMM   2");
    let servers = crate::room::room_available_servers(&room_id, &room_alias, servers).await?;
    println!("vMMMMMMM   3");
    debug!(?room_alias, ?room_id, "available servers: {servers:?}");
    json_ok(AliasResBody::new(room_id, servers))
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

    if crate::room::resolve_local_alias(&alias_id).is_ok() {
        return Err(MatrixError::forbidden("Alias already exists.").into());
    }

    let query = room_aliases::table
        .filter(room_aliases::alias_id.eq(&alias_id))
        .filter(room_aliases::room_id.ne(&body.room_id));
    if diesel_exists!(query, &mut connect()?)? {
        return Err(StatusError::conflict()
            .brief("A room alias with that name already exists.")
            .into());
    }

    crate::room::set_alias(body.room_id.clone(), alias_id, authed.user_id())?;

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
