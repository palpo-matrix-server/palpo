//! Synapse Admin API - User Management
//!
//! Phase 1 (MAS Critical):
//! - GET/PUT /_synapse/admin/v2/users/{user_id}
//! - GET /_synapse/admin/v2/users
//! - GET /_synapse/admin/v3/users
//! - POST /_synapse/admin/v1/users/{user_id}/_allow_cross_signing_replacement_without_uia
//!
//! Phase 2 (User Management):
//! - POST /_synapse/admin/v1/deactivate/{user_id}
//! - POST /_synapse/admin/v1/reset_password/{user_id}
//! - GET/PUT /_synapse/admin/v1/users/{user_id}/admin
//! - POST/DELETE /_synapse/admin/v1/users/{user_id}/shadow_ban

use salvo::oapi::extract::*;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::identifiers::*;
use crate::{data, empty_ok, json_ok, user, EmptyResult, JsonResult, MatrixError};

// ============================================================================
// Response/Request Types
// ============================================================================

/// User info for admin API v2
#[derive(Debug, Serialize, ToSchema)]
pub struct UserInfoV2 {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displayname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threepids: Option<Vec<ThreepidInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub is_guest: bool,
    pub admin: bool,
    pub deactivated: bool,
    pub shadow_banned: bool,
    pub locked: bool,
    pub creation_ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appservice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_server_notice_sent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ids: Option<Vec<ExternalIdInfo>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ThreepidInfo {
    pub medium: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ExternalIdInfo {
    pub auth_provider: String,
    pub external_id: String,
}

/// Request body for PUT /v2/users/{user_id}
#[derive(Debug, Deserialize, ToSchema)]
pub struct PutUserReqBody {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub logout_devices: Option<bool>,
    #[serde(default)]
    pub displayname: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub threepids: Option<Vec<ThreepidInfo>>,
    #[serde(default)]
    pub external_ids: Option<Vec<ExternalIdInfo>>,
    #[serde(default)]
    pub admin: Option<bool>,
    #[serde(default)]
    pub deactivated: Option<bool>,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub user_type: Option<String>,
}

/// Response for user list
#[derive(Debug, Serialize, ToSchema)]
pub struct UsersListResponse {
    pub users: Vec<UserInfoV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    pub total: i64,
}

/// Request for deactivate
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeactivateReqBody {
    #[serde(default)]
    pub erase: Option<bool>,
}

/// Request for reset password
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetPasswordReqBody {
    pub new_password: String,
    #[serde(default)]
    pub logout_devices: Option<bool>,
}

/// Response for admin status
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminStatusResponse {
    pub admin: bool,
}

/// Request for admin status
#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminStatusReqBody {
    pub admin: bool,
}

/// Response for cross signing
#[derive(Debug, Serialize, ToSchema)]
pub struct CrossSigningResponse {
    // Empty response on success
}

// ============================================================================
// Helper functions
// ============================================================================

fn build_user_info(user_id: &UserId) -> crate::AppResult<UserInfoV2> {
    let db_user = data::user::get_user(user_id).map_err(|_| MatrixError::not_found("User not found"))?;

    let display_name = data::user::display_name(user_id).ok().flatten();
    let avatar_url = data::user::avatar_url(user_id).ok().flatten();
    let threepids = data::user::get_threepids(user_id).ok().map(|tps| {
        tps.into_iter()
            .map(|tp| ThreepidInfo {
                medium: tp.medium,
                address: tp.address,
                added_at: Some(tp.added_at.get() as i64),
                validated_at: Some(tp.validated_at.get() as i64),
            })
            .collect()
    });
    let external_ids = data::user::get_external_ids_by_user(user_id).ok().map(|eids| {
        eids.into_iter()
            .map(|eid| ExternalIdInfo {
                auth_provider: eid.auth_provider,
                external_id: eid.external_id,
            })
            .collect()
    });

    Ok(UserInfoV2 {
        name: user_id.to_string(),
        displayname: display_name,
        threepids,
        avatar_url: avatar_url.map(|u| u.to_string()),
        is_guest: db_user.is_guest,
        admin: db_user.is_admin,
        deactivated: db_user.deactivated_at.is_some(),
        shadow_banned: db_user.shadow_banned,
        locked: db_user.locked_at.is_some(),
        creation_ts: db_user.created_at.get() as i64,
        appservice_id: db_user.appservice_id,
        consent_version: db_user.consent_version,
        consent_ts: db_user.consent_at.map(|t| t.get() as i64),
        consent_server_notice_sent: db_user.consent_server_notice_sent,
        user_type: db_user.ty,
        external_ids,
    })
}

fn build_users_list(filter: &data::user::ListUsersFilter) -> crate::AppResult<UsersListResponse> {
    let (users, total) = data::user::list_users(filter)?;
    let limit = filter.limit.unwrap_or(100) as usize;
    let from = filter.from.unwrap_or(0) as usize;

    let user_infos: Vec<UserInfoV2> = users
        .into_iter()
        .map(|db_user| {
            let uid = &db_user.id;
            let display_name = data::user::display_name(uid).ok().flatten();
            let avatar_url = data::user::avatar_url(uid).ok().flatten();

            UserInfoV2 {
                name: uid.to_string(),
                displayname: display_name,
                threepids: None, // Not included in list response for performance
                avatar_url: avatar_url.map(|u| u.to_string()),
                is_guest: db_user.is_guest,
                admin: db_user.is_admin,
                deactivated: db_user.deactivated_at.is_some(),
                shadow_banned: db_user.shadow_banned,
                locked: db_user.locked_at.is_some(),
                creation_ts: db_user.created_at.get() as i64,
                appservice_id: db_user.appservice_id,
                consent_version: db_user.consent_version,
                consent_ts: db_user.consent_at.map(|t| t.get() as i64),
                consent_server_notice_sent: db_user.consent_server_notice_sent,
                user_type: db_user.ty,
                external_ids: None,
            }
        })
        .collect();

    let next_token = if user_infos.len() >= limit {
        Some((from + user_infos.len()).to_string())
    } else {
        None
    };

    Ok(UsersListResponse {
        users: user_infos,
        next_token,
        total,
    })
}

// ============================================================================
// Phase 1: MAS Critical Endpoints
// ============================================================================

/// GET /_synapse/admin/v2/users/{user_id}
///
/// Get details of a single user
#[endpoint]
pub async fn get_user_v2(user_id: PathParam<OwnedUserId>) -> JsonResult<UserInfoV2> {
    let user_id = user_id.into_inner();
    json_ok(build_user_info(&user_id)?)
}

/// PUT /_synapse/admin/v2/users/{user_id}
///
/// Create or modify a user
#[endpoint]
pub async fn put_user_v2(
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<PutUserReqBody>,
) -> JsonResult<UserInfoV2> {
    let user_id = user_id.into_inner();
    let body = body.into_inner();

    // Check if user exists
    let user_exists = data::user::user_exists(&user_id).unwrap_or(false);

    if !user_exists {
        // Create new user
        user::create_user(user_id.clone(), body.password.as_deref())?;
    } else {
        // Update password if provided
        if let Some(password) = &body.password {
            let hash = crate::utils::hash_password(password)?;
            data::user::set_password(&user_id, &hash)?;

            // Logout devices if requested
            if body.logout_devices.unwrap_or(true) {
                data::user::remove_all_devices(&user_id)?;
            }
        }
    }

    // Update display name
    if let Some(display_name) = &body.displayname {
        data::user::set_display_name(&user_id, display_name)?;
    }

    // Update avatar
    if let Some(avatar_url) = &body.avatar_url {
        if let Ok(mxc_uri) = <&MxcUri>::try_from(avatar_url.as_str()) {
            data::user::set_avatar_url(&user_id, mxc_uri)?;
        }
    }

    // Update admin status
    if let Some(admin) = body.admin {
        data::user::set_admin(&user_id, admin)?;
    }

    // Update deactivated status
    if let Some(deactivated) = body.deactivated {
        if deactivated {
            data::user::deactivate(&user_id)?;
        }
    }

    // Update locked status
    if let Some(locked) = body.locked {
        data::user::set_locked(&user_id, locked, None)?;
    }

    // Update external IDs if provided
    if let Some(external_ids) = body.external_ids {
        let ids: Vec<(String, String)> = external_ids
            .into_iter()
            .map(|eid| (eid.auth_provider, eid.external_id))
            .collect();
        data::user::replace_external_ids(&user_id, &ids)?;
    }

    // Return updated user info
    json_ok(build_user_info(&user_id)?)
}

/// GET /_synapse/admin/v2/users
///
/// List all users with pagination and filtering
#[endpoint]
pub async fn list_users_v2(
    from: QueryParam<i64, false>,
    limit: QueryParam<i64, false>,
    user_id: QueryParam<String, false>,
    name: QueryParam<String, false>,
    guests: QueryParam<bool, false>,
    deactivated: QueryParam<bool, false>,
    admins: QueryParam<bool, false>,
    order_by: QueryParam<String, false>,
    dir: QueryParam<String, false>,
) -> JsonResult<UsersListResponse> {
    let name_filter = name.into_inner().or(user_id.into_inner());
    let filter = data::user::ListUsersFilter {
        from: from.into_inner(),
        limit: limit.into_inner(),
        name: name_filter,
        guests: guests.into_inner(),
        deactivated: deactivated.into_inner(),
        admins: admins.into_inner(),
        user_types: None,
        order_by: order_by.into_inner(),
        dir: dir.into_inner(),
    };

    json_ok(build_users_list(&filter)?)
}

/// GET /_synapse/admin/v3/users
///
/// Same as v2 but with different deactivated parameter handling
#[endpoint]
pub async fn list_users_v3(
    from: QueryParam<i64, false>,
    limit: QueryParam<i64, false>,
    user_id: QueryParam<String, false>,
    name: QueryParam<String, false>,
    guests: QueryParam<bool, false>,
    deactivated: QueryParam<bool, false>,
    admins: QueryParam<bool, false>,
    order_by: QueryParam<String, false>,
    dir: QueryParam<String, false>,
) -> JsonResult<UsersListResponse> {
    // v3 uses deactivated=true/false differently
    // In v2, deactivated=true means show only deactivated users
    // In v3, not_deactivated parameter is used instead
    let name_filter = name.into_inner().or(user_id.into_inner());
    let filter = data::user::ListUsersFilter {
        from: from.into_inner(),
        limit: limit.into_inner(),
        name: name_filter,
        guests: guests.into_inner(),
        deactivated: deactivated.into_inner(),
        admins: admins.into_inner(),
        user_types: None,
        order_by: order_by.into_inner(),
        dir: dir.into_inner(),
    };

    json_ok(build_users_list(&filter)?)
}

/// POST /_synapse/admin/v1/users/{user_id}/_allow_cross_signing_replacement_without_uia
///
/// Allow a user to replace cross-signing keys without UIA
#[endpoint]
pub async fn allow_cross_signing_replacement(
    user_id: PathParam<OwnedUserId>,
) -> JsonResult<CrossSigningResponse> {
    let user_id = user_id.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    // In a full implementation, this would set a flag allowing the user
    // to replace their cross-signing keys without User-Interactive Authentication.
    // For now, we just acknowledge the request.
    // TODO: Implement actual cross-signing replacement flag storage

    json_ok(CrossSigningResponse {})
}

// ============================================================================
// Phase 2: User Management Endpoints
// ============================================================================

/// POST /_synapse/admin/v1/deactivate/{user_id}
///
/// Deactivate a user account
#[endpoint]
pub async fn deactivate_user(
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<DeactivateReqBody>,
) -> EmptyResult {
    let user_id = user_id.into_inner();
    let body = body.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    // Get all joined rooms before deactivation
    let joined_rooms = data::user::joined_rooms(&user_id)?;

    // Perform full deactivation
    user::full_user_deactivate(&user_id, &joined_rooms).await?;

    // Erase user data if requested
    if body.erase.unwrap_or(false) {
        // Delete all media
        user::delete_all_media(&user_id).await?;
    }

    empty_ok()
}

/// POST /_synapse/admin/v1/reset_password/{user_id}
///
/// Reset a user's password
#[endpoint]
pub async fn reset_password(
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<ResetPasswordReqBody>,
) -> EmptyResult {
    let user_id = user_id.into_inner();
    let body = body.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    // Hash and set new password
    let hash = crate::utils::hash_password(&body.new_password)?;
    data::user::set_password(&user_id, &hash)?;

    // Logout all devices if requested (default true)
    if body.logout_devices.unwrap_or(true) {
        data::user::remove_all_devices(&user_id)?;
    }

    empty_ok()
}

/// GET /_synapse/admin/v1/users/{user_id}/admin
///
/// Get admin status of a user
#[endpoint]
pub async fn get_admin_status(user_id: PathParam<OwnedUserId>) -> JsonResult<AdminStatusResponse> {
    let user_id = user_id.into_inner();

    let is_admin = data::user::is_admin(&user_id).map_err(|_| MatrixError::not_found("User not found"))?;

    json_ok(AdminStatusResponse { admin: is_admin })
}

/// PUT /_synapse/admin/v1/users/{user_id}/admin
///
/// Set admin status of a user
#[endpoint]
pub async fn set_admin_status(
    user_id: PathParam<OwnedUserId>,
    body: JsonBody<AdminStatusReqBody>,
) -> EmptyResult {
    let user_id = user_id.into_inner();
    let body = body.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    data::user::set_admin(&user_id, body.admin)?;

    empty_ok()
}

/// POST /_synapse/admin/v1/users/{user_id}/shadow_ban
///
/// Shadow ban a user
#[endpoint]
pub async fn shadow_ban_user(user_id: PathParam<OwnedUserId>) -> EmptyResult {
    let user_id = user_id.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    data::user::set_shadow_banned(&user_id, true)?;

    empty_ok()
}

/// DELETE /_synapse/admin/v1/users/{user_id}/shadow_ban
///
/// Remove shadow ban from a user
#[endpoint]
pub async fn unshadow_ban_user(user_id: PathParam<OwnedUserId>) -> EmptyResult {
    let user_id = user_id.into_inner();

    // Verify user exists
    if !data::user::user_exists(&user_id)? {
        return Err(MatrixError::not_found("User not found").into());
    }

    data::user::set_shadow_banned(&user_id, false)?;

    empty_ok()
}

// ============================================================================
// Router
// ============================================================================

pub fn router() -> Router {
    Router::new()
        // Phase 1: MAS Critical
        // v2/users/{user_id}
        .push(
            Router::with_path("v2/users/<user_id>")
                .get(get_user_v2)
                .put(put_user_v2),
        )
        // v2/users (list)
        .push(Router::with_path("v2/users").get(list_users_v2))
        // v3/users (list)
        .push(Router::with_path("v3/users").get(list_users_v3))
        // v1/users/{user_id}/_allow_cross_signing_replacement_without_uia
        .push(
            Router::with_path("v1/users/<user_id>/_allow_cross_signing_replacement_without_uia")
                .post(allow_cross_signing_replacement),
        )
        // Phase 2: User Management
        // v1/deactivate/{user_id}
        .push(Router::with_path("v1/deactivate/<user_id>").post(deactivate_user))
        // v1/reset_password/{user_id}
        .push(Router::with_path("v1/reset_password/<user_id>").post(reset_password))
        // v1/users/{user_id}/admin
        .push(
            Router::with_path("v1/users/<user_id>/admin")
                .get(get_admin_status)
                .put(set_admin_status),
        )
        // v1/users/{user_id}/shadow_ban
        .push(
            Router::with_path("v1/users/<user_id>/shadow_ban")
                .post(shadow_ban_user)
                .delete(unshadow_ban_user),
        )
}
