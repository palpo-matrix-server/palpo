use salvo::oapi::extract::QueryParam;
use salvo::prelude::*;

use crate::core::federation::openid::{UserInfoResBody,UserInfoReqArgs};
use crate::{empty_ok, AuthArgs, json_ok, EmptyResult, JsonResult};

pub fn router() -> Router {
    Router::with_path("openid/userinfo").get(user_info)
}

#[endpoint]
async fn user_info(_aa: AuthArgs, args: UserInfoReqArgs) -> JsonResult<UserInfoResBody> {
    let user_id = crate::user::find_from_openid_token(&args.access_token).await?;
    json_ok(UserInfoResBody::new(user_id))
}
