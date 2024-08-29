//! Endpoints to retrieve information from a homeserver about a resource.

use palpo_core::federation::query::ProfileReqArgs;
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::federation::query::RoomInfoResBody;
use crate::core::identifiers::*;
use crate::core::user::ProfileField;
use crate::core::user::ProfileResBody;
use crate::{empty_ok, json_ok, AuthArgs, EmptyResult, JsonResult, MatrixError};

pub fn router() -> Router {
    Router::with_path("query")
        .push(Router::with_path("profile").get(get_profile))
        .push(Router::with_path("directory").get(get_directory))
        .push(Router::with_path("<query_type>").get(query_by_type))
}

// #GET /_matrix/federation/v1/query/profile
/// Gets information on a profile.
#[endpoint]
async fn get_profile(_aa: AuthArgs, args: ProfileReqArgs) -> JsonResult<ProfileResBody> {
    let mut display_name = None;
    let mut avatar_url = None;
    let mut blurhash = None;

    let profile = crate::user::get_profile(&args.user_id, None)?.ok_or(MatrixError::not_found("Profile not found."))?;

    match &args.field {
        Some(ProfileField::DisplayName) => display_name = profile.display_name.clone(),
        Some(ProfileField::AvatarUrl) => {
            avatar_url = profile.avatar_url.clone();
            blurhash = profile.blurhash.clone();
        }
        // TODO: what to do with custom
        Some(_) => {}
        None => {
            display_name = profile.display_name.clone();
            avatar_url = profile.avatar_url.clone();
            blurhash = profile.blurhash.clone();
        }
    }

    json_ok(ProfileResBody {
        blurhash,
        display_name,
        avatar_url,
    })
}

// #GET /_matrix/federation/v1/query/directory
/// Resolve a room alias to a room id.
#[endpoint]
async fn get_directory(_aa: AuthArgs, room_alias: QueryParam<OwnedRoomAliasId, true>) -> JsonResult<RoomInfoResBody> {
    let room_id =
        crate::room::resolve_local_alias(&room_alias)?.ok_or(MatrixError::not_found("Room alias not found."))?;

    json_ok(RoomInfoResBody {
        room_id,
        servers: vec![crate::config().server_name.to_owned()],
    })
}
#[endpoint]
async fn query_by_type(_aa: AuthArgs) -> EmptyResult {
    // TODDO: todo
    empty_ok()
}
