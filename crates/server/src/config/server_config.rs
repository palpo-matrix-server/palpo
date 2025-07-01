use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

use either::Either;
use regex::RegexSet;
use salvo::http::HeaderValue;
use serde::Deserialize;
use serde::de::IgnoredAny;

use super::{BlurhashConfig, JwtConfig, LdapConfig};
use crate::core::serde::{default_false, default_true};
use crate::core::{OwnedRoomOrAliasId, OwnedServerName, RoomVersionId};
use crate::data::DbConfig;
use crate::env_vars::required_var;
use crate::{AppError, AppResult};

const DEPRECATED_KEYS: &[&str; 0] = &[];

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
    #[serde(default = "default_false")]
    pub enable_lightning_bolt: bool,
    #[serde(default = "default_true")]
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
    #[serde(default = "default_false")]
    pub allow_registration: bool,

    /// Allow sending read receipts to remote servers.
    #[serde(default = "default_false")]
    pub allow_outgoing_read_receipts: bool,

    /// Allow outgoing typing updates to federation.
    #[serde(default = "default_true")]
    pub allow_outgoing_typing: bool,

    /// Allow incoming typing updates from federation.
    #[serde(default = "default_true")]
    pub allow_incoming_typing: bool,

    pub registration_token: Option<String>,
    #[serde(default = "default_true")]
    pub allow_encryption: bool,
    #[serde(default = "default_false")]
    pub allow_federation: bool,
    #[serde(default = "default_true")]
    pub allow_room_creation: bool,
    #[serde(default = "default_true")]
    pub allow_unstable_room_versions: bool,
    #[serde(default = "default_default_room_version")]
    pub default_room_version: RoomVersionId,
    pub well_known_client: Option<String>,
    #[serde(default = "default_false")]
    pub allow_jaeger: bool,
    #[serde(default = "default_false")]
    pub tracing_flame: bool,

    #[serde(default = "default_true")]
    pub enable_admin_room: bool,

    pub appservice_registration_dir: Option<String>,

    // #[serde(default)]
    // pub proxy: ProxyConfig,
    pub ldap: Option<LdapConfig>,

    pub jwt: Option<JwtConfig>,

    #[serde(default)]
    pub blurhashing: BlurhashConfig,

    #[serde(default = "default_trusted_servers")]
    pub trusted_servers: Vec<OwnedServerName>,
    #[serde(default = "default_rust_log")]
    pub rust_log: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// OpenID token expiration/TTL in seconds.
    ///
    /// These are the OpenID tokens that are primarily used for Matrix account
    /// integrations (e.g. Vector Integrations in Element), *not* OIDC/OpenID
    /// Connect/etc.
    ///
    /// default: 3600
    #[serde(default = "default_openid_token_ttl")]
    pub openid_token_ttl: u64,

    /// Allow an existing session to mint a login token for another client.
    /// This requires interactive authentication, but has security ramifications
    /// as a malicious client could use the mechanism to spawn more than one
    /// session.
    /// Enabled by default.
    #[serde(default = "default_true")]
    pub login_via_existing_session: bool,

    /// Login token expiration/TTL in milliseconds.
    ///
    /// These are short-lived tokens for the m.login.token endpoint.
    /// This is used to allow existing sessions to create new sessions.
    /// see login_via_existing_session.
    ///
    /// default: 120000
    #[serde(default = "default_login_token_ttl")]
    pub login_token_ttl: u64,

    #[serde(default = "default_refresh_token_ttl")]
    pub refresh_token_ttl: u64,

    #[serde(default = "default_session_ttl")]
    pub session_ttl: u64,

    /// Static TURN username to provide the client if not using a shared secret
    /// ("turn_secret"), It is recommended to use a shared secret over static
    /// credentials.
    #[serde(default)]
    pub turn_username: String,

    /// Static TURN password to provide the client if not using a shared secret
    /// ("turn_secret"). It is recommended to use a shared secret over static
    /// credentials.
    ///
    /// display: sensitive
    #[serde(default)]
    pub turn_password: String,

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
    pub turn_uris: Vec<String>,

    /// TURN secret to use for generating the HMAC-SHA1 hash apart of username
    /// and password generation.
    ///
    /// This is more secure, but if needed you can use traditional static
    /// username/password credentials.
    ///
    /// display: sensitive
    #[serde(default)]
    pub turn_secret: String,

    /// TURN secret to use that's read from the file path specified.
    ///
    /// This takes priority over "turn_secret" first, and falls back to
    /// "turn_secret" if invalid or failed to open.
    ///
    /// example: "/etc/conduwuit/.turn_secret"
    pub turn_secret_file: Option<PathBuf>,

    /// TURN TTL, in seconds.
    ///
    /// default: 86400
    #[serde(default = "default_turn_ttl")]
    pub turn_ttl: u64,

    /// List/vector of room IDs or room aliases that conduwuit will make newly
    /// registered users join. The rooms specified must be rooms that you have
    /// joined at least once on the server, and must be public.
    ///
    /// example: ["#conduwuit:puppygock.gay",
    /// "!eoIzvAvVwY23LPDay8:puppygock.gay"]
    ///
    /// default: []
    #[serde(default = "Vec::new")]
    pub auto_join_rooms: Vec<OwnedRoomOrAliasId>,

    /// Config option to automatically deactivate the account of any user who
    /// attempts to join a:
    /// - banned room
    /// - forbidden room alias
    /// - room alias or ID with a forbidden server name
    ///
    /// This may be useful if all your banned lists consist of toxic rooms or
    /// servers that no good faith user would ever attempt to join, and
    /// to automatically remediate the problem without any admin user
    /// intervention.
    ///
    /// This will also make the user leave all rooms. Federation (e.g. remote
    /// room invites) are ignored here.
    ///
    /// Defaults to false as rooms can be banned for non-moderation-related
    /// reasons and this performs a full user deactivation.
    #[serde(default)]
    pub auto_deactivate_banned_room_attempts: bool,

    pub emergency_password: Option<String>,

    /// Allow local (your server only) presence updates/requests.
    ///
    /// Note that presence on conduwuit is very fast unlike Synapse's. If using
    /// outgoing presence, this MUST be enabled.
    #[serde(default = "default_true")]
    pub allow_local_presence: bool,

    /// Allow incoming federated presence updates/requests.
    ///
    /// This option receives presence updates from other servers, but does not
    /// send any unless `allow_outgoing_presence` is true. Note that presence on
    /// conduwuit is very fast unlike Synapse's.
    #[serde(default = "default_true")]
    pub allow_incoming_presence: bool,

    /// Allow outgoing presence updates/requests.
    ///
    /// This option sends presence updates to other servers, but does not
    /// receive any unless `allow_incoming_presence` is true. Note that presence
    /// on conduwuit is very fast unlike Synapse's. If using outgoing presence,
    /// you MUST enable `allow_local_presence` as well.
    #[serde(default = "default_true")]
    pub allow_outgoing_presence: bool,

    /// How many seconds without presence updates before you become idle.
    /// Defaults to 5 minutes.
    ///
    /// default: 300
    #[serde(default = "default_presence_idle_timeout_s")]
    pub presence_idle_timeout_s: u64,

    /// How many seconds without presence updates before you become offline.
    /// Defaults to 30 minutes.
    ///
    /// default: 1800
    #[serde(default = "default_presence_offline_timeout_s")]
    pub presence_offline_timeout_s: u64,

    /// Controls whether admin room notices like account registrations, password
    /// changes, account deactivations, room directory publications, etc will be
    /// sent to the admin room. Update notices and normal admin command
    /// responses will still be sent.
    #[serde(default = "default_true")]
    pub admin_room_notices: bool,

    /// Config option to control maximum time federation user can indicate
    /// typing.
    ///
    /// default: 30
    #[serde(default = "default_typing_federation_timeout_s")]
    pub typing_federation_timeout_s: u64,

    /// Minimum time local client can indicate typing. This does not override a
    /// client's request to stop typing. It only enforces a minimum value in
    /// case of no stop request.
    ///
    /// default: 15
    #[serde(default = "default_typing_client_timeout_min_s")]
    pub typing_client_timeout_min_s: u64,

    /// Maximum time local client can indicate typing.
    ///
    /// default: 45
    #[serde(default = "default_typing_client_timeout_max_s")]
    pub typing_client_timeout_max_s: u64,

    /// Set this to true for palpo to compress HTTP response bodies using
    /// zstd. This option does nothing if palpo was not built with
    /// `zstd_compression` feature. Please be aware that enabling HTTP
    /// compression may weaken TLS. Most users should not need to enable this.
    /// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
    /// before deciding to enable this.
    #[serde(default)]
    pub zstd_compression: bool,

    /// Set this to true for palpo to compress HTTP response bodies using
    /// gzip. This option does nothing if palpo was not built with
    /// `gzip_compression` feature. Please be aware that enabling HTTP
    /// compression may weaken TLS. Most users should not need to enable this.
    /// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH before
    /// deciding to enable this.
    ///
    /// If you are in a large amount of rooms, you may find that enabling this
    /// is necessary to reduce the significantly large response bodies.
    #[serde(default)]
    pub gzip_compression: bool,

    /// Set this to true for palpo to compress HTTP response bodies using
    /// brotli. This option does nothing if palpo was not built with
    /// `brotli_compression` feature. Please be aware that enabling HTTP
    /// compression may weaken TLS. Most users should not need to enable this.
    /// See https://breachattack.com/ and https://wikipedia.org/wiki/BREACH
    /// before deciding to enable this.
    #[serde(default)]
    pub brotli_compression: bool,

    /// Set to true to allow user type "guest" registrations. Some clients like
    /// Element attempt to register guest users automatically.
    #[serde(default)]
    pub allow_guest_registration: bool,

    /// Set to true to log guest registrations in the admin room. Note that
    /// these may be noisy or unnecessary if you're a public homeserver.
    #[serde(default)]
    pub log_guest_registrations: bool,

    /// Set to true to allow guest registrations/users to auto join any rooms
    /// specified in `auto_join_rooms`.
    #[serde(default)]
    pub allow_guests_auto_join_rooms: bool,

    /// Enable the legacy unauthenticated Matrix media repository endpoints.
    /// These endpoints consist of:
    /// - /_matrix/media/*/config
    /// - /_matrix/media/*/upload
    /// - /_matrix/media/*/preview_url
    /// - /_matrix/media/*/download/*
    /// - /_matrix/media/*/thumbnail/*
    ///
    /// The authenticated equivalent endpoints are always enabled.
    ///
    /// Defaults to true for now, but this is highly subject to change, likely
    /// in the next release.
    #[serde(default = "default_true")]
    pub allow_legacy_media: bool,

    #[serde(default = "default_true")]
    pub freeze_legacy_media: bool,

    /// Check consistency of the media directory at startup:
    /// 1. When `media_compat_file_link` is enabled, this check will upgrade
    ///    media when switching back and forth between Conduit and palpo.
    ///    Both options must be enabled to handle this.
    /// 2. When media is deleted from the directory, this check will also delete
    ///    its database entry.
    ///
    /// If none of these checks apply to your use cases, and your media
    /// directory is significantly large setting this to false may reduce
    /// startup time.
    #[serde(default = "default_true")]
    pub media_startup_check: bool,

    /// Enable backward-compatibility with Conduit's media directory by creating
    /// symlinks of media.
    ///
    /// This option is only necessary if you plan on using Conduit again.
    /// Otherwise setting this to false reduces filesystem clutter and overhead
    /// for managing these symlinks in the directory. This is now disabled by
    /// default. You may still return to upstream Conduit but you have to run
    /// palpo at least once with this set to true and allow the
    /// media_startup_check to take place before shutting down to return to
    /// Conduit.
    #[serde(default)]
    pub media_compat_file_link: bool,

    /// Prune missing media from the database as part of the media startup
    /// checks.
    ///
    /// This means if you delete files from the media directory the
    /// corresponding entries will be removed from the database. This is
    /// disabled by default because if the media directory is accidentally moved
    /// or inaccessible, the metadata entries in the database will be lost with
    /// sadness.
    #[serde(default)]
    pub prune_missing_media: bool,

    /// Vector list of regex patterns of server names that palpo will refuse
    /// to download remote media from.
    ///
    /// example: ["badserver\.tld$", "badphrase", "19dollarfortnitecards"]
    ///
    /// default: []
    #[serde(default, with = "serde_regex")]
    pub prevent_media_downloads_from: RegexSet,

    /// List of forbidden server names via regex patterns that we will block
    /// incoming AND outgoing federation with, and block client room joins /
    /// remote user invites.
    ///
    /// This check is applied on the room ID, room alias, sender server name,
    /// sender user's server name, inbound federation X-Matrix origin, and
    /// outbound federation handler.
    ///
    /// Basically "global" ACLs.
    ///
    /// example: ["badserver\.tld$", "badphrase", "19dollarfortnitecards"]
    ///
    /// default: []
    #[serde(default, with = "serde_regex")]
    pub forbidden_remote_server_names: RegexSet,

    /// List of forbidden server names via regex patterns that we will block all
    /// outgoing federated room directory requests for. Useful for preventing
    /// our users from wandering into bad servers or spaces.
    ///
    /// example: ["badserver\.tld$", "badphrase", "19dollarfortnitecards"]
    ///
    /// default: []
    #[serde(default, with = "serde_regex")]
    pub forbidden_remote_room_directory_server_names: RegexSet,

    /// Vector list of IPv4 and IPv6 CIDR ranges / subnets *in quotes* that you
    /// do not want palpo to send outbound requests to. Defaults to
    /// RFC1918, unroutable, loopback, multicast, and testnet addresses for
    /// security.
    ///
    /// Please be aware that this is *not* a guarantee. You should be using a
    /// firewall with zones as doing this on the application layer may have
    /// bypasses.
    ///
    /// Currently this does not account for proxies in use like Synapse does.
    ///
    /// To disable, set this to be an empty vector (`[]`).
    ///
    /// Defaults to:
    /// ["127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12",
    /// "192.168.0.0/16", "100.64.0.0/10", "192.0.0.0/24", "169.254.0.0/16",
    /// "192.88.99.0/24", "198.18.0.0/15", "192.0.2.0/24", "198.51.100.0/24",
    /// "203.0.113.0/24", "224.0.0.0/4", "::1/128", "fe80::/10", "fc00::/7",
    /// "2001:db8::/32", "ff00::/8", "fec0::/10"]
    #[serde(default = "default_ip_range_denylist")]
    pub ip_range_denylist: Vec<String>,

    /// Optional IP address or network interface-name to bind as the source of
    /// URL preview requests. If not set, it will not bind to a specific
    /// address or interface.
    ///
    /// Interface names only supported on Linux, Android, and Fuchsia platforms;
    /// all other platforms can specify the IP address. To list the interfaces
    /// on your system, use the command `ip link show`.
    ///
    /// example: `"eth0"` or `"1.2.3.4"`
    ///
    /// default:
    #[serde(default, with = "either::serde_untagged_optional")]
    pub url_preview_bound_interface: Option<Either<IpAddr, String>>,

    /// Vector list of domains allowed to send requests to for URL previews.
    ///
    /// This is a *contains* match, not an explicit match. Putting "google.com"
    /// will match "https://google.com" and
    /// "http://mymaliciousdomainexamplegoogle.com" Setting this to "*" will
    /// allow all URL previews. Please note that this opens up significant
    /// attack surface to your server, you are expected to be aware of the risks
    /// by doing so.
    ///
    /// default: []
    #[serde(default)]
    pub url_preview_domain_contains_allowlist: Vec<String>,

    /// Vector list of explicit domains allowed to send requests to for URL
    /// previews.
    ///
    /// This is an *explicit* match, not a contains match. Putting "google.com"
    /// will match "https://google.com", "http://google.com", but not
    /// "https://mymaliciousdomainexamplegoogle.com". Setting this to "*" will
    /// allow all URL previews. Please note that this opens up significant
    /// attack surface to your server, you are expected to be aware of the risks
    /// by doing so.
    ///
    /// default: []
    #[serde(default)]
    pub url_preview_domain_explicit_allowlist: Vec<String>,

    /// Vector list of explicit domains not allowed to send requests to for URL
    /// previews.
    ///
    /// This is an *explicit* match, not a contains match. Putting "google.com"
    /// will match "https://google.com", "http://google.com", but not
    /// "https://mymaliciousdomainexamplegoogle.com". The denylist is checked
    /// first before allowlist. Setting this to "*" will not do anything.
    ///
    /// default: []
    #[serde(default)]
    pub url_preview_domain_explicit_denylist: Vec<String>,

    /// Vector list of URLs allowed to send requests to for URL previews.
    ///
    /// Note that this is a *contains* match, not an explicit match. Putting
    /// "google.com" will match "https://google.com/",
    /// "https://google.com/url?q=https://mymaliciousdomainexample.com", and
    /// "https://mymaliciousdomainexample.com/hi/google.com" Setting this to "*"
    /// will allow all URL previews. Please note that this opens up significant
    /// attack surface to your server, you are expected to be aware of the risks
    /// by doing so.
    ///
    /// default: []
    #[serde(default)]
    pub url_preview_url_contains_allowlist: Vec<String>,

    /// Maximum amount of bytes allowed in a URL preview body size when
    /// spidering. Defaults to 256KB in bytes.
    ///
    /// default: 256000
    #[serde(default = "default_url_preview_max_spider_size")]
    pub url_preview_max_spider_size: usize,

    /// Option to decide whether you would like to run the domain allowlist
    /// checks (contains and explicit) on the root domain or not. Does not apply
    /// to URL contains allowlist. Defaults to false.
    ///
    /// Example usecase: If this is enabled and you have "wikipedia.org" allowed
    /// in the explicit and/or contains domain allowlist, it will allow all
    /// subdomains under "wikipedia.org" such as "en.m.wikipedia.org" as the
    /// root domain is checked and matched. Useful if the domain contains
    /// allowlist is still too broad for you but you still want to allow all the
    /// subdomains under a root domain.
    #[serde(default)]
    pub url_preview_check_root_domain: bool,

    /// List of forbidden room aliases and room IDs as strings of regex
    /// patterns.
    ///
    /// Regex can be used or explicit contains matches can be done by just
    /// specifying the words (see example).
    ///
    /// This is checked upon room alias creation, custom room ID creation if
    /// used, and startup as warnings if any room aliases in your database have
    /// a forbidden room alias/ID.
    ///
    /// example: ["19dollarfortnitecards", "b[4a]droom", "badphrase"]
    ///
    /// default: []
    #[serde(default, with = "serde_regex")]
    pub forbidden_alias_names: RegexSet,

    #[serde(default = "default_space_path")]
    pub space_path: String,

    pub keypair: Option<KeypairConfig>,

    #[serde(default)]
    pub well_known: WellKnownConfig,

    pub auto_acme: Option<String>,
    #[serde(default = "default_false")]
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
    #[serde(default = "default_true")]
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

    /// List of forbidden username patterns/strings.
    ///
    /// Regex can be used or explicit contains matches can be done by just
    /// specifying the words (see example).
    ///
    /// This is checked upon username availability check, registration, and
    /// startup as warnings if any local users in your database have a forbidden
    /// username.
    ///
    /// example: ["administrator", "b[a4]dusernam[3e]", "badphrase"]
    ///
    /// default: []
    #[serde(default, with = "serde_regex")]
    pub forbidden_usernames: RegexSet,

    /// Retry failed and incomplete messages to remote servers immediately upon
    /// startup. This is called bursting. If this is disabled, said messages may
    /// not be delivered until more messages are queued for that server. Do not
    /// change this option unless server resources are extremely limited or the
    /// scale of the server's deployment is huge. Do not disable this unless you
    /// know what you are doing.
    #[serde(default = "default_true")]
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

    #[serde(flatten)]
    #[allow(clippy::zero_sized_map_values)]
    // this is a catchall, the map shouldn't be zero at runtime
    catch_others: BTreeMap<String, IgnoredAny>,
}
impl ServerConfig {
    pub fn check(&self) -> AppResult<()> {
        if cfg!(debug_assertions) {
            tracing::warn!("Note: conduwuit was built without optimisations (i.e. debug build)");
        }

        // if self
        //     .allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure
        // {
        //     tracing::warn!(
        //         "\n\nWARNING: \n\nTLS CERTIFICATE VALIDATION IS DISABLED, THIS IS HIGHLY INSECURE AND SHOULD NOT BE USED IN PRODUCTION.\n\n"
        //     );
        // }

        self.warn_deprecated();
        self.warn_unknown_key();

        // if self.sentry && self.sentry_endpoint.is_none() {
        //     return Err!(AppError::internal(
        //         "sentry_endpoint",
        //         "Sentry cannot be enabled without an endpoint set"
        //     ));
        // }

        // if cfg!(all(
        //     feature = "hardened_malloc",
        //     feature = "jemalloc",
        //     not(target_env = "msvc")
        // )) {
        //     tracing::warn!(
        //         "hardened_malloc and jemalloc compile-time features are both enabled, this causes \
        //          jemalloc to be used."
        //     );
        // }

        // if cfg!(not(unix)) && self.unix_socket_path.is_some() {
        //     return Err(AppError::internal(
        //         "UNIX socket support is only available on *nix platforms. Please remove \
        //          'unix_socket_path' from your config.",
        //     ));
        // }

        // if self.unix_socket_path.is_none() && self.get_bind_hosts().is_empty() {
        //     return Err(AppError::internal("No TCP addresses were specified to listen on"));
        // }

        // if self.unix_socket_path.is_none() && self.get_bind_ports().is_empty() {
        //     return EErr(AppError::internal("No ports were specified to listen on"));
        // }

        // if self.unix_socket_path.is_none() {
        //     self.get_bind_addrs().iter().for_each(|addr| {
        //         use std::path::Path;

        //         if addr.ip().is_loopback() {
        //             tracing::info!(
        //                 "Found loopback listening address {addr}, running checks if we're in a \
        //                  container."
        //             );

        //             if Path::new("/proc/vz").exists() /* Guest */ && !Path::new("/proc/bz").exists()
        //             /* Host */
        //             {
        //                 error!(
        //                     "You are detected using OpenVZ with a loopback/localhost listening \
        //                      address of {addr}. If you are using OpenVZ for containers and you use \
        //                      NAT-based networking to communicate with the host and guest, this will \
        //                      NOT work. Please change this to \"0.0.0.0\". If this is expected, you \
        //                      can ignore.",
        //                 );
        //             } else if Path::new("/.dockerenv").exists() {
        //                 error!(
        //                     "You are detected using Docker with a loopback/localhost listening \
        //                      address of {addr}. If you are using a reverse proxy on the host and \
        //                      require communication to conduwuit in the Docker container via \
        //                      NAT-based networking, this will NOT work. Please change this to \
        //                      \"0.0.0.0\". If this is expected, you can ignore.",
        //                 );
        //             } else if Path::new("/run/.containerenv").exists() {
        //                 error!(
        //                     "You are detected using Podman with a loopback/localhost listening \
        //                      address of {addr}. If you are using a reverse proxy on the host and \
        //                      require communication to conduwuit in the Podman container via \
        //                      NAT-based networking, this will NOT work. Please change this to \
        //                      \"0.0.0.0\". If this is expected, you can ignore.",
        //                 );
        //             }
        //         }
        //     });
        // }

        // yeah, unless the user built a debug build hopefully for local testing only
        if cfg!(not(debug_assertions)) && self.server_name == "your.server.name" {
            return Err(AppError::internal(
                "You must specify a valid server name for production usage of conduwuit.",
            ));
        }

        if self.emergency_password == Some(String::from("F670$2CP@Hw8mG7RY1$%!#Ic7YA")) {
            return Err(AppError::internal(
                "The public example emergency password is being used, this is insecure. Please \
                 change this.",
            ));
        }

        if self.emergency_password == Some(String::new()) {
            return Err(AppError::internal(
                "Emergency password was set to an empty string, this is not valid. Unset \
                 emergency_password to disable it or set it to a real password.",
            ));
        }

        // check if the user specified a registration token as `""`
        if self.registration_token == Some(String::new()) {
            return Err(AppError::internal(
                "Registration token was specified but is empty (\"\")",
            ));
        }

        // // check if we can read the token file path, and check if the file is empty
        // if self.registration_token_file.as_ref().is_some_and(|path| {
        //     let Ok(token) = std::fs::read_to_string(path).inspect_err(|e| {
        //         error!("Failed to read the registration token file: {e}");
        //     }) else {
        //         return true;
        //     };

        //     token == String::new()
        // }) {
        //     return Err(AppError::internal(
        //         "Registration token file was specified but is empty or failed to be read",
        //     ));
        // }

        if self.max_request_size < 10_000_000 {
            return Err(AppError::internal(
                "Max request size is less than 10MB. Please increase it as this is too low for \
                 operable federation.",
            ));
        }

        // check if user specified valid IP CIDR ranges on startup
        for cidr in &self.ip_range_denylist {
            if let Err(e) = ipaddress::IPAddress::parse(cidr) {
                return Err(AppError::internal(
                    "Parsing specified IP CIDR range from string failed: {e}.",
                ));
            }
        }

        //     if self.allow_registration
        //         && !self.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse
        //         && self.registration_token.is_none()
        //         && self.registration_token_file.is_none()
        //     {
        //         return Err(AppError::internal(
        //             "!! You have `allow_registration` enabled without a token configured in your config \
        //              which means you are allowing ANYONE to register on your conduwuit instance without \
        //              any 2nd-step (e.g. registration token). If this is not the intended behaviour, \
        //              please set a registration token. For security and safety reasons, conduwuit will \
        //              shut down. If you are extra sure this is the desired behaviour you want, please \
        //              set the following config option to true:
        // `yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse`",
        //         ));
        //     }

        // if self.allow_registration
        //     && self.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse
        //     && self.registration_token.is_none()
        //     && self.registration_token_file.is_none()
        // {
        //     warn!(
        //         "Open registration is enabled via setting \
        //          `yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse` and \
        //          `allow_registration` to true without a registration token configured. You are \
        //          expected to be aware of the risks now. If this is not the desired behaviour, \
        //          please set a registration token."
        //     );
        // }

        if self.allow_outgoing_presence && !self.allow_local_presence {
            return Err(AppError::internal(
                "Outgoing presence requires allowing local presence. Please enable \
                 'allow_local_presence' or disable outgoing presence.",
            ));
        }

        if self.url_preview_domain_contains_allowlist.contains(&"*".to_owned()) {
            warn!(
                "All URLs are allowed for URL previews via setting \
                 \"url_preview_domain_contains_allowlist\" to \"*\". This opens up significant \
                 attack surface to your server. You are expected to be aware of the risks by doing \
                 this."
            );
        }
        if self.url_preview_domain_explicit_allowlist.contains(&"*".to_owned()) {
            warn!(
                "All URLs are allowed for URL previews via setting \
                 \"url_preview_domain_explicit_allowlist\" to \"*\". This opens up significant \
                 attack surface to your server. You are expected to be aware of the risks by doing \
                 this."
            );
        }
        if self.url_preview_url_contains_allowlist.contains(&"*".to_owned()) {
            warn!(
                "All URLs are allowed for URL previews via setting \
                 \"url_preview_url_contains_allowlist\" to \"*\". This opens up significant attack \
                 surface to your server. You are expected to be aware of the risks by doing this."
            );
        }

        // if let Some(Either::Right(_)) = self.url_preview_bound_interface.as_ref() {
        //     if !matches!(OS, "android" | "fuchsia" | "linux") {
        //         return Err(AppError::internal(
        //             "url_preview_bound_interface",
        //             "Not a valid IP address. Interface names not supported on {OS}."
        //         ));
        //     }
        // }

        // if !Server::available_room_versions().any(|(version, _)| version == self.default_room_version) {
        //     return Err(AppError::internal(formmat!(
        //         "Room version {:?} is not available",
        //         self.default_room_version
        //     )));
        // }

        Ok(())
    }
    /// Iterates over all the keys in the config file and warns if there is a
    /// deprecated key specified
    fn warn_deprecated(&self) {
        debug!("Checking for deprecated config keys");
        let mut was_deprecated = false;
        for key in self
            .catch_others
            .keys()
            .filter(|key| DEPRECATED_KEYS.iter().any(|s| s == key))
        {
            warn!("Config parameter \"{}\" is deprecated, ignoring.", key);
            was_deprecated = true;
        }

        if was_deprecated {
            warn!(
                "Read conduwuit config documentation at https://conduwuit.puppyirl.gay/configuration.html and check your \
                 configuration if any new configuration parameters should be adjusted"
            );
        }
    }

