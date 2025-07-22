use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;


use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Default)]
pub struct TurnConfig {
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
}

fn default_ttl() -> u64 {
    60 * 60 * 24
}