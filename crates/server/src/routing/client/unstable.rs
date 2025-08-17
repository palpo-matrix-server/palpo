use salvo::prelude::*;

use crate::hoops;

pub(super) fn router() -> Router {
    Router::with_path("unstable")
        .hoop(hoops::limit_rate)
        .hoop(hoops::auth_by_access_token)
        .push(
            Router::with_path("org.matrix.msc3391/user/{user_id}/account_data/{account_type}")
                .delete(super::account::delete_account_data_msc3391),
        )
        .push(
            Router::with_path("org.matrix.simplified_msc3575/sync")
                .post(super::sync_msc4186::sync_events_v5),
        )
        .push(
            Router::with_path("im.nheko.summary/rooms/{room_id_or_alias}/summary")
                .get(super::room::summary::get_summary_msc_3266),
        )
}
