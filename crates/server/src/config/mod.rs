use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use figment::Figment;
use figment::providers::{Env, Format, Json, Toml, Yaml};
use ipaddress::IPAddress;

mod server;
pub use server::*;
mod admin;
pub use admin::*;
// mod appservice;
// pub use appservice::*;
mod jwt;
pub use jwt::*;
mod blurhash;
pub use blurhash::*;
mod cache;
pub use cache::*;
mod compression;
pub use compression::*;
mod db;
pub use db::*;
// mod dns;
// pub use dns::*;
mod federation;
pub use federation::*;
// mod ldap;
// pub use ldap::*;
mod logger;
pub use logger::*;
mod media;
pub use media::*;
mod presence;
pub use presence::*;
mod proxy;
pub use proxy::*;
mod read_receipt;
pub use read_receipt::*;
mod turn;
pub use turn::*;
mod typing;
pub use typing::*;
mod url_preview;
pub use url_preview::*;

use crate::AppResult;
use crate::core::identifiers::*;
use crate::core::signatures::Ed25519KeyPair;

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

pub fn init() {
    let config_file = Env::var("PALPO_CONFIG").unwrap_or("palpo.toml".into());

    let config_path = PathBuf::from(config_file);
    if !config_path.exists() {
        panic!(
            "Config file not found: `{}`, new default file will be created",
            config_path.display()
        );
    }

    let raw_conf = figment_from_path(config_path).merge(Env::prefixed("PALPO_").global());
    let conf = match raw_conf.extract::<ServerConfig>() {
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
    format!("@palpo:{}", get().server_name)
}

pub fn space_path() -> &'static str {
    get().space_path.deref()
}

pub fn media_path(server_name: &ServerName, media_id: &str) -> PathBuf {
    let server_name = if server_name == &get().server_name {
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
