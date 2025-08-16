//! # OIDC (OpenID Connect) Authentication Module
//!
//! This module implements OAuth 2.0 Authorization Code flow with OpenID Connect (OIDC) 
//! for Matrix server authentication using Google as the identity provider.
//!
//! ## Overview
//!
//! The OIDC authentication system allows users to log into the Matrix server using their
//! Google accounts, eliminating the need for separate Matrix passwords. This implementation
//! follows the OAuth 2.0 Authorization Code flow with PKCE-like state protection.
//!
//! ## Authentication Flow
//!
//! ### 1. Authorization Request (`GET /_matrix/client/*/oidc/auth`)
//! - Client initiates authentication by requesting authorization URL
//! - Server generates a random `state` parameter for CSRF protection
//! - Server stores the state in an HTTP-only cookie with 10-minute expiration
//! - Server redirects client to Google's authorization endpoint with:
//!   - `client_id`: Google OAuth application ID
//!   - `redirect_uri`: Callback URL for this server
//!   - `response_type`: "code" (Authorization Code flow)
//!   - `scope`: "openid email profile" (OIDC + user info)
//!   - `state`: CSRF protection token
//!
//! ### 2. User Authorization (Google)
//! - User is redirected to Google's login page
//! - User authenticates with Google credentials
//! - User grants permission to access profile information
//! - Google redirects back to server with authorization code
//!
//! ### 3. Authorization Callback (`GET /_matrix/client/*/oidc/callback`)
//! - Server receives authorization code and state from Google
//! - Server validates state parameter against stored cookie (CSRF protection)
//! - Server exchanges authorization code for access token via Google's token endpoint
//! - Server uses access token to fetch user profile from Google's userinfo endpoint
//! - Server creates or retrieves Matrix user account based on Google user ID
//! - Server generates Matrix access token and device ID
//! - Server returns Matrix authentication credentials to client
//!
//! ### 4. JWT Token Login (`POST /_matrix/client/*/oidc/login`) - Alternative Flow
//! - Alternative method for clients that can obtain Google JWT tokens directly
//! - Currently returns "not implemented" - requires JWT validation middleware
//! - Intended for mobile apps or SPAs with Google SDK integration

use cookie::time::Duration;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use reqwest;
use url::Url;

use crate::{
    core::{MatrixError, UnixMillis, OwnedUserId, OwnedDeviceId},
    data::user::DbUser,
    JsonResult, json_ok, AppResult,
};

const GOOGLE_ISSUER_URL: &str = "https://accounts.google.com";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

// TODO: These should come from configuration
const CLIENT_ID: &str = "your-google-client-id.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "your-google-client-secret";
const REDIRECT_URI: &str = "http://localhost:8080/oidc/callback";

/// OIDC provider type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum OidcProvider {
    Google,
}

impl FromStr for OidcProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "google" => Ok(OidcProvider::Google),
            _ => Err(format!("Unknown OIDC provider: {}", s)),
        }
    }
}

impl OidcProvider {
    pub fn issuer_url(&self) -> &'static str {
        match self {
            OidcProvider::Google => GOOGLE_ISSUER_URL,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            OidcProvider::Google => "google",
        }
    }
}

/// JWT Claims structure for OIDC
#[derive(Debug, Serialize, Deserialize)]
pub struct OidcClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub email_verified: Option<bool>,
    pub exp: i64,
    pub iat: i64,
    pub iss: String,
    pub aud: String,
}

/// OIDC user information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OidcUserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub email_verified: Option<bool>,
}

/// Google OAuth token response
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    id_token: Option<String>,
}

/// Google user info response
#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    id: String,
    email: Option<String>,
    name: Option<String>,
    picture: Option<String>,
    verified_email: Option<bool>,
}

impl From<&OidcClaims> for OidcUserInfo {
    fn from(claims: &OidcClaims) -> Self {
        Self {
            sub: claims.sub.clone(),
            email: claims.email.clone(),
            name: claims.name.clone(),
            picture: claims.picture.clone(),
            email_verified: claims.email_verified,
        }
    }
}

