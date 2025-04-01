mod device_signing;
mod signature;

use std::collections::HashSet;

use palpo_core::client::key::KeyChangesReqArgs;
use salvo::oapi::extract::*;
use salvo::prelude::*;

use crate::core::client::key::{
    ClaimKeysReqBody, ClaimKeysResBody, KeyChangesResBody, KeysReqBody, KeysResBody, UploadKeysReqBody,
    UploadKeysResBody,
};
use crate::user::key;
use crate::{AuthArgs, CjsonResult, DepotExt, JsonResult, cjson_ok, json_ok};

pub fn authed_router() -> Router {
    Router::with_path("keys")
        .push(Router::with_path("claim").post(claim_keys))
        .push(Router::with_path("query").post(query_keys))
        .push(Router::with_path("upload").post(upload_keys))
        .push(Router::with_path("changes").get(get_key_changes))
        .push(Router::with_path("signatures/upload").post(signature::upload))
        .push(Router::with_path("device_signing/upload").post(device_signing::upload))
}

/// #POST /_matrix/client/r0/keys/claim
/// Claims one-time keys
#[endpoint]
async fn claim_keys(_aa: AuthArgs, body: JsonBody<ClaimKeysReqBody>) -> CjsonResult<ClaimKeysResBody> {
    cjson_ok(key::claim_one_time_keys(&body.one_time_keys).await?)
}

/// #POST /_matrix/client/r0/keys/query
/// Get end-to-end encryption keys for the given users.
///
/// - Always fetches users from other servers over federation
/// - Gets master keys, self-signing keys, user signing keys and device keys.
/// - The master and self-signing keys contain signatures that the user is allowed to see
#[endpoint]
async fn query_keys(_aa: AuthArgs, body: JsonBody<KeysReqBody>, depot: &mut Depot) -> CjsonResult<KeysResBody> {
    let authed = depot.authed_info()?;
    cjson_ok(
        key::query_keys(
            Some(authed.user_id()),
            &body.device_keys,
            |u| u == authed.user_id(),
            false,
        )
        .await?,
    )
}

/// #POST /_matrix/client/r0/keys/upload
/// Publish end-to-end encryption keys for the sender device.
///
/// - Adds one time keys
/// - If there are no device keys yet: Adds device keys (TODO: merge with existing keys?)
#[endpoint]
async fn upload_keys(
    _aa: AuthArgs,
    body: JsonBody<UploadKeysReqBody>,
    depot: &mut Depot,
) -> JsonResult<UploadKeysResBody> {
    let authed = depot.authed_info()?;

    for (key_id, one_time_key) in &body.one_time_keys {
        crate::user::add_one_time_key(authed.user_id(), authed.device_id(), key_id, one_time_key)?;
    }

    if let Some(device_keys) = &body.device_keys {
        crate::user::add_device_keys(authed.user_id(), authed.device_id(), device_keys)?;
    }

    json_ok(UploadKeysResBody {
        one_time_key_counts: crate::user::count_one_time_keys(authed.user_id(), authed.device_id())?,
    })
}

/// #POST /_matrix/client/r0/keys/changes
/// Gets a list of users who have updated their device identity keys since the previous sync token.
///
/// - TODO: left users
#[endpoint]
async fn get_key_changes(_aa: AuthArgs, args: KeyChangesReqArgs, depot: &mut Depot) -> JsonResult<KeyChangesResBody> {
    let authed = depot.authed_info()?;

    let from_sn = args.from.parse()?;
    let to_sn = args.to.parse()?;
    let mut device_list_updates = HashSet::new();
    device_list_updates.extend(crate::user::keys_changed_users(authed.user_id(), from_sn, Some(to_sn))?);

    for room_id in crate::user::joined_rooms(authed.user_id(), 0)? {
        device_list_updates.extend(crate::room::keys_changed_users(&room_id, from_sn, Some(to_sn))?);
    }
    json_ok(KeyChangesResBody {
        changed: device_list_updates.into_iter().collect(),
        left: Vec::new(), // TODO
    })
}
