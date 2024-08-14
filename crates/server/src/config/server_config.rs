use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::{env, fmt};

use super::DbConfig;
use crate::core::signatures::Ed25519KeyPair;
use crate::core::{OwnedServerName, RoomVersionId};
use crate::env_vars::{required_var, var, var_parsed};
use crate::{false_value, true_value};
use anyhow::{anyhow, Context};
use oauth2::{ClientId, ClientSecret};
use palpo_core::OwnedUserId;
use salvo::http::HeaderValue;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct WellKnownConfig {
    pub client: Option<String>,
    pub server: Option<OwnedServerName>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_server_addr")]
    pub server_addr: String,
    pub tls: Option<TlsConfig>,

    pub server_name: OwnedServerName,
    pub db: DbConfig,
    #[serde(default = "true_value")]
    pub enable_lightning_bolt: bool,
    #[serde(default = "true_value")]
    pub allow_check_for_updates: bool,
    #[serde(default = "default_pdu_cache_capacity")]
    pub pdu_cache_capacity: u32,
    #[serde(default = "default_cleanup_second_interval")]
    pub cleanup_second_interval: u32,
    #[serde(default = "default_max_request_size")]
    pub max_request_size: u32,
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: u16,
    #[serde(default = "default_max_fetch_prev_events")]
    pub max_fetch_prev_events: u16,
    #[serde(default = "false_value")]
    pub allow_registration: bool,
    pub registration_token: Option<String>,
    #[serde(default = "true_value")]
    pub allow_encryption: bool,
    #[serde(default = "false_value")]
    pub allow_federation: bool,
    #[serde(default = "true_value")]
    pub allow_room_creation: bool,
    #[serde(default = "true_value")]
    pub allow_unstable_room_versions: bool,
    #[serde(default = "default_room_version")]
    pub room_version: RoomVersionId,
    pub well_known_client: Option<String>,
    #[serde(default = "false_value")]
    pub allow_jaeger: bool,
    #[serde(default = "false_value")]
    pub tracing_flame: bool,
    // #[serde(default)]
    // pub proxy: ProxyConfig,
    pub jwt_secret: Option<String>,
    #[serde(default = "default_trusted_servers")]
    pub trusted_servers: Vec<OwnedServerName>,
    #[serde(default = "default_log")]
    pub log: String,
    #[serde(default)]
    pub turn_username: String,
    #[serde(default)]
    pub turn_password: String,
    #[serde(default = "Vec::new")]
    pub turn_uris: Vec<String>,
    #[serde(default)]
    pub turn_secret: String,
    #[serde(default = "default_turn_ttl")]
    pub turn_ttl: u64,

    pub emergency_password: Option<String>,

    #[serde(default = "false_value")]
    pub allow_local_presence: bool,
    #[serde(default = "false_value")]
    pub allow_incoming_presence: bool,
    #[serde(default = "false_value")]
    pub allow_outgoing_presence: bool,
    #[serde(default = "default_presence_idle_timeout_s")]
    pub presence_idle_timeout_s: u64,
    #[serde(default = "default_presence_offline_timeout_s")]
    pub presence_offline_timeout_s: u64,

    #[serde(default = "default_space_path")]
    pub space_path: String,

    pub keypair: String,

    pub well_known: WellKnownConfig,
}

fn default_space_path() -> String {
    "./space".into()
}

impl fmt::Display for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Prepare a list of config values to show
        let lines = [
            ("Server name", self.server_name.host()),
            ("PDU cache capacity", &self.pdu_cache_capacity.to_string()),
            ("Cleanup interval in seconds", &self.cleanup_second_interval.to_string()),
            ("Maximum request size", &self.max_request_size.to_string()),
            ("Maximum concurrent requests", &self.max_concurrent_requests.to_string()),
            ("Allow registration", &self.allow_registration.to_string()),
            ("Enabled lightning bolt", &self.enable_lightning_bolt.to_string()),
            ("Allow encryption", &self.allow_encryption.to_string()),
            ("Allow federation", &self.allow_federation.to_string()),
            ("Allow room creation", &self.allow_room_creation.to_string()),
            (
                "JWT secret",
                match self.jwt_secret {
                    Some(_) => "set",
                    None => "not set",
                },
            ),
            ("Trusted servers", {
                let mut lst = vec![];
                for server in &self.trusted_servers {
                    lst.push(server.host());
                }
                &lst.join(", ")
            }),
            (
                "TURN username",
                if self.turn_username.is_empty() {
                    "not set"
                } else {
                    &self.turn_username
                },
            ),
            ("TURN password", {
                if self.turn_password.is_empty() {
                    "not set"
                } else {
                    "set"
                }
            }),
            ("TURN secret", {
                if self.turn_secret.is_empty() {
                    "not set"
                } else {
                    "set"
                }
            }),
            ("Turn TTL", &self.turn_ttl.to_string()),
            ("Turn URIs", {
                let mut lst = vec![];
                for item in self.turn_uris.iter().cloned().enumerate() {
                    let (_, uri): (usize, String) = item;
                    lst.push(uri);
                }
                &lst.join(", ")
            }),
        ];

        let mut msg: String = "Active config values:\n\n".to_owned();

        for line in lines.into_iter().enumerate() {
            msg += &format!("{}: {}\n", line.1 .0, line.1 .1);
        }

        write!(f, "{msg}")
    }
}

#[derive(Clone, Debug, Default)]
pub struct AllowedOrigins(Vec<String>);

impl AllowedOrigins {
    pub fn from_env() -> anyhow::Result<Self> {
        let allowed_origins = required_var("WEB_ALLOWED_ORIGINS")?
            .split(',')
            .map(ToString::to_string)
            .collect();

        Ok(Self(allowed_origins))
    }

    pub fn contains(&self, value: &HeaderValue) -> bool {
        self.0.iter().any(|it| it == value)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TlsConfig {
    pub certs: String,
    pub key: String,
}

fn default_server_addr() -> String {
    "127.0.0.1:8008".into()
}

fn default_database_backend() -> String {
    "sqlite".to_owned()
}

fn default_db_cache_capacity_mb() -> f64 {
    300.0
}

fn default_palpo_cache_capacity_modifier() -> f64 {
    1.0
}

fn default_pdu_cache_capacity() -> u32 {
    150_000
}

fn default_cleanup_second_interval() -> u32 {
    60 // every minute
}

fn default_max_request_size() -> u32 {
    20 * 1024 * 1024 // Default to 20 MB
}

fn default_max_concurrent_requests() -> u16 {
    100
}

fn default_max_fetch_prev_events() -> u16 {
    100_u16
}

fn default_trusted_servers() -> Vec<OwnedServerName> {
    vec![OwnedServerName::try_from("matrix.org").unwrap()]
}

fn default_log() -> String {
    "warn,state=warn,_=off,sled=off".to_owned()
}

fn default_turn_ttl() -> u64 {
    60 * 60 * 24
}

fn default_presence_idle_timeout_s() -> u64 {
    5 * 60
}

fn default_presence_offline_timeout_s() -> u64 {
    15 * 60
}

pub fn default_room_version() -> RoomVersionId {
    RoomVersionId::V10
}
