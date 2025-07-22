use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use figment::Figment;
use figment::providers::{Env, Format, Yaml, Json, Toml};
use ipaddress::IPAddress;

mod server;
pub use server::*;
mod ldap;
pub use ldap::*;
mod jwt;
pub use jwt::*;
mod blurhash;
pub use blurhash::*;
mod url_preview;
pub use url_preview::*;
mod turn;
pub use turn::*;
mod media;
pub use media::*;

use crate::core::identifiers::*;
use crate::core::signatures::Ed25519KeyPair;
use crate::AppResult;
pub use crate::data::DbConfig;

pub static CONFIG: OnceLock<ServerConfig> = OnceLock::new();

pub static STABLE_ROOM_VERSIONS: LazyLock<Vec<RoomVersionId>> = LazyLock::new(|| {
    vec![
        RoomVersionId::V6,
        RoomVersionId::V7,
        RoomVersionId::V8,
        RoomVersionId::V9,
        RoomVersionId::V10,
        RoomVersionId::V11,
    ]
});
pub static UNSTABLE_ROOM_VERSIONS: LazyLock<Vec<RoomVersionId>> = LazyLock::new(|| {
    vec![
        RoomVersionId::V2,
        RoomVersionId::V3,
        RoomVersionId::V4,
        RoomVersionId::V5,
    ]
});

fn figment_from_path<P: AsRef<Path>>(path: P) -> Figment {
    let ext = path.as_ref().extension().and_then(|s| s.to_str()).unwrap_or_default();
    match ext {
        "yaml" | "yml" => Figment::new().merge(Yaml::file(path)),
        "json" => Figment::new().merge(Json::file(path)),
        "toml" => Figment::new().merge(Toml::file(path)),
        _ => panic!("Unsupported config file format: {ext}"),
    }
}

fn write_default_config<P: AsRef<Path>>(path: P)  {
    let config = ServerConfig::default();
    let ext = path.as_ref().extension().and_then(|s| s.to_str()).unwrap_or_default();
    let data = match ext {
        "yaml" | "yml" => serde_yaml::to_string(&config).expect("failed to serialize config to YAML"),
        "json" => serde_json::to_string_pretty(&config).expect("failed to serialize config to YAML"),
        "toml" => toml::to_string_pretty(&config).expect("failed to serialize config to YAML"),
        _ => panic!("Unsupported config file format: {ext}"),
    };
    fs::write(path, data).expect("msg: failed to write default config file");
}

pub fn init() {
    let config_file = Env::var("PALPO_CONFIG").unwrap_or("palpo.toml".into());

    let config_path = PathBuf::from(config_file);
    if !config_path.exists() {
        warn!(
            "Config file not found: `{}`, new default file will be created",
            config_path.display()
        );
        write_default_config(&config_path);
    }

    let raw_config = figment_from_path(config_path).merge(Env::prefixed("PALPO_").global());
    let conf = match raw_config.extract::<ServerConfig>() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("It looks like your config is invalid. The following error occurred: {e}");
            std::process::exit(1);
        }
    };

    CONFIG.set(conf).expect("config should be set");
}
pub fn get() -> &'static ServerConfig {
    CONFIG.get().unwrap()
}

pub fn server_user() -> String {
    format!("@palpo:{}", server_name())
}

pub fn space_path() -> &'static str {
    get().space_path.deref()
}

pub fn media_path(server_name: &ServerName, media_id: &str) -> PathBuf {
    let server_name = if server_name == self::server_name().as_str() {
        "_"
    } else {
        server_name.as_str()
    };
    let mut r = PathBuf::new();
    r.push(space_path());
    r.push("media");
    r.push(server_name);
    // let extension = extension.unwrap_or_default();
    // if !extension.is_empty() {
    //     r.push(format!("{media_id}.{extension}"));
    // } else {
    r.push(media_id);
    // }
    r
}

pub fn appservice_registration_dir() -> Option<&'static str> {
    get().appservice_registration_dir.as_deref()
}

/// Returns this server's keypair.
pub fn keypair() -> &'static Ed25519KeyPair {
    static KEYPAIR: OnceLock<Ed25519KeyPair> = OnceLock::new();
    KEYPAIR.get_or_init(|| {
        if let Some(keypair) = &get().keypair {
            let bytes = base64::decode(&keypair.document).expect("server keypair is invalid base64 string");
            Ed25519KeyPair::from_der(&bytes, keypair.version.clone()).expect("invalid server Ed25519KeyPair")
        } else {
            crate::utils::generate_keypair()
        }
    })
}

