use std::path::PathBuf;

use serde::Deserialize;
use url::Url;

use crate::core::serde::default_true;
use crate::macros::config_example;

#[config_example(filename = "palpo-example.toml", section = "ldap")]
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LdapConfig {
    /// Whether to enable LDAP login.
    ///
    /// example: "true"
    #[serde(default = "default_true")]
    pub enable: bool,

    /// URI of the LDAP server.
    ///
    /// example: "ldap://ldap.example.com:389"
    pub uri: Option<Url>,

    /// Root of the searches.
    ///
    /// example: "ou=users,dc=example,dc=org"
    #[serde(default)]
    pub base_dn: String,

    /// Bind DN if anonymous search is not enabled.
    ///
    /// You can use the variable `{username}` that will be replaced by the
    /// entered username. In such case, the password used to bind will be the
    /// one provided for the login and not the one given by
    /// `bind_password_file`. Beware: automatically granting admin rights will
    /// not work if you use this direct bind instead of a LDAP search.
    ///
    /// example: "cn=ldap-reader,dc=example,dc=org" or
    /// "cn={username},ou=users,dc=example,dc=org"
    #[serde(default)]
    pub bind_dn: Option<String>,

    /// Path to a file on the system that contains the password for the
    /// `bind_dn`.
    ///
    /// The server must be able to access the file, and it must not be empty.
    #[serde(default)]
    pub bind_password_file: Option<PathBuf>,

    /// Search filter to limit user searches.
    ///
    /// You can use the variable `{username}` that will be replaced by the
    /// entered username for more complex filters.
    ///
    /// example: "(&(objectClass=person)(memberOf=matrix))"
    ///
    /// default: "(objectClass=*)"
    #[serde(default = "default_ldap_search_filter")]
    pub filter: String,

    /// Attribute to use to uniquely identify the user.
    ///
    /// example: "uid" or "cn"
    ///
    /// default: "uid"
    #[serde(default = "default_ldap_uid_attribute")]
    pub uid_attribute: String,

    /// Attribute containing the mail of the user.
    ///
    /// example: "mail"
    ///
    /// default: "mail"
    #[serde(default = "default_ldap_mail_attribute")]
    pub mail_attribute: String,

    /// Attribute containing the distinguished name of the user.
    ///
    /// example: "givenName" or "sn"
    ///
    /// default: "givenName"
    #[serde(default = "default_ldap_name_attribute")]
    pub name_attribute: String,

    /// Root of the searches for admin users.
    ///
    /// Defaults to `base_dn` if empty.
    ///
    /// example: "ou=admins,dc=example,dc=org"
    #[serde(default)]
    pub admin_base_dn: String,

    /// The LDAP search filter to find administrative users for palpo.
    ///
    /// If left blank, administrative state must be configured manually for each
    /// user.
    ///
    /// You can use the variable `{username}` that will be replaced by the
    /// entered username for more complex filters.
    ///
    /// example: "(objectClass=palpoAdmin)" or "(uid={username})"
    #[serde(default)]
    pub admin_filter: String,
}

fn default_ldap_search_filter() -> String {
    "(objectClass=*)".to_owned()
}

fn default_ldap_uid_attribute() -> String {
    String::from("uid")
}

fn default_ldap_mail_attribute() -> String {
    String::from("mail")
}

fn default_ldap_name_attribute() -> String {
    String::from("givenName")
}
