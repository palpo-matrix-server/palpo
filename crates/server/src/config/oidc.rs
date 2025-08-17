use std::collections::BTreeMap;

use serde::Deserialize;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "oidc")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct OidcConfig {
    /// Enable OIDC authentication
    /// 
    /// When enabled, users can authenticate using external OIDC providers
    /// like Google, GitHub, etc. instead of Matrix passwords.
    /// 
    /// default: false
    #[serde(default)]
    pub enable: bool,

    /// OIDC provider configuration
    /// 
    /// Configure one or more OIDC providers for authentication.
    /// Each provider must have a unique name as the key.
    /// 
    /// example:
    /// [oidc.providers.google]
    /// issuer = "https://accounts.google.com"
    /// client_id = "your-client-id.apps.googleusercontent.com"
    /// client_secret = "your-client-secret"
    /// redirect_uri = "https://your-server.com/_matrix/client/v3/oidc/callback/google"
    /// 
    /// default: {}
    #[serde(default)]
    pub providers: BTreeMap<String, OidcProviderConfig>,

    /// Default provider to use when multiple providers are configured
    /// 
    /// This provider will be used for the main login flow.
    /// If not specified, the first provider in alphabetical order is used.
    /// 
    /// example: "google"
    /// 
    /// default: None
    pub default_provider: Option<String>,

    /// Allow automatic user registration from OIDC
    /// 
    /// When enabled, new users will be automatically created when they
    /// authenticate with OIDC for the first time. When disabled, only
    /// existing users can authenticate via OIDC.
    /// 
    /// default: true
    #[serde(default = "default_true")]
    pub allow_registration: bool,

    /// User ID mapping strategy
    /// 
    /// Controls how Matrix user IDs are generated from OIDC user information:
    /// - "email" - Use the email address localpart
    /// - "sub" - Use the OIDC subject identifier
    /// - "preferred_username" - Use the preferred_username claim
    /// 
    /// default: "email"
    #[serde(default = "default_user_mapping")]
    pub user_mapping: String,

    /// User ID prefix for OIDC users
    /// 
    /// All OIDC-created users will have this prefix added to their localpart
    /// to distinguish them from regular Matrix users.
    /// 
    /// example: "oidc_" (results in @oidc_john:example.com)
    /// 
    /// default: ""
    #[serde(default)]
    pub user_prefix: String,

    /// Require email verification
    /// 
    /// When enabled, only users with verified email addresses can authenticate.
    /// The email_verified claim from the OIDC provider is checked.
    /// 
    /// default: true
    #[serde(default = "default_true")]
    pub require_email_verified: bool,

    /// Session timeout in seconds
    /// 
    /// How long OIDC authentication state is kept in memory.
    /// After this time, users need to restart the authentication flow.
    /// 
    /// default: 600 (10 minutes)
    #[serde(default = "default_session_timeout")]
    pub session_timeout: u64,

    /// Enable PKCE (Proof Key for Code Exchange)
    /// 
    /// Enables PKCE for additional security in the OAuth flow.
    /// Recommended for all deployments, especially mobile/SPA clients.
    /// 
    /// default: true
    #[serde(default = "default_true")]
    pub enable_pkce: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OidcProviderConfig {
    /// OIDC provider issuer URL
    /// 
    /// The base URL of the OIDC provider. Must support OpenID Connect Discovery.
    /// 
    /// example: "https://accounts.google.com"
    pub issuer: String,

    /// OAuth 2.0 client ID
    /// 
    /// The client ID registered with the OIDC provider.
    /// 
    /// example: "123456-abc.apps.googleusercontent.com"
    pub client_id: String,

    /// OAuth 2.0 client secret
    /// 
    /// The client secret for authenticating with the OIDC provider.
    /// Keep this value secure and never expose it in logs.
    /// 
    /// example: "GOCSPX-abc123def456"
    pub client_secret: String,

    /// OAuth 2.0 redirect URI
    /// 
    /// The URI where the OIDC provider will redirect after authentication.
    /// Must be registered with the provider and use HTTPS in production.
    /// 
    /// example: "https://matrix.example.com/_matrix/client/v3/oidc/callback/google"
    pub redirect_uri: String,

    /// OAuth 2.0 scopes to request
    /// 
    /// List of scopes to request from the OIDC provider.
    /// "openid" is required, "email" and "profile" are recommended.
    /// 
    /// example: ["openid", "email", "profile"]
    /// 
    /// default: ["openid", "email", "profile"]
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,

    /// Additional authorization parameters
    /// 
    /// Extra parameters to include in the authorization request.
    /// Provider-specific options can be configured here.
    /// 
    /// example: { "prompt" = "select_account" }
    /// 
    /// default: {}
    #[serde(default)]
    pub additional_params: BTreeMap<String, String>,

    /// Skip TLS verification (INSECURE - for development only)
    /// 
    /// WARNING: This disables TLS certificate verification and should
    /// NEVER be used in production environments.
    /// 
    /// default: false
    #[serde(default)]
    pub skip_tls_verify: bool,

    /// Human-readable display name for this provider
    /// 
    /// Used in user interfaces to identify this authentication method.
    /// 
    /// example: "Sign in with Google"
    /// 
    /// default: provider key name
    pub display_name: Option<String>,

    /// Custom attribute mapping
    /// 
    /// Override the default mapping of OIDC claims to Matrix user attributes.
    /// Keys are Matrix attributes, values are OIDC claim names.
    /// 
    /// example: { "display_name" = "given_name", "avatar_url" = "picture" }
    /// 
    /// default: {}
    #[serde(default)]
    pub attribute_mapping: BTreeMap<String, String>,
}

fn default_user_mapping() -> String {
    "email".to_string()
}

fn default_session_timeout() -> u64 {
    600 // 10 minutes
}

fn default_scopes() -> Vec<String> {
    vec!["openid".to_string(), "email".to_string(), "profile".to_string()]
}
