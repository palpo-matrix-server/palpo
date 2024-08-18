use diesel::prelude::*;
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::user_directory::SearchedUser;
use crate::core::client::user_directory::{SearchUsersReqArgs, SearchUsersReqBody, SearchUsersResBody};
use crate::core::events::room::join_rules::{JoinRule, RoomJoinRulesEventContent};
use crate::core::events::StateEventType;
use crate::core::identifiers::*;
use crate::schema::*;
use crate::{db, hoops, json_ok, AuthArgs, DepotExt, JsonResult};

pub fn authed_router() -> Router {
    Router::with_path("user_directory/search")
        .hoop(hoops::limit_rate)
        .post(search)
}

// #POST /_matrix/client/r0/user_directory/search
/// Searches all known users for a match.
///
/// - Hides any local users that aren't in any public rooms (i.e. those that have the join rule set to public)
/// and don't share a room with the sender
#[endpoint]
fn search(
    _aa: AuthArgs,
    args: SearchUsersReqArgs,
    body: JsonBody<SearchUsersReqBody>,
    depot: &mut Depot,
) -> JsonResult<SearchUsersResBody> {
    let authed = depot.authed_info()?;
    let user_ids = users::table
        .select(users::id)
        .load::<OwnedUserId>(&mut *db::connect()?)?;

    let mut users = user_ids.into_iter().filter_map(|user_id| {
        let user = SearchedUser {
            user_id: user_id.clone(),
            display_name: crate::user::display_name(&user_id).ok()?,
            avatar_url: crate::user::avatar_url(&user_id).ok()?,
        };

        let user_id_matches = user
            .user_id
            .to_string()
            .to_lowercase()
            .contains(&body.search_term.to_lowercase());

        let user_display_name_matches = user
            .display_name
            .as_ref()
            .filter(|name| name.to_lowercase().contains(&body.search_term.to_lowercase()))
            .is_some();

        if !user_id_matches && !user_display_name_matches {
            return None;
        }

        let user_is_in_public_rooms = crate::user::joined_rooms(&user_id, 0).ok()?.into_iter().any(|room| {
            crate::room::state::get_state(&room, &StateEventType::RoomJoinRules, "").map_or(false, |event| {
                event.map_or(false, |event| {
                    serde_json::from_str(event.content.get())
                        .map_or(false, |r: RoomJoinRulesEventContent| r.join_rule == JoinRule::Public)
                })
            })
        });

        if user_is_in_public_rooms {
            return Some(user);
        }

        let user_is_in_shared_rooms = crate::room::user::get_shared_rooms(vec![authed.user_id().clone(), user_id])
            .ok()?
            .is_empty();

        if user_is_in_shared_rooms {
            return Some(user);
        }

        None
    });

    let results = users.by_ref().take(body.limit).collect();
    let limited = users.next().is_some();
    json_ok(SearchUsersResBody { results, limited })
}
