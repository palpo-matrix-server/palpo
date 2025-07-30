mod alias;
mod room;

use salvo::prelude::*;

pub fn public_router() -> Router {
    Router::with_path("directory")
        .push(Router::with_path("room/{room_alias}").get(alias::get_alias))
        .push(Router::with_path("list/room/{room_id}").get(room::get_visibility))
}

pub fn authed_router() -> Router {
    Router::with_path("directory")
        .push(
            Router::with_path("room/{room_alias}")
                .put(alias::upsert_alias)
                .delete(alias::delete_alias),
        )
        .push(
            Router::with_path("list")
                .push(
                    Router::with_path("appservice/{network_id}/{room_id}")
                        .put(room::set_visibility_with_network_id),
                )
                .push(
                    Router::with_path("room/{room_id}")
                        .get(room::get_visibility)
                        .put(room::set_visibility),
                ),
        )
}