pub fn enabled_ldap() -> Option<&'static LdapConfig> {
    if let Some(ldap) = get().ldap.as_ref() {
        if ldap.enable { Some(ldap) } else { None }
    } else {
        None
    }
}

pub fn enabled_jwt() -> Option<&'static JwtConfig> {
    if let Some(jwt) = get().jwt.as_ref() {
        if jwt.enable { Some(jwt) } else { None }
    } else {
        None
    }
}

pub fn well_known_client() -> String {
    let config = get();
    if let Some(url) = &config.well_known.client {
        url.to_string()
    } else {
        format!("https://{}", config.server_name)
    }
}

pub fn well_known_server() -> OwnedServerName {
    let config = get();
    match &config.well_known.server {
        Some(server_name) => server_name.to_owned(),
        None => {
            if config.server_name.port().is_some() {
                config.server_name.to_owned()
            } else {
                format!("{}:443", config.server_name.host())
                    .try_into()
                    .expect("Host from valid hostname + :443 must be valid")
            }
        }
    }
}

pub fn valid_cidr_range(ip: &IPAddress) -> bool {
    cidr_range_denylist().iter().all(|cidr| !cidr.includes(ip))
}

pub fn cidr_range_denylist() -> &'static [IPAddress] {
    static CIDR_RANGE_DENYLIST: OnceLock<Vec<IPAddress>> = OnceLock::new();
    CIDR_RANGE_DENYLIST.get_or_init(|| {
        let conf = get();
        conf.ip_range_denylist
            .iter()
            .map(IPAddress::parse)
            .inspect(|cidr| trace!("Denied CIDR range: {cidr:?}"))
            .collect::<Result<_, String>>()
            .expect("Invalid CIDR range in config")
    })
}

pub fn server_name() -> &'static ServerName {
    get().server_name.as_ref()
}
pub fn listen_addr() -> &'static str {
    get().listen_addr.deref()
}

pub fn max_request_size() -> u32 {
    get().max_request_size
}

pub fn max_fetch_prev_events() -> u16 {
    get().max_fetch_prev_events
}

pub fn allow_registration() -> bool {
    get().allow_registration
}

pub fn allow_encryption() -> bool {
    get().allow_encryption
}

pub fn allow_federation() -> bool {
    get().allow_federation
}

pub fn allow_room_creation() -> bool {
    get().allow_room_creation
}

pub fn allow_unstable_room_versions() -> bool {
    get().allow_unstable_room_versions
}

pub fn default_room_version() -> RoomVersionId {
    get().default_room_version.clone()
}

pub fn enable_lightning_bolt() -> bool {
    get().enable_lightning_bolt
}

pub fn allow_check_for_updates() -> bool {
    get().allow_check_for_updates
}

pub fn trusted_servers() -> &'static [OwnedServerName] {
    &get().trusted_servers
}

pub fn jwt_decoding_key() -> Option<&'static jsonwebtoken::DecodingKey> {
    static JWT_DECODING_KEY: OnceLock<Option<jsonwebtoken::DecodingKey>> = OnceLock::new();
    JWT_DECODING_KEY
        .get_or_init(|| {
            get()
                .jwt
                .as_ref()
                .map(|jwt| jsonwebtoken::DecodingKey::from_secret(jwt.secret.as_bytes()))
        })
        .as_ref()
}

pub fn turn_password() -> &'static str {
    &get().turn_password
}

pub fn turn_ttl() -> u64 {
    get().turn_ttl
}

pub fn turn_uris() -> &'static [String] {
    &get().turn_uris
}

pub fn turn_username() -> &'static str {
    &get().turn_username
}

pub fn turn_secret() -> &'static String {
    &get().turn_secret
}

pub fn emergency_password() -> Option<&'static str> {
    get().emergency_password.as_deref()
}

pub fn allow_local_presence() -> bool {
    get().allow_local_presence
}

pub fn allow_incoming_presence() -> bool {
    get().allow_incoming_presence
}

pub fn allow_outcoming_presence() -> bool {
    get().allow_outgoing_presence
}

pub fn presence_idle_timeout_s() -> u64 {
    get().presence_idle_timeout_s
}

pub fn presence_offline_timeout_s() -> u64 {
    get().presence_offline_timeout_s
}

pub fn supported_room_versions() -> Vec<RoomVersionId> {
    let mut room_versions: Vec<RoomVersionId> = vec![];
    room_versions.extend(STABLE_ROOM_VERSIONS.clone());
    if get().allow_unstable_room_versions {
        room_versions.extend(UNSTABLE_ROOM_VERSIONS.clone());
    };
    room_versions
}

pub fn supports_room_version(room_version: &RoomVersionId) -> bool {
    supported_room_versions().contains(room_version)
}
