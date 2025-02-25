use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::key::UploadSigningKeysReqBody;
use crate::core::client::uiaa::{AuthFlow, AuthType, UiaaInfo};
use crate::{AuthArgs, DepotExt, EmptyResult, SESSION_ID_LENGTH, empty_ok, utils};

/// #POST /_matrix/client/r0/keys/device_signing/upload
/// Uploads end-to-end key information for the sender user.
///
/// - Requires UIAA to verify password
#[endpoint]
pub(super) async fn upload(_aa: AuthArgs, body: JsonBody<UploadSigningKeysReqBody>, depot: &mut Depot) -> EmptyResult {
    let authed = depot.authed_info()?;

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
    let Some(auth) = &body.auth else {
        uiaa_info.session = Some(utils::random_string(SESSION_ID_LENGTH));
        return Err(uiaa_info.into());
    };

    crate::uiaa::try_auth(authed.user_id(), authed.device_id(), &auth, &uiaa_info)?;

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
