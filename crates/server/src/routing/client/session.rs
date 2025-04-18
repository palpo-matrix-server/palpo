use std::time::Duration;

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::Deserialize;

use crate::core::client::session::*;
use crate::core::client::uiaa::{AuthFlow, AuthType, UiaaInfo, UserIdentifier};
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonValue;
use crate::{
    AppError, AuthArgs, DEVICE_ID_LENGTH, DepotExt, EmptyResult, JsonResult, MatrixError, SESSION_ID_LENGTH,
    TOKEN_LENGTH, data, empty_ok, hoops, json_ok, utils,
};

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    //exp: usize,
}

pub fn public_router() -> Router {
    Router::new().push(
        Router::with_path("login")
            .hoop(hoops::limit_rate)
            .get(login_types)
            .post(login)
            .push(
                Router::with_path("sso/redirect")
                    .get(redirect)
                    .push(Router::with_path("idpId").get(provider_url)),
            ),
    )
}
pub fn authed_router() -> Router {
    Router::new()
        .push(
            Router::with_path("login")
                .hoop(hoops::limit_rate)
                .push(Router::with_path("get_token").post(get_token))
                .push(Router::with_path("refresh").get(refresh_token)),
        )
        .push(
            Router::with_path("logout")
                .post(logout)
                .push(Router::with_path("all").post(logout_all)),
        )
}

/// #GET /_matrix/client/r0/login
/// Get the supported login types of this server. One of these should be used as the `type` field
/// when logging in.
#[endpoint]
async fn login_types(_aa: AuthArgs) -> JsonResult<LoginTypesResBody> {
    let flows = vec![LoginType::password(), LoginType::appservice()];
    Ok(Json(LoginTypesResBody::new(flows)))
}

/// #POST /_matrix/client/r0/login
/// Authenticates the user and returns an access token it can use in subsequent requests.
///
/// - The user needs to authenticate using their password (or if enabled using a json web token)
/// - If `device_id` is known: invalidates old access token of that device
/// - If `device_id` is unknown: creates a new device
/// - Returns access token that is associated with the user and device
///
/// Note: You can use [`GET /_matrix/client/r0/login`](fn.get_supported_versions_route.html) to see
/// supported login types.
#[endpoint]
async fn login(body: JsonBody<LoginReqBody>, res: &mut Response) -> JsonResult<LoginResBody> {
    // Validate login method
    // TODO: Other login methods
    let user_id = match &body.login_info {
        LoginInfo::Password(Password { identifier, password }) => {
            let username = if let UserIdentifier::UserIdOrLocalpart(user_id) = identifier {
                user_id.to_lowercase()
            } else {
                warn!("Bad login type: {:?}", &body.login_info);
                return Err(MatrixError::forbidden("Bad login type.").into());
            };
            let user_id = UserId::parse_with_server_name(username, crate::server_name())
                .map_err(|_| MatrixError::invalid_username("Username is invalid."))?;
            let Some(user) = data::user::get_user(&user_id)? else {
                return Err(MatrixError::forbidden("User not found.").into());
            };
            if let Err(_e) = crate::user::vertify_password(&user, &password) {
                res.status_code(StatusCode::FORBIDDEN); //for complement testing: TestLogin/parallel/POST_/login_wrong_password_is_rejected
                return Err(MatrixError::forbidden("Wrong username or password.").into());
            }
            user_id
        }
        LoginInfo::Token(Token { token }) => {
            if !crate::config().login_via_existing_session {
                return Err(MatrixError::unknown("Token login is not enabled.").into());
            }
            crate::user::take_login_token(token)?
        }
        LoginInfo::Appservice(Appservice { identifier }) => {
            let username = if let UserIdentifier::UserIdOrLocalpart(user_id) = identifier {
                user_id.to_lowercase()
            } else {
                return Err(MatrixError::forbidden("Bad login type.").into());
            };
            let user_id = UserId::parse_with_server_name(username, crate::server_name())
                .map_err(|_| MatrixError::invalid_username("Username is invalid."))?;
            user_id
        }
        _ => {
            warn!("Unsupported or unknown login type: {:?}", &body.login_info);
            return Err(MatrixError::unknown("Unsupported login type.").into());
        }
    };

    // Generate new device id if the user didn't specify one
    let device_id = body
        .device_id
        .clone()
        .unwrap_or_else(|| utils::random_string(DEVICE_ID_LENGTH).into());

    // Generate a new token for the device
    let token = utils::random_string(TOKEN_LENGTH);

    // Determine if device_id was provided and exists in the db for this user
    if crate::user::is_device_exists(&user_id, &device_id)? {
        crate::user::set_token(&user_id, &device_id, &token)?;
    } else {
        crate::user::create_device(&user_id, &device_id, &token, body.initial_device_display_name.clone())?;
    }

    tracing::info!("{} logged in", user_id);

    json_ok(LoginResBody {
        user_id,
        access_token: token,
        device_id,
        well_known: None,
        refresh_token: None,
        expires_in: None,
    })
}

