use diesel::prelude::*;
use rand::seq::SliceRandom;
use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::client::room::{AliasResBody, SetAliasReqBody};
use crate::core::federation::query::RoomInfoResBody;
use crate::core::federation::query::directory_request;
use crate::core::identifiers::*;
use crate::exts::*;
use crate::schema::*;
use crate::{AppError, AuthArgs, EmptyResult, JsonResult, MatrixError, db, diesel_exists, empty_ok, json_ok};

/// #GET /_matrix/client/r0/directory/room/{room_alias}
/// Resolve an alias locally or over federation.
///
/// - TODO: Suggest more servers to join via
#[endpoint]
pub(super) async fn get_alias(_aa: AuthArgs, room_alias: PathParam<OwnedRoomAliasId>) -> JsonResult<AliasResBody> {
    println!("GGGGGGet alias zzz");
    let room_alias = room_alias.into_inner();
    println!("GGGGGGet alias   0: {:?}", room_alias);
    if room_alias.is_remote() {
        println!("GGGGGGet alias   1  {}", room_alias.server_name().origin().await);
        let response = directory_request(&room_alias.server_name().origin().await, &room_alias)?
            .send::<RoomInfoResBody>()
            .await?;

        let mut servers = response.servers;
        servers.shuffle(&mut rand::thread_rng());

        return json_ok(AliasResBody::new(response.room_id, servers));
    }

    println!("GGGGGGet alias   2");
    let mut room_id = None;
    if let Ok(r) = crate::room::resolve_local_alias(&room_alias) {
        println!("GGGGGGet alias   2   0  {room_alias}");
        room_id = Some(r);
    } else {
        println!("GGGGGGet alias   2   1");
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
                    crate::room::resolve_local_alias(&room_alias)
                        .map_err(|_| AppError::public("Appservice lied to us. Room does not exist."))?,
                );
                break;
            }
        }
    }

    println!("GGGGGGet alias   3");
    let Some(room_id) = room_id else {
        return Err(MatrixError::not_found("Room with alias not found.").into());
    };
    json_ok(AliasResBody::new(room_id, vec![crate::server_name().to_owned()]))
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
    if diesel_exists!(query, &mut *db::connect()?)? {
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
