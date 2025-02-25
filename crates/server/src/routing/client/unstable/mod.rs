use salvo::prelude::*;

mod msc3391;

pub(super) fn router() -> Router {
    Router::with_path("unstable").push(
        Router::with_path("org.matrix.msc3391/user/{user_id}/account_data/{account_type}")
            .delete(msc3391::delete_account_data),
    )
}