/// # `POST /_matrix/client/v1/login/get_token`
///
/// Allows a logged-in user to get a short-lived token which can be used
/// to log in with the m.login.token flow.
///
/// <https://spec.matrix.org/v1.13/client-server-api/#post_matrixclientv1loginget_token>
#[endpoint]
async fn get_token(_aa: AuthArgs, req: &mut Request, depot: &mut Depot) -> JsonResult<TokenResBody> {
    let conf = crate::config();
    let authed = depot.authed_info()?;
    let sender_id = authed.user_id();
    let device_id = authed.device_id();

    if !conf.login_via_existing_session {
        return Err(MatrixError::forbidden("Login via an existing session is not enabled").into());
    }

    // This route SHOULD have UIA
    // TODO: How do we make only UIA sessions that have not been used before valid?
    let mut uiaa_info = UiaaInfo {
        flows: vec![AuthFlow {
            stages: vec![AuthType::Password],
        }],
        completed: Vec::new(),
        params: Box::default(),
        session: None,
        auth_error: None,
    };

    let payload = req.payload().await?;
    let body = serde_json::from_slice::<TokenReqBody>(&payload);
    if let Ok(Some(auth)) = body.as_ref().map(|b| &b.auth) {
        let (worked, uiaa_info) = crate::uiaa::try_auth(sender_id, device_id, auth, &uiaa_info)?;

        if !worked {
            return Err(AppError::Uiaa(uiaa_info));
        }
    } else if let Ok(json) = serde_json::from_slice::<CanonicalJsonValue>(&payload) {
        uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
        crate::uiaa::create_session(sender_id, device_id, &uiaa_info, json);
        return Err(AppError::Uiaa(uiaa_info));
    } else {
        return Err(MatrixError::not_json("No JSON body was sent when required.").into());
    }

    let login_token = utils::random_string(TOKEN_LENGTH);
    let expires_in = crate::user::create_login_token(sender_id, &login_token)?;

    json_ok(TokenResBody {
        expires_in: Duration::from_millis(expires_in),
        login_token,
    })
}

/// #POST /_matrix/client/r0/logout
/// Log out the current device.
///
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip, last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
#[endpoint]
async fn logout(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    let Ok(authed) = depot.authed_info() else {
        return empty_ok();
    };

    crate::user::remove_device(authed.user_id(), authed.device_id())?;

    empty_ok()
}

/// #POST /_matrix/client/r0/logout/all
/// Log out all devices of this user.
///
/// - Invalidates all access tokens
/// - Deletes all device metadata (device id, device display name, last seen ip, last seen ts)
/// - Forgets all to-device events
/// - Triggers device list updates
///
/// Note: This is equivalent to calling [`GET /_matrix/client/r0/logout`](fn.logout_route.html)
/// from each device of this user.
#[endpoint]
async fn logout_all(_aa: AuthArgs, depot: &mut Depot) -> EmptyResult {
    let Ok(authed) = depot.authed_info() else {
        return empty_ok();
    };

    crate::user::remove_all_devices(authed.user_id())?;

    empty_ok()
}

#[endpoint]
async fn refresh_token(_aa: AuthArgs) -> EmptyResult {
    // TODO: fixme
    panic!("refresh_tokenNot implemented")
    // let authed = depot.authed_info()?;
    // Ok(())
}

#[endpoint]
async fn redirect(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}

#[endpoint]
async fn provider_url(_aa: AuthArgs) -> EmptyResult {
    // TODO: todo
    empty_ok()
}
