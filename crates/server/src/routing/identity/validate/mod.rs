mod email;
mod msisdn;

use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;

use crate::{empty_ok, hoops, json_ok, DepotExt, JsonResult};
use crate::{AuthArgs, AuthedInfo};

pub fn router() -> Router {
    Router::with_path("validate")
        .push(
            Router::with_path("email")
                .push(Router::with_path("requestToken").post(email::create_session))
                .push(
                    Router::with_path("submitToken")
                        .get(email::validate_by_end_user)
                        .post(email::validate),
                ),
        )
        .push(
            Router::with_path("msisdn")
                .push(Router::with_path("requestToken").post(msisdn::create_session))
                .push(
                    Router::with_path("submitToken")
                        .get(msisdn::validate_by_phone_number)
                        .post(msisdn::validate),
                ),
        )
}
