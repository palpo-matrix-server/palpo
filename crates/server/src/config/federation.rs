use serde::Deserialize;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "federation")]
#[derive(Clone, Debug, Deserialize)]
pub struct FederationConfig {
    /// Controls whether federation is allowed or not. It is not recommended to
    /// disable this after the fact due to potential federation breakage.
    #[serde(default = "default_true")]
    pub enable: bool,

    /// Allows federation requests to be made to itself
    ///
    /// This isn't intended and is very likely a bug if federation requests are
    /// being sent to yourself. This currently mainly exists for development
    /// purposes.
    #[serde(default)]
    pub allow_loopback: bool,

    /// Set this to true to allow federating device display names / allow
    /// external users to see your device display name. If federation is
    /// disabled entirely (`allow_federation`), this is inherently false. For
    /// privacy reasons, this is best left disabled.
    #[serde(default)]
    pub allow_device_name: bool,

    /// Config option to allow or disallow incoming federation requests that
    /// obtain the profiles of our local users from
    /// `/_matrix/federation/v1/query/profile`
    ///
    /// Increases privacy of your local user's such as display names, but some
    /// remote users may get a false "this user does not exist" error when they
    /// try to invite you to a DM or room. Also can protect against profile
    /// spiders.
    ///
    /// This is inherently false if `allow_federation` is disabled
    #[serde(default = "default_true")]
    pub allow_inbound_profile_lookup: bool,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            enable: true,
            allow_loopback: false,
            allow_device_name: false,
            allow_inbound_profile_lookup: true,
        }
    }
}
