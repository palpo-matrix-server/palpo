use std::fmt;

use super::DbConfig;
use crate::core::{OwnedServerName, RoomVersionId};
use crate::env_vars::required_var;
use crate::{false_value, true_value};
use salvo::http::HeaderValue;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct WellKnownConfig {
    pub client: Option<String>,
    pub server: Option<OwnedServerName>,
}
#[derive(Clone, Debug, Deserialize, Default)]
pub struct KeypairConfig {
    pub document: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerConfig {
    pub tls: Option<TlsConfig>,

    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
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
    #[serde(default = "false_value")]
    pub allow_outgoing_read_receipts: bool,
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

    #[serde(default = "true_value")]
    pub enable_admin_room: bool,

    // #[serde(default)]
    // pub proxy: ProxyConfig,
    pub jwt_secret: Option<String>,
    #[serde(default = "default_trusted_servers")]
    pub trusted_servers: Vec<OwnedServerName>,
    #[serde(default = "default_rust_log")]
    pub rust_log: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
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
    /// Config option to control maximum time federation user can indicate
    /// typing.
    ///
    /// default: 30
    #[serde(default = "default_typing_federation_timeout_s")]
    pub typing_federation_timeout_s: u64,

    #[serde(default = "default_space_path")]
    pub space_path: String,

    pub keypair: Option<KeypairConfig>,

    #[serde(default)]
    pub well_known: WellKnownConfig,

    pub auto_acme: Option<String>,
    #[serde(default = "false_value")]
    pub enable_tls: bool,

    /// Whether to query the servers listed in trusted_servers first or query
    /// the origin server first. For best security, querying the origin server
    /// first is advised to minimize the exposure to a compromised trusted
    /// server. For maximum federation/join performance this can be set to true,
    /// however other options exist to query trusted servers first under
    /// specific high-load circumstances and should be evaluated before setting
    /// this to true.
    #[serde(default)]
    pub query_trusted_key_servers_first: bool,

    /// Whether to query the servers listed in trusted_servers first
    /// specifically on room joins. This option limits the exposure to a
    /// compromised trusted server to room joins only. The join operation
    /// requires gathering keys from many origin servers which can cause
    /// significant delays. Therefor this defaults to true to mitigate
    /// unexpected delays out-of-the-box. The security-paranoid or those
    /// willing to tolerate delays are advised to set this to false. Note that
    /// setting query_trusted_key_servers_first to true causes this option to
    /// be ignored.
    #[serde(default = "true_value")]
    pub query_trusted_key_servers_first_on_join: bool,

    /// Only query trusted servers for keys and never the origin server. This is
    /// intended for clusters or custom deployments using their trusted_servers
    /// as forwarding-agents to cache and deduplicate requests. Notary servers
    /// do not act as forwarding-agents by default, therefor do not enable this
    /// unless you know exactly what you are doing.
    #[serde(default)]
    pub only_query_trusted_key_servers: bool,

    /// Maximum number of keys to request in each trusted server batch query.
    ///
    /// default: 1024
    #[serde(default = "default_trusted_server_batch_size")]
    pub trusted_server_batch_size: usize,

	/// Retry failed and incomplete messages to remote servers immediately upon
	/// startup. This is called bursting. If this is disabled, said messages may
	/// not be delivered until more messages are queued for that server. Do not
	/// change this option unless server resources are extremely limited or the
	/// scale of the server's deployment is huge. Do not disable this unless you
	/// know what you are doing.
	#[serde(default = "true_value")]
	pub startup_netburst: bool,

	/// Messages are dropped and not reattempted. The `startup_netburst` option
	/// must be enabled for this value to have any effect. Do not change this
	/// value unless you know what you are doing. Set this value to -1 to
	/// reattempt every message without trimming the queues; this may consume
	/// significant disk. Set this value to 0 to drop all messages without any
	/// attempt at redelivery.
	///
	/// default: 50
	#[serde(default = "default_startup_netburst_keep")]
	pub startup_netburst_keep: i64,
}

fn default_trusted_server_batch_size() -> usize {
    256
}

fn default_space_path() -> String {
    "./space".into()
}

fn default_startup_netburst_keep() -> i64 { 50 }

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
                if self.turn_secret.is_empty() { "not set" } else { "set" }
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
            msg += &format!("{}: {}\n", line.1.0, line.1.1);
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

fn default_listen_addr() -> String {
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

fn default_rust_log() -> String {
    "warn".to_owned()
}

fn default_log_format() -> String {
    "json".to_owned()
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
fn default_typing_federation_timeout_s() -> u64 {
    30
}

pub fn default_room_version() -> RoomVersionId {
    RoomVersionId::V10
}
