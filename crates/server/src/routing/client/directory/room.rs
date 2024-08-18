use diesel::prelude::*;
use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::client::directory::SetRoomVisibilityReqBody;
use crate::core::client::directory::VisibilityResBody;
use crate::core::identifiers::*;
use crate::core::room::Visibility;
use crate::room::DbRoom;
use crate::schema::*;
use crate::{db, diesel_exists, AuthArgs};
use crate::{empty_ok, json_ok, EmptyResult, JsonResult};

// #GET /_matrix/client/r0/directory/list/room/{room_id}
/// Gets the visibility of a given room in the room directory.
#[endpoint]
pub(super) async fn get_visibility(_aa: AuthArgs, room_id: PathParam<OwnedRoomId>) -> JsonResult<VisibilityResBody> {
    let room_id = room_id.into_inner();
    let query = rooms::table
        .filter(rooms::id.eq(&room_id))
        .filter(rooms::is_public.eq(true));
    let visibility = if diesel_exists!(query, &mut *db::connect()?)? {
        Visibility::Public
    } else {
        Visibility::Private
    };

    json_ok(VisibilityResBody { visibility })
}
// #PUT /_matrix/client/r0/directory/list/room/{room_id}
/// Sets the visibility of a given room in the room directory.
///
/// - TODO: Access control checks
#[endpoint]
pub(super) async fn set_visibility(
    _aa: AuthArgs,
    room_id: PathParam<OwnedRoomId>,
    body: JsonBody<SetRoomVisibilityReqBody>,
) -> EmptyResult {
    let room_id = room_id.into_inner();
    let room = rooms::table.find(&room_id).first::<DbRoom>(&mut *db::connect()?)?;

    diesel::update(&room)
        .set(rooms::is_public.eq(body.visibility == Visibility::Public))
        .execute(&mut *db::connect()?)?;
    empty_ok()
}

#[endpoint]
pub(super) async fn set_visibility_with_network_id(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
