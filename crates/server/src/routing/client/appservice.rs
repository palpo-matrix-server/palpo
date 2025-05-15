use salvo::oapi::extract::{JsonBody, PathParam};
use salvo::prelude::*;

use crate::core::OwnedTransactionId;
use crate::core::appservice::ping::{SendPingReqBody, SendPingResBody, send_ping_request};
use crate::{AuthArgs, DepotExt, JsonResult, MatrixError, json_ok};

pub fn authed_router() -> Router {
    Router::with_path("appservice/{appservice_id}/ping").post(ping)
}

#[endpoint]
async fn ping(
    _aa: AuthArgs,
    appservice_id: PathParam<OwnedTransactionId>,
    body: JsonBody<SendPingReqBody>,
    depot: &mut Depot,
) -> JsonResult<SendPingResBody> {
    let appservice_id = appservice_id.into_inner();
    let body = body.into_inner();
    let authed = depot.authed_info()?;
    let Some(appservice) = authed.appservice.as_ref() else {
        return Err(MatrixError::forbidden("This endpoint can only be called by appservices.", None).into());
    };

    if appservice_id != appservice.registration.id {
        return Err(MatrixError::forbidden("Appservices can only ping themselves (wrong appservice ID).", None).into());
    }

    if appservice.registration.url.is_none()
        || appservice
            .registration
            .url
            .as_ref()
            .is_some_and(|url| url.is_empty() || url == "null")
    {
        return Err(MatrixError::url_not_set("Appservice does not have a URL set, there is nothing to ping.").into());
    }

    let timer = tokio::time::Instant::now();

    if let Some(url) = appservice.registration.url.as_ref() {
        let request = send_ping_request(
            url,
            SendPingReqBody {
                transaction_id: body.transaction_id.clone(),
            },
        )?
        .into_inner();
        let _response = crate::sending::send_appservice_request::<()>(appservice.registration.clone(), request).await?;
    }

    json_ok(SendPingResBody::new(timer.elapsed()))
}
