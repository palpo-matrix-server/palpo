use salvo::prelude::*;

use crate::core::client::key::UploadSigningKeysReqBody;
use crate::core::client::uiaa::{AuthFlow, AuthType, UiaaInfo};
use crate::core::serde::CanonicalJsonValue;
use crate::{AuthArgs, DepotExt, EmptyResult, MatrixError, SESSION_ID_LENGTH, empty_ok, utils};

/// #POST /_matrix/client/r0/keys/device_signing/upload
/// Uploads end-to-end key information for the sender user.
///
/// - Requires UIAA to verify password
#[endpoint]
pub(super) async fn upload(_aa: AuthArgs, req: &mut Request, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;

    let payload = req.payload().await?;
    // UIAA
    let mut uiaa_info = UiaaInfo {
        flows: vec![AuthFlow {
            stages: vec![AuthType::Password],
        }],
        completed: Vec::new(),
        params: Default::default(),
        session: None,
        auth_error: None,
    };
    let body = serde_json::from_slice::<UploadSigningKeysReqBody>(&payload);
    if body.is_err() || body.as_ref().map(|b| b.auth.is_none()).unwrap_or(true) {
        if let Ok(json) = serde_json::from_slice::<CanonicalJsonValue>(&payload) {
            uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
            crate::uiaa::create_session(authed.user_id(), authed.device_id(), &uiaa_info, json)?;
            return Err(uiaa_info.into());
        } else {
            return Err(MatrixError::not_json("No JSON body was sent when required.").into());
        }
    };
    let body = body.expect("body should be ok");
    let Some(auth) = &body.auth else {
        return Err(MatrixError::not_json("auth is none should not happend.").into());
    };

    crate::uiaa::try_auth(authed.user_id(), authed.device_id(), auth, &uiaa_info)?;

    if let Some(master_key) = &body.master_key {
        crate::user::add_cross_signing_keys(
            authed.user_id(),
            master_key,
            &body.self_signing_key,
            &body.user_signing_key,
            true, // notify so that other users see the new keys
        )?;
    }
    empty_ok()
}
