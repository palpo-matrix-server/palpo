use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;


use serde::Deserialize;

use crate::core::serde::{default_false, default_true};use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "turn")]
#[derive(Clone, Debug, Deserialize, Default)]
pub struct TurnConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    /// Static TURN username to provide the client if not using a shared secret
    /// ("turn_secret"), It is recommended to use a shared secret over static
    /// credentials.
    #[serde(default)]
    pub username: String,

    /// Static TURN password to provide the client if not using a shared secret
    /// ("turn_secret"). It is recommended to use a shared secret over static
    /// credentials.
    ///
    /// display: sensitive
    #[serde(default)]
    pub password: String,

    /// Vector list of TURN URIs/servers to use.
    ///
    /// Replace "example.turn.uri" with your TURN domain, such as the coturn
    /// "realm" config option. If using TURN over TLS, replace the URI prefix
    /// "turn:" with "turns:".
    ///
    /// example: ["turn:example.turn.uri?transport=udp",
    /// "turn:example.turn.uri?transport=tcp"]
    ///
    /// default: []
    #[serde(default = "Vec::new")]
    pub uris: Vec<String>,

    /// TURN secret to use for generating the HMAC-SHA1 hash apart of username
    /// and password generation.
    ///
    /// This is more secure, but if needed you can use traditional static
    /// username/password credentials.
    ///
    /// display: sensitive
    #[serde(default)]
    pub secret: String,

    /// TURN secret to use that's read from the file path specified.
    ///
    /// This takes priority over "tsecret" first, and falls back to
    /// "secret" if invalid or failed to open.
    ///
    /// example: "/etc/palpo/.turn_secret"
    pub secret_file: Option<PathBuf>,

    /// TURN TTL, in seconds.
    ///
    /// default: 86400
    #[serde(default = "default_ttl")]
    pub ttl: u64,
    
	/// Allow guests/unauthenticated users to access TURN credentials.
	///
	/// This is the equivalent of Synapse's `turn_allow_guests` config option.
	/// This allows any unauthenticated user to call the endpoint
	/// `/_matrix/client/v3/voip/turnServer`.
	///
	/// It is unlikely you need to enable this as all major clients support
	/// authentication for this endpoint and prevents misuse of your TURN server
	/// from potential bots.
	#[serde(default)]
	pub allow_guests: bool,
}

fn default_ttl() -> u64 {
    60 * 60 * 24
}