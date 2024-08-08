mod data;
mod filter;
mod openid;
mod room;

use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::hoops;

pub fn authed_router() -> Router {
    Router::with_path("user")
        .push(
            Router::with_hoop(hoops::limit_rate).push(
                Router::with_path("<user_id>")
                    .push(Router::with_path("mutual_rooms").get(room::mutual))
                    .push(Router::with_path("openid/request_token").post(openid::request_token)),
            ),
        )
        .push(
            Router::with_hoop(hoops::limit_rate).push(
                Router::with_path("<user_id>")
                    .push(
                        Router::with_path("filter")
                            .post(filter::create_filter)
                            .push(Router::with_path("<filter_id>").get(filter::get_filter)),
                    )
                    .push(
                        Router::with_path("account_data/<event_type>")
                            .get(data::get_data)
                            .put(data::set_data),
                    )
                    .push(
                        Router::with_path("room/<room_id>/account_data/<event_type>")
                            .get(room::get_data)
                            .put(room::set_data),
                    ),
            ),
        )
}