impl From<GoogleUserInfoResponse> for OidcUserInfo {
    fn from(info: GoogleUserInfoResponse) -> Self {
        Self {
            sub: info.id,
            email: info.email,
            name: info.name,
            picture: info.picture,
            email_verified: info.verified_email,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcStatusResponse {
    pub enabled: bool,
    pub provider: String,
    pub issuer_url: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcAuthResponse {
    pub auth_url: String,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OidcLoginResponse {
    pub user_id: String,
    pub access_token: String,
    pub device_id: String,
    pub home_server: String,
}

/// `GET /_matrix/client/*/oidc/status`
///
/// Get OIDC provider status and configuration.
/// Returns information about the enabled OIDC provider and its configuration.
#[endpoint]
pub async fn oidc_status() -> JsonResult<OidcStatusResponse> {
    json_ok(OidcStatusResponse {
        enabled: true,
        provider: "google".to_string(),
        issuer_url: GOOGLE_ISSUER_URL.to_string(),
    })
}

/// `GET /_matrix/client/*/oidc/auth`
///
/// Start OAuth authorization flow by redirecting to the identity provider.
/// Initiates the OAuth 2.0 Authorization Code flow with CSRF protection.
#[endpoint]
pub async fn oidc_auth(res: &mut Response) -> AppResult<()> {
    // Generate random state for CSRF protection
    let state = crate::utils::random_string(32);
    
    // Build authorization URL
    let mut auth_url = Url::parse(GOOGLE_AUTH_URL)
        .map_err(|_| MatrixError::unknown("Failed to parse auth URL"))?;
    
    auth_url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state);

    // Store state in session/cookie for later verification
    res.add_cookie(
        salvo::http::cookie::Cookie::build(("oidc_state", state.clone()))
            .http_only(true)
            .secure(false) // Set to true in production with HTTPS
            .same_site(salvo::http::cookie::SameSite::Lax)
            .max_age(Duration::minutes(10))
            .build()
    );
    
    // Redirect to Google
    res.render(Redirect::found(auth_url.to_string()));
    Ok(())
}

/// `GET /_matrix/client/*/oidc/callback`
///
/// Handle OAuth callback from the identity provider.
/// Processes the authorization code and completes the authentication flow.
/// Query parameters: `code` (authorization code) and `state` (CSRF token).
#[endpoint]
pub async fn oidc_callback(req: &mut Request) -> JsonResult<OidcLoginResponse> {
    // Extract query parameters
    let code = req.query::<String>("code")
        .ok_or_else(|| MatrixError::invalid_param("Missing authorization code"))?;
    let state = req.query::<String>("state")
        .ok_or_else(|| MatrixError::invalid_param("Missing state parameter"))?;
    
    // Verify state parameter (CSRF protection)
    let stored_state = req.cookie("oidc_state")
        .map(|cookie| cookie.value().to_string())
        .ok_or_else(|| MatrixError::forbidden("Invalid or missing state", None))?;
    
    if state != stored_state {
        return Err(MatrixError::forbidden("State parameter mismatch", None).into());
    }

    // Exchange authorization code for tokens
    let token_response = exchange_code_for_tokens(&code).await?;
    
    // Get user info from Google
    let user_info = get_google_user_info(&token_response.access_token).await?;
    
    // Generate Matrix user ID
    let config = crate::config::get();
    let matrix_user_id = generate_matrix_user_id(&user_info, config.server_name.as_str())?;
    let display_name = generate_display_name(&user_info);

    // Create or get user
    let user = create_or_get_user(&matrix_user_id, &display_name, &user_info).await?;

    // Create device and access token
    let device_id = format!("OIDC_{}", crate::utils::random_string(8));
    let access_token = create_access_token_for_user(&user, &device_id).await?;

    json_ok(OidcLoginResponse {
        user_id: matrix_user_id,
        access_token,
        device_id,
        home_server: config.server_name.to_string(),
    })
}

/// `POST /_matrix/client/*/oidc/login`
///
/// Login using OIDC JWT token (alternative authentication method).
/// Currently not implemented - requires JWT validation middleware setup.
/// Intended for mobile apps or SPAs with direct Google SDK integration.
#[endpoint]
pub async fn oidc_login(_depot: &mut Depot) -> JsonResult<OidcLoginResponse> {
    // For now, return an error since we need JWT auth middleware setup
    Err(MatrixError::unknown("JWT login not implemented yet").into())
}

/// Generate Matrix user ID from OIDC user info
pub fn generate_matrix_user_id(user_info: &OidcUserInfo, server_name: &str) -> Result<String, MatrixError> {
    let localpart = if let Some(email) = &user_info.email {
        email.split('@').next().unwrap_or(&user_info.sub)
    } else {
        &user_info.sub[..std::cmp::min(8, user_info.sub.len())]
    };
    
    let sanitized = localpart
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect::<String>()
        .to_lowercase();
    
    if sanitized.is_empty() {
        return Err(MatrixError::invalid_param("Cannot generate valid Matrix user ID"));
    }
    
    let prefixed_localpart = format!("google_{}", sanitized);
    Ok(format!("@{}:{}", prefixed_localpart, server_name))
}

/// Generate display name from OIDC user info
pub fn generate_display_name(user_info: &OidcUserInfo) -> String {
    user_info.name.clone()
        .or_else(|| user_info.email.clone())
        .unwrap_or_else(|| format!("Google User {}", &user_info.sub[..std::cmp::min(8, user_info.sub.len())]))
}

/// Exchange authorization code for access token
async fn exchange_code_for_tokens(code: &str) -> Result<GoogleTokenResponse, MatrixError> {
    let client = reqwest::Client::new();
    
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
    ];

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| MatrixError::unknown(format!("Token exchange failed: {}", e)))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(MatrixError::unknown(format!("Token exchange error: {}", error_text)));
    }

    response
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|e| MatrixError::unknown(format!("Failed to parse token response: {}", e)))
}

