use std::path::PathBuf;

use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UrlPreviewConfig {
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
    pub bound_interface: Option<Either<IpAddr, String>>,

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
    pub domain_contains_allowlist: Vec<String>,

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
    pub domain_explicit_allowlist: Vec<String>,

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
    pub domain_explicit_denylist: Vec<String>,

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
    pub url_contains_allowlist: Vec<String>,

    /// Maximum amount of bytes allowed in a URL preview body size when
    /// spidering. Defaults to 256KB in bytes.
    ///
    /// default: 256000
    #[serde(default = "default_url_preview_max_spider_size")]
    pub max_spider_size: usize,

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
    pub check_root_domain: bool,
}

fn default_url_preview_max_spider_size() -> usize {
    256_000 // 256KB
}
