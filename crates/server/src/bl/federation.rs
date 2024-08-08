use std::fmt::Debug;
use std::net::SocketAddr;

/// Wraps either an literal IP address plus port, or a hostname plus complement
/// (colon-plus-port if it was specified).
///
/// Note: A `FedDest::Named` might contain an IP address in string form if there
/// was no port specified to construct a SocketAddr with.
///
/// # Examples:
/// ```rust
/// # use palpo::api::server_server::FedDest;
/// # fn main() -> Result<(), std::net::AddrParseError> {
/// FedDest::Literal("198.51.100.3:8448".parse()?);
/// FedDest::Literal("[2001:db8::4:5]:443".parse()?);
/// FedDest::Named("matrix.example.org".to_owned(), "".to_owned());
/// FedDest::Named("matrix.example.org".to_owned(), ":8448".to_owned());
/// FedDest::Named("198.51.100.5".to_owned(), "".to_owned());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FedDest {
    Literal(SocketAddr),
    Named(String, String),
}

impl FedDest {
    fn into_https_string(self) -> String {
        match self {
            Self::Literal(addr) => format!("https://{addr}"),
            Self::Named(host, port) => format!("https://{host}{port}"),
        }
    }

    fn into_uri_string(self) -> String {
        match self {
            Self::Literal(addr) => addr.to_string(),
            Self::Named(host, ref port) => host + port,
        }
    }

    fn hostname(&self) -> String {
        match &self {
            Self::Literal(addr) => addr.ip().to_string(),
            Self::Named(host, _) => host.clone(),
        }
    }

    fn port(&self) -> Option<u16> {
        match &self {
            Self::Literal(addr) => Some(addr.port()),
            Self::Named(_, port) => port[1..].parse().ok(),
        }
    }
}