    /// iterates over all the catchall keys (unknown config options) and warns
    /// if there are any.
    fn warn_unknown_key(&self) {
        debug!("Checking for unknown config keys");
        for key in self.catch_others.keys().filter(
            |key| "config".to_owned().ne(key.to_owned()), /* "config" is expected */
        ) {
            warn!("Config parameter \"{}\" is unknown to conduwuit, ignoring.", key);
        }
    }
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
                "JWT config",
                match self.jwt {
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

fn default_trusted_server_batch_size() -> usize {
    256
}

fn default_space_path() -> String {
    "./space".into()
}

fn default_startup_netburst_keep() -> i64 {
    50
}
fn default_login_token_ttl() -> u64 {
    2 * 60 * 1000
}
fn default_refresh_token_ttl() -> u64 {
    2 * 60 * 1000
}
fn default_session_ttl() -> u64 {
    60 * 60 * 1000
}
fn default_openid_token_ttl() -> u64 {
    60 * 60
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
    30 * 60
}

fn default_typing_federation_timeout_s() -> u64 {
    30
}

fn default_typing_client_timeout_min_s() -> u64 {
    15
}

fn default_typing_client_timeout_max_s() -> u64 {
    45
}

fn default_default_room_version() -> RoomVersionId {
    RoomVersionId::V11
}

fn default_url_preview_max_spider_size() -> usize {
    256_000 // 256KB
}
fn default_ip_range_denylist() -> Vec<String> {
    vec![
        "127.0.0.0/8".to_owned(),
        "10.0.0.0/8".to_owned(),
        "172.16.0.0/12".to_owned(),
        "192.168.0.0/16".to_owned(),
        "100.64.0.0/10".to_owned(),
        "192.0.0.0/24".to_owned(),
        "169.254.0.0/16".to_owned(),
        "192.88.99.0/24".to_owned(),
        "198.18.0.0/15".to_owned(),
        "192.0.2.0/24".to_owned(),
        "198.51.100.0/24".to_owned(),
        "203.0.113.0/24".to_owned(),
        "224.0.0.0/4".to_owned(),
        "::1/128".to_owned(),
        "fe80::/10".to_owned(),
        "fc00::/7".to_owned(),
        "2001:db8::/32".to_owned(),
        "ff00::/8".to_owned(),
        "fec0::/10".to_owned(),
    ]
}