/// Get user information from Google
async fn get_google_user_info(access_token: &str) -> Result<OidcUserInfo, MatrixError> {
    let client = reqwest::Client::new();
    
    let response = client
        .get(GOOGLE_USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| MatrixError::unknown(format!("User info request failed: {}", e)))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(MatrixError::unknown(format!("User info error: {}", error_text)));
    }

    let user_info_response = response
        .json::<GoogleUserInfoResponse>()
        .await
        .map_err(|e| MatrixError::unknown(format!("Failed to parse user info: {}", e)))?;

    Ok(OidcUserInfo::from(user_info_response))
}

/// Create or get user
async fn create_or_get_user(
    user_id: &str,
    display_name: &str,
    user_info: &OidcUserInfo,
) -> Result<DbUser, MatrixError> {
    use crate::core::identifiers::UserId;
    use crate::data::connect;
    use diesel::prelude::*;
    use crate::data::schema::*;

    let parsed_user_id = UserId::parse(user_id)
        .map_err(|_| MatrixError::invalid_param("Invalid user ID format"))?;

    let mut conn = connect()
        .map_err(|_| MatrixError::unknown("Database connection failed"))?;

    // Try to find existing user
    if let Ok(existing_user) = users::table
        .filter(users::id.eq(&parsed_user_id))
        .first::<DbUser>(&mut conn)
    {
        return Ok(existing_user);
    }

    // Create new user
    let new_user = crate::data::user::NewDbUser {
        id: parsed_user_id,
        ty: Some("oidc".to_string()),
        is_admin: false,
        is_guest: false,
        appservice_id: None,
        created_at: UnixMillis::now(),
    };

    let user = diesel::insert_into(users::table)
        .values(&new_user)
        .get_result::<DbUser>(&mut conn)
        .map_err(|e| MatrixError::unknown(format!("Failed to create user: {}", e)))?;

    // Set user profile (display name and avatar)
    if let Err(e) = set_user_profile(&user.id, display_name, user_info.picture.as_deref()).await {
        tracing::warn!("Failed to set user profile: {}", e);
    }

    Ok(user)
}

async fn set_user_profile(user_id: &OwnedUserId, display_name: &str, avatar_url: Option<&str>) -> Result<(), MatrixError> {
    use crate::data::connect;
    use diesel::prelude::*;
    use crate::data::schema::*;

    let mut conn = connect()
        .map_err(|_| MatrixError::unknown("Database connection failed"))?;

    // Set display name
    diesel::insert_into(user_profiles::table)
        .values((
            user_profiles::user_id.eq(user_id),
            user_profiles::display_name.eq(Some(display_name)),
            user_profiles::avatar_url.eq(avatar_url),
        ))
        .on_conflict(user_profiles::user_id)
        .do_update()
        .set((
            user_profiles::display_name.eq(Some(display_name)),
            user_profiles::avatar_url.eq(avatar_url),
        ))
        .execute(&mut conn)
        .map_err(|e| MatrixError::unknown(format!("Failed to set user profile: {}", e)))?;

    Ok(())
}

async fn create_access_token_for_user(user: &DbUser, device_id: &str) -> Result<String, MatrixError> {
    use crate::data::connect;
    use diesel::prelude::*;
    use crate::data::schema::*;

    let parsed_device_id: OwnedDeviceId = device_id.try_into()
        .map_err(|_| MatrixError::invalid_param("Invalid device ID format"))?;

    let mut conn = connect()
        .map_err(|_| MatrixError::unknown("Database connection failed"))?;

    // Create device if it doesn't exist
    let new_device = crate::data::user::NewDbUserDevice {
        user_id: user.id.clone(),
        device_id: parsed_device_id.clone(),
        display_name: Some("OIDC Login".to_string()),
        user_agent: Some("OIDC/1.0".to_string()),
        is_hidden: false,
        last_seen_ip: None,
        last_seen_at: Some(UnixMillis::now()),
        created_at: UnixMillis::now(),
    };

    diesel::insert_into(user_devices::table)
        .values(&new_device)
        .on_conflict((user_devices::user_id, user_devices::device_id))
        .do_update()
        .set(user_devices::last_seen_at.eq(Some(UnixMillis::now())))
        .execute(&mut conn)
        .map_err(|e| MatrixError::unknown(format!("Failed to create/update device: {}", e)))?;

    // Generate access token
    let access_token = crate::utils::random_string(64);
    
    let new_access_token = crate::data::user::NewDbAccessToken {
        user_id: user.id.clone(),
        device_id: parsed_device_id,
        token: access_token.clone(),
        puppets_user_id: None,
        last_validated: Some(UnixMillis::now()),
        refresh_token_id: None,
        is_used: false,
        expires_at: None,
        created_at: UnixMillis::now(),
    };

    diesel::insert_into(user_access_tokens::table)
        .values(&new_access_token)
        .execute(&mut conn)
        .map_err(|e| MatrixError::unknown(format!("Failed to create access token: {}", e)))?;

    Ok(access_token)
}
