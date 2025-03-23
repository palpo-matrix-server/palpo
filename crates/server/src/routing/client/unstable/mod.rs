use salvo::prelude::*;

mod msc3391;
mod msc3575;
// mod msc4186;

pub(super) fn router() -> Router {
    Router::with_path("unstable")
        .push(
            Router::with_path("org.matrix.msc3391/user/{user_id}/account_data/{account_type}")
                .delete(msc3391::delete_account_data),
        )
        .push(Router::with_path("org.matrix.msc3575/sync").delete(msc3575::sync_events_v4))
        // .push(Router::with_path("org.matrix.simplified_msc3575/sync").post(msc4186::sync_events_v5))
}
