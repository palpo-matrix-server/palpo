use diesel::prelude::*;
use palpo_core::UnixMillis;
use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde_json::value::to_raw_value;

use crate::core::client::profile::*;
use crate::core::events::room::member::RoomMemberEventContent;
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::query::{ProfileReqArgs, profile_request};
use crate::core::identifiers::*;
use crate::core::user::{ProfileField, ProfileResBody};
use crate::diesel_exists;
use crate::exts::*;
use crate::schema::*;
use crate::user::{DbProfile, NewDbPresence};
use crate::{AppError, AuthArgs, EmptyResult, JsonResult, MatrixError, PduBuilder, db, empty_ok, hoops, json_ok};

pub fn public_router() -> Router {
    Router::with_path("profile/{user_id}")
        .get(get_profile)
        .push(Router::with_path("avatar_url").get(get_avatar_url))
        .push(Router::with_path("displayname").get(get_display_name))
}
pub fn authed_router() -> Router {
    Router::with_path("profile/{user_id}")
        .hoop(hoops::limit_rate)
        .push(Router::with_path("avatar_url").put(set_avatar_url))
        .push(Router::with_path("displayname").put(set_display_name))
}

/// #GET /_matrix/client/r0/profile/{user_d}
/// Returns the display_name, avatar_url and blurhash of the user.
///
/// - If user is on another server: Fetches profile over federation
#[endpoint]
async fn get_profile(_aa: AuthArgs, user_id: PathParam<OwnedUserId>) -> JsonResult<ProfileResBody> {
    let user_id = user_id.into_inner();
    if user_id.is_remote() {
        let server_name = user_id.server_name().to_owned();
        let request =
            profile_request(&server_name.origin().await, ProfileReqArgs { user_id, field: None })?.into_inner();

        let profile = crate::sending::send_federation_request(&server_name, request)
            .await?
            .json::<ProfileResBody>()
            .await?;
        return json_ok(profile);
    }
    let DbProfile {
        blurhash,
        avatar_url,
        display_name,
        ..
    } = user_profiles::table
        .filter(user_profiles::user_id.eq(&user_id))
        .filter(user_profiles::room_id.is_null())
        .first::<DbProfile>(&mut *db::connect()?)?;

    json_ok(ProfileResBody {
        avatar_url,
        blurhash,
        display_name,
    })
}

/// #GET /_matrix/client/r0/profile/{user_id}/avatar_url
/// Returns the avatar_url and blurhash of the user.
///
/// - If user is on another server: Fetches avatar_url and blurhash over federation
#[endpoint]
async fn get_avatar_url(_aa: AuthArgs, user_id: PathParam<OwnedUserId>) -> JsonResult<AvatarUrlResBody> {
    let user_id = user_id.into_inner();
    if user_id.is_remote() {
        let server_name = user_id.server_name().to_owned();
        let request = profile_request(
            &server_name.origin().await,
            ProfileReqArgs {
                user_id,
                field: Some(ProfileField::AvatarUrl),
            },
        )?
        .into_inner();

        let body: AvatarUrlResBody = crate::sending::send_federation_request(&server_name, request)
            .await?
            .json::<AvatarUrlResBody>()
            .await?;
        return json_ok(body);
    }

    let DbProfile {
        avatar_url, blurhash, ..
    } = user_profiles::table
        .filter(user_profiles::user_id.eq(&user_id))
        .first::<DbProfile>(&mut *db::connect()?)?;

    json_ok(AvatarUrlResBody {
        avatar_url,
        blurhash: blurhash,
    })
}

