use diesel::PgConnection;
use tracing::error;

use crate::core::serde::CanonicalJsonValue;
use crate::core::{
    client::uiaa::{AuthData, AuthError, AuthType, Password, UiaaInfo, UserIdentifier},
    DeviceId, UserId,
};
use crate::SESSION_ID_LENGTH;
use crate::{db, utils, AppError, AppResult, MatrixError};

/// Creates a new Uiaa session. Make sure the session token is unique.
pub fn create_session(
    user_id: &UserId,
    device_id: &DeviceId,
    uiaa_info: &UiaaInfo,
    json_body: &CanonicalJsonValue,
) -> AppResult<()> {
    // TODO: fixme
    panic!("fixme")
    // db::set_uiaa_request(
    //     user_id,
    //     device_id,
    //     uiaa_info.session.as_ref().expect("session should be set"), // TODO: better session error handling (why is it optional in palpo?)
    //     json_body,
    // )?;
    // db::update_uiaa_session(
    //     user_id,
    //     device_id,
    //     uiaa_info.session.as_ref().expect("session should be set"),
    //     Some(uiaa_info),
    // )
}

pub fn update_session(
    user_id: &UserId,
    device_id: &DeviceId,
    session: &str,
    uiaainfo: Option<&UiaaInfo>,
) -> AppResult<()> {
    Ok(())
}
pub fn try_auth(
    user_id: &UserId,
    device_id: &DeviceId,
    auth: &AuthData,
    uiaa_info: &UiaaInfo,
) -> AppResult<(bool, UiaaInfo)> {
    // let mut uiaa_info = auth
    //     .session()
    //     .map(|session| db::get_uiaa_session(user_id, device_id, session))
    //     .unwrap_or_else(|| Ok(uiaa_info.clone()))?;
    let mut uiaa_info = uiaa_info.clone();

    if uiaa_info.session.is_none() {
        uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
    }
    let conf = crate::config();

    match auth {
        // Find out what the user completed
        AuthData::Password(Password {
            identifier, password, ..
        }) => {
            let username = match identifier {
                UserIdentifier::UserIdOrLocalpart(username) => username,
                _ => return Err(MatrixError::unrecognized("Identifier type not recognized.").into()),
            };

            let user_id = UserId::parse_with_server_name(username.clone(), &conf.server_name)
                .map_err(|_| MatrixError::invalid_param("User ID is invalid."))?;

            crate::user::vertify_password(&user_id, &password)?;
        }
        AuthData::RegistrationToken(t) => {
            if Some(t.token.trim()) == conf.registration_token.as_deref() {
                uiaa_info.completed.push(AuthType::RegistrationToken);
            } else {
                uiaa_info.auth_error = Some(AuthError::forbidden("Invalid registration token."));
                return Ok((false, uiaa_info));
            }
        }
        AuthData::Dummy(_) => {
            uiaa_info.completed.push(AuthType::Dummy);
        }
        k => error!("type not supported: {:?}", k),
    }

    // Check if a flow now succeeds
    let mut completed = false;
    'flows: for flow in &mut uiaa_info.flows {
        for stage in &flow.stages {
            if !uiaa_info.completed.contains(stage) {
                continue 'flows;
            }
        }
        // We didn't break, so this flow succeeded!
        completed = true;
    }

    if !completed {
        crate::uiaa::update_session(
            user_id,
            device_id,
            uiaa_info.session.as_ref().expect("session is always set"),
            Some(&uiaa_info),
        )?;
        return Ok((false, uiaa_info));
    }

    // UIAA was successful! Remove this session and return true
    crate::uiaa::update_session(
        user_id,
        device_id,
        uiaa_info.session.as_ref().expect("session is always set"),
        None,
    )?;
    Ok((true, uiaa_info))
}

pub fn get_uiaa_request(user_id: &UserId, device_id: &DeviceId, session: &str) -> Option<CanonicalJsonValue> {
    // TODO: fixme
    panic!("fixme")
    // self.userdevicesessionid_uiaarequest
    //     .read()
    //     .unwrap()
    //     .get(&(user_id.to_owned(), device_id.to_owned(), session.to_owned()))
    //     .map(|j| j.to_owned())
}