/// #PUT /_matrix/client/r0/profile/{user_id}/avatar_url
/// Updates the avatar_url and blurhash.
///
/// - Also makes sure other users receive the update using presence EDUs
#[endpoint]
async fn set_avatar_url(
    _aa: AuthArgs,
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<SetAvatarUrlReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let user_id = user_id.into_inner();
    let authed = depot.authed_info()?;
    if authed.user_id() != &user_id {
        return Err(MatrixError::forbidden("forbidden").into());
    }

    let SetAvatarUrlReqBody { avatar_url, blurhash } = body.into_inner();

    let query = user_profiles::table
        .filter(user_profiles::user_id.eq(&user_id))
        .filter(user_profiles::room_id.is_null());
    if diesel_exists!(query, &mut *db::connect()?)? {
        #[derive(AsChangeset, Debug)]
        #[diesel(table_name = user_profiles, treat_none_as_null = true)]
        struct UpdateParams {
            avatar_url: Option<OwnedMxcUri>,
            blurhash: Option<String>,
        }
        let updata_params = UpdateParams {
            avatar_url: avatar_url.clone(),
            blurhash,
        };
        diesel::update(query).set(updata_params).execute(&mut *db::connect()?)?;
    } else {
        return Err(StatusError::not_found().brief("Profile not found.").into());
    }

    // Send a new membership event and presence update into all joined rooms
    let all_joined_rooms: Vec<_> = crate::user::joined_rooms(&user_id, 0)?
        .into_iter()
        .map(|room_id| {
            Ok::<_, AppError>((
                PduBuilder {
                    event_type: TimelineEventType::RoomMember,
                    content: to_raw_value(&RoomMemberEventContent {
                        avatar_url: avatar_url.clone(),
                        ..serde_json::from_str(
                            crate::room::state::get_room_state(
                                &room_id,
                                &StateEventType::RoomMember,
                                user_id.as_str(), None,
                            )?
                            .ok_or_else(|| {
                                AppError::internal("Tried to send avatar_url update for user not in the room.")
                            })?
                            .content
                            .get(),
                        )
                        .map_err(|_| AppError::internal("Database contains invalid PDU."))?
                    })
                    .expect("event is valid, we just created it"),
                    state_key: Some(user_id.to_string()),
                    ..Default::default()
                },
                room_id,
            ))
        })
        .filter_map(|r| r.ok())
        .collect();

    // Presence update
    crate::user::set_presence(
        NewDbPresence {
            user_id: user_id.clone(),
            // room_id: Some(room_id),
            stream_id: None,
            state: None,
            status_msg: None,
            last_active_at: Some(UnixMillis::now()),
            last_federation_update_at: None,
            last_user_sync_at: None,
            currently_active: None,
            occur_sn: None,
        },
        true,
    )?;
    for (pdu_builder, room_id) in all_joined_rooms {
        // let mutex_state = Arc::clone(
        //     services()
        //         .globals
        //         .roomid_mutex_state
        //         .write()
        //         .await
        //         .entry(room_id.clone())
        //         .or_default(),
        // );
        // let state_lock = mutex_state.lock().await;

        let _ = crate::room::timeline::build_and_append_pdu(pdu_builder, &user_id, &room_id)?;
    }

    empty_ok()
}

/// #GET /_matrix/client/r0/profile/{user_id}/displayname
/// Returns the display_name of the user.
///
/// - If user is on another server: Fetches display_name over federation
#[endpoint]
async fn get_display_name(_aa: AuthArgs, user_id: PathParam<OwnedUserId>) -> JsonResult<DisplayNameResBody> {
    let user_id = user_id.into_inner();
    if user_id.is_remote() {
        let server_name = user_id.server_name().to_owned();
        let request = profile_request(
            &server_name.origin().await,
            ProfileReqArgs {
                user_id,
                field: Some(ProfileField::DisplayName),
            },
        )?
        .into_inner();

        let body = crate::sending::send_federation_request(&server_name, request)
            .await?
            .json::<DisplayNameResBody>()
            .await?;
        return json_ok(body);
    }
    json_ok(DisplayNameResBody {
        display_name: crate::user::display_name(&user_id)?,
    })
}

/// #PUT /_matrix/client/r0/profile/{user_id}/displayname
/// Updates the display_name.
///
/// - Also makes sure other users receive the update using presence EDUs
#[endpoint]
async fn set_display_name(
    _aa: AuthArgs,
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<SetDisplayNameReqBody>,
    depot: &mut Depot,
) -> EmptyResult {
    let user_id = user_id.into_inner();
    let authed = depot.authed_info()?;
    if authed.user_id() != &user_id {
        return Err(MatrixError::forbidden("forbidden").into());
    }
    let SetDisplayNameReqBody { display_name } = body.into_inner();

    crate::user::set_display_name(&user_id, display_name.as_deref())?;

    // Send a new membership event and presence update into all joined rooms
    let all_joined_rooms: Vec<_> = crate::user::joined_rooms(&user_id, 0)?
        .into_iter()
        .map(|room_id| {
            Ok::<_, AppError>((
                PduBuilder {
                    event_type: TimelineEventType::RoomMember,
                    content: to_raw_value(&RoomMemberEventContent {
                        display_name: display_name.clone(),
                        ..serde_json::from_str(
                            crate::room::state::get_room_state(
                                &room_id,
                                &StateEventType::RoomMember,
                                user_id.as_str(), None,
                            )?
                            .ok_or_else(|| {
                                AppError::internal("Tried to send display_name update for user not in the room.")
                            })?
                            .content
                            .get(),
                        )
                        .map_err(|_| AppError::internal("Database contains invalid PDU."))?
                    })
                    .expect("event is valid, we just created it"),
                    state_key: Some(user_id.to_string()),
                    ..Default::default()
                },
                room_id,
            ))
        })
        .filter_map(|r| r.ok())
        .collect();

    for (pdu_builder, room_id) in all_joined_rooms {
        // let mutex_state = Arc::clone(
        //     services()
        //         .globals
        //         .roomid_mutex_state
        //         .write()
        //         .await
        //         .entry(room_id.clone())
        //         .or_default(),
        // );
        // let state_lock = mutex_state.lock().await;

        let _ = crate::room::timeline::build_and_append_pdu(pdu_builder, &user_id, &room_id)?;

        // Presence update
        crate::user::set_presence(
            NewDbPresence {
                user_id: user_id.clone(),
                stream_id: None,
                state: None,
                status_msg: None,
                last_active_at: Some(UnixMillis::now()),
                last_federation_update_at: None,
                last_user_sync_at: None,
                currently_active: None,
                occur_sn: None,
            },
            true,
        )?;
    }

    empty_ok()
}
