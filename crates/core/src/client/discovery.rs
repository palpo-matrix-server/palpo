//! `GET /_matrix/client/*/capabilities`
//!
//! Get information about the server's supported feature set and other relevant capabilities
//! ([spec]).
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#capabilities-negotiation

use std::borrow::Cow;
use std::collections::{BTreeMap, btree_map};

use maplit::btreemap;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, from_value as from_json_value, to_value as to_json_value};

use crate::{MatrixVersion, PrivOwnedStr, RoomVersionId, serde::StringEnum};

/// Contains information about all the capabilities that the server supports.
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Capability to indicate if the user can change their password.
    #[serde(rename = "m.change_password", default)]
    pub change_password: ChangePasswordCapability,

    /// The room versions the server supports.
    #[serde(
        rename = "m.room_versions",
        default,
        skip_serializing_if = "RoomVersionsCapability::is_default"
    )]
    pub room_versions: RoomVersionsCapability,

    /// Capability to indicate if the user can change their display name.
    #[serde(
        rename = "m.set_display_name",
        default,
        skip_serializing_if = "SetDisplayNameCapability::is_default"
    )]
    pub set_display_name: SetDisplayNameCapability,

    /// Capability to indicate if the user can change their avatar.
    #[serde(
        rename = "m.set_avatar_url",
        default,
        skip_serializing_if = "SetAvatarUrlCapability::is_default"
    )]
    pub set_avatar_url: SetAvatarUrlCapability,

    /// Capability to indicate if the user can change the third-party identifiers associated with
    /// their account.
    #[serde(
        rename = "m.3pid_changes",
        default,
        skip_serializing_if = "ThirdPartyIdChangesCapability::is_default"
    )]
    pub thirdparty_id_changes: ThirdPartyIdChangesCapability,

    /// Any other custom capabilities that the server supports outside of the specification,
    /// labeled using the Java package naming convention and stored as arbitrary JSON values.
    #[serde(flatten)]
    #[salvo(schema(skip))]
    #[salvo(schema(value_type = Object, additional_properties = true))]
    pub custom_capabilities: BTreeMap<String, JsonValue>,
}

impl Capabilities {
    /// Creates empty `Capabilities`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns the value of the given capability.
    ///
    /// Prefer to use the public fields of `Capabilities` where possible; this method is meant to be
    /// used for unsupported capabilities only.
    pub fn get(&self, capability: &str) -> Option<Cow<'_, JsonValue>> {
        fn serialize<T: Serialize>(cap: &T) -> JsonValue {
            to_json_value(cap).expect("capability serialization to succeed")
        }

        match capability {
            "m.change_password" => Some(Cow::Owned(serialize(&self.change_password))),
            "m.room_versions" => Some(Cow::Owned(serialize(&self.room_versions))),
            "m.set_display_name" => Some(Cow::Owned(serialize(&self.set_display_name))),
            "m.set_avatar_url" => Some(Cow::Owned(serialize(&self.set_avatar_url))),
            "m.3pid_changes" => Some(Cow::Owned(serialize(&self.thirdparty_id_changes))),
            _ => self.custom_capabilities.get(capability).map(Cow::Borrowed),
        }
    }

    /// Sets a capability to the given value.
    ///
    /// Prefer to use the public fields of `Capabilities` where possible; this method is meant to be
    /// used for unsupported capabilities only and does not allow setting arbitrary data for
    /// supported ones.
    pub fn set(&mut self, capability: &str, value: JsonValue) -> serde_json::Result<()> {
        match capability {
            "m.change_password" => self.change_password = from_json_value(value)?,
            "m.room_versions" => self.room_versions = from_json_value(value)?,
            "m.set_display_name" => self.set_display_name = from_json_value(value)?,
            "m.set_avatar_url" => self.set_avatar_url = from_json_value(value)?,
            "m.3pid_changes" => self.thirdparty_id_changes = from_json_value(value)?,
            _ => {
                self.custom_capabilities.insert(capability.to_owned(), value);
            }
        }

        Ok(())
    }

    /// Returns an iterator over the capabilities.
    pub fn iter(&self) -> CapabilitiesIter<'_> {
        CapabilitiesIter::new(self)
    }
}

impl<'a> IntoIterator for &'a Capabilities {
    type Item = CapabilityRef<'a>;
    type IntoIter = CapabilitiesIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Information about the m.change_password capability
#[derive(ToSchema, Clone, Debug, Serialize, Deserialize)]
pub struct ChangePasswordCapability {
    /// `true` if the user can change their password, `false` otherwise.
    pub enabled: bool,
}

impl ChangePasswordCapability {
    /// Creates a new `ChangePasswordCapability` with the given enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Returns whether all fields have their default value.
    pub fn is_default(&self) -> bool {
        self.enabled
    }
}

impl Default for ChangePasswordCapability {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Information about the m.room_versions capability
#[derive(ToSchema, Clone, Debug, Serialize, Deserialize)]
pub struct RoomVersionsCapability {
    /// The default room version the server is using for new rooms.
    pub default: RoomVersionId,

    /// A detailed description of the room versions the server supports.
    pub available: BTreeMap<RoomVersionId, RoomVersionStability>,
}

impl RoomVersionsCapability {
    /// Creates a new `RoomVersionsCapability` with the given default room version ID and room
    /// version descriptions.
    pub fn new(default: RoomVersionId, available: BTreeMap<RoomVersionId, RoomVersionStability>) -> Self {
        Self { default, available }
    }

    /// Returns whether all fields have their default value.
    pub fn is_default(&self) -> bool {
        self.default == RoomVersionId::V1
            && self.available.len() == 1
            && self
                .available
                .get(&RoomVersionId::V1)
                .map(|stability| *stability == RoomVersionStability::Stable)
                .unwrap_or(false)
    }
}

impl Default for RoomVersionsCapability {
    fn default() -> Self {
        Self {
            default: RoomVersionId::V1,
            available: btreemap! { RoomVersionId::V1 => RoomVersionStability::Stable },
        }
    }
}

/// The stability of a room version.
#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(ToSchema, Clone, PartialEq, Eq, StringEnum)]
#[palpo_enum(rename_all = "lowercase")]
#[non_exhaustive]
pub enum RoomVersionStability {
    /// Support for the given version is stable.
    Stable,

    /// Support for the given version is unstable.
    Unstable,

    #[doc(hidden)]
    #[salvo(schema(skip))]
    _Custom(PrivOwnedStr),
}

/// Information about the `m.set_display_name` capability
#[derive(ToSchema, Clone, Debug, Serialize, Deserialize)]
pub struct SetDisplayNameCapability {
    /// `true` if the user can change their display name, `false` otherwise.
    pub enabled: bool,
}

impl SetDisplayNameCapability {
    /// Creates a new `SetDisplayNameCapability` with the given enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Returns whether all fields have their default value.
    pub fn is_default(&self) -> bool {
        self.enabled
    }
}

impl Default for SetDisplayNameCapability {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Information about the `m.set_avatar_url` capability
#[derive(ToSchema, Clone, Debug, Serialize, Deserialize)]
pub struct SetAvatarUrlCapability {
    /// `true` if the user can change their avatar, `false` otherwise.
    pub enabled: bool,
}

impl SetAvatarUrlCapability {
    /// Creates a new `SetAvatarUrlCapability` with the given enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Returns whether all fields have their default value.
    pub fn is_default(&self) -> bool {
        self.enabled
    }
}

impl Default for SetAvatarUrlCapability {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Information about the `m.3pid_changes` capability
#[derive(ToSchema, Clone, Debug, Serialize, Deserialize)]
pub struct ThirdPartyIdChangesCapability {
    /// `true` if the user can change the third-party identifiers associated with their account,
    /// `false` otherwise.
    pub enabled: bool,
}

impl ThirdPartyIdChangesCapability {
    /// Creates a new `ThirdPartyIdChangesCapability` with the given enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Returns whether all fields have their default value.
    pub fn is_default(&self) -> bool {
        self.enabled
    }
}

impl Default for ThirdPartyIdChangesCapability {
    fn default() -> Self {
        Self { enabled: true }
    }
}
/// Iterator implementation for `Capabilities`

/// Reference to a capability.
#[derive(Debug)]
pub struct CapabilityRef<'a> {
    name: &'a str,
    value: Option<&'a JsonValue>,
    caps: &'a Capabilities,
}

impl<'a> CapabilityRef<'a> {
    /// Get name of the capability.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Get value of the capability.
    pub fn value(&self) -> Cow<'a, JsonValue> {
        match self.value {
            // unknown capability from btreemap iterator
            Some(val) => Cow::Borrowed(val),
            // O(1) lookup of known capability
            None => self.caps.get(self.name).unwrap(),
        }
    }
}

/// An iterator over capabilities.
#[derive(Debug)]
pub struct CapabilitiesIter<'a> {
    /// Reference to Capabilities
    caps: &'a Capabilities,
    /// Current position of the iterator
    pos: usize,
    /// Iterator for custom capabilities
    custom_caps_iterator: btree_map::Iter<'a, String, JsonValue>,
}

impl<'a> CapabilitiesIter<'a> {
    /// Creates a new CapabilitiesIter
    pub(super) fn new(caps: &'a Capabilities) -> Self {
        Self {
            caps,
            pos: 0,
            custom_caps_iterator: caps.custom_capabilities.iter(),
        }
    }
}

impl<'a> Iterator for CapabilitiesIter<'a> {
    type Item = CapabilityRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.pos {
            0 => {
                self.pos += 1;
                Some(CapabilityRef {
                    name: "m.change_password",
                    value: None,
                    caps: self.caps,
                })
            }
            1 => {
                self.pos += 1;
                Some(CapabilityRef {
                    name: "m.room_versions",
                    value: None,
                    caps: self.caps,
                })
            }
            2 => {
                self.pos += 1;
                Some(CapabilityRef {
                    name: "m.set_display_name",
                    value: None,
                    caps: self.caps,
                })
            }
            3 => {
                self.pos += 1;
                Some(CapabilityRef {
                    name: "m.set_avatar_url",
                    value: None,
                    caps: self.caps,
                })
            }
            4 => {
                self.pos += 1;
                Some(CapabilityRef {
                    name: "m.3pid_changes",
                    value: None,
                    caps: self.caps,
                })
            }
            _ => self.custom_caps_iterator.next().map(|(name, value)| CapabilityRef {
                name,
                value: Some(value),
                caps: self.caps,
            }),
        }
    }
}

// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: false,
//     authentication: None,
//     history: {
//         1.0 => "/.well-known/matrix/client",
//     }
// };

/// Response type for the `client_well_known` endpoint.

#[derive(ToSchema, Serialize, Debug)]
pub struct ClientWellKnownResBody {
    /// Information about the homeserver to connect to.
    #[serde(rename = "m.homeserver")]
    pub homeserver: HomeServerInfo,

    /// Information about the identity server to connect to.
    #[serde(default, rename = "m.identity_server", skip_serializing_if = "Option::is_none")]
    pub identity_server: Option<IdentityServerInfo>,

    /// Information about the tile server to use to display location data.
    #[serde(
        default,
        rename = "org.matrix.msc3488.tile_server",
        alias = "m.tile_server",
        skip_serializing_if = "Option::is_none"
    )]
    pub tile_server: Option<TileServerInfo>,

    /// Information about the authentication server to connect to when using OpenID Connect.
    #[serde(
        default,
        rename = "org.matrix.msc2965.authentication",
        alias = "m.authentication",
        skip_serializing_if = "Option::is_none"
    )]
    pub authentication: Option<AuthenticationServerInfo>,

    /// Information about the homeserver's trusted proxy to use for sliding sync development.
    #[serde(
        default,
        rename = "org.matrix.msc3575.proxy",
        skip_serializing_if = "Option::is_none"
    )]
    pub sliding_sync_proxy: Option<SlidingSyncProxyInfo>,
}

impl ClientWellKnownResBody {
    /// Creates a new `Response` with the given `HomeServerInfo`.
    pub fn new(homeserver: HomeServerInfo) -> Self {
        Self {
            homeserver,
            identity_server: None,
            tile_server: None,
            authentication: None,
            sliding_sync_proxy: None,
        }
    }
}

/// Information about a discovered homeserver.
#[derive(ToSchema, Clone, Debug, Deserialize, Hash, Serialize)]
pub struct HomeServerInfo {
    /// The base URL for the homeserver for client-server connections.
    pub base_url: String,
}

impl HomeServerInfo {
    /// Creates a new `HomeServerInfo` with the given `base_url`.
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

/// Information about a discovered identity server.
#[derive(ToSchema, Clone, Debug, Deserialize, Hash, Serialize)]
pub struct IdentityServerInfo {
    /// The base URL for the identity server for client-server connections.
    pub base_url: String,
}

impl IdentityServerInfo {
    /// Creates an `IdentityServerInfo` with the given `base_url`.
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

/// Information about a discovered map tile server.
#[derive(ToSchema, Clone, Debug, Deserialize, Hash, Serialize)]
pub struct TileServerInfo {
    /// The URL of a map tile server's `style.json` file.
    ///
    /// See the [Mapbox Style Specification](https://docs.mapbox.com/mapbox-gl-js/style-spec/) for more details.
    pub map_style_url: String,
}

impl TileServerInfo {
    /// Creates a `TileServerInfo` with the given map style URL.
    pub fn new(map_style_url: String) -> Self {
        Self { map_style_url }
    }
}

/// Information about a discovered authentication server.
#[derive(ToSchema, Clone, Debug, Deserialize, Hash, Serialize)]
pub struct AuthenticationServerInfo {
    /// The OIDC Provider that is trusted by the homeserver.
    pub issuer: String,

    /// The URL where the user is able to access the account management
    /// capabilities of the OIDC Provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl AuthenticationServerInfo {
    /// Creates an `AuthenticationServerInfo` with the given `issuer` and an optional `account`.
    pub fn new(issuer: String, account: Option<String>) -> Self {
        Self { issuer, account }
    }
}

/// Information about a discovered sliding sync proxy.
#[derive(ToSchema, Clone, Debug, Deserialize, Hash, Serialize)]
pub struct SlidingSyncProxyInfo {
    /// The URL of a sliding sync proxy that is trusted by the homeserver.
    pub url: String,
}

impl SlidingSyncProxyInfo {
    /// Creates a `SlidingSyncProxyInfo` with the given proxy URL.
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

/// Response type for the `api_versions` endpoint.

#[derive(ToSchema, Serialize, Debug)]
pub struct VersionsResBody {
    /// A list of Matrix client API protocol versions supported by the homeserver.
    pub versions: Vec<String>,

    /// Experimental features supported by the server.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unstable_features: BTreeMap<String, bool>,
}

impl VersionsResBody {
    /// Creates a new `Response` with the given `versions`.
    pub fn new(versions: Vec<String>) -> Self {
        Self {
            versions,
            unstable_features: BTreeMap::new(),
        }
    }

    /// Extracts known Matrix versions from this response.
    ///
    /// Matrix versions that Palpo cannot parse, or does not know about, are discarded.
    ///
    /// The versions returned will be sorted from oldest to latest. Use [`.find()`][Iterator::find]
    /// or [`.rfind()`][DoubleEndedIterator::rfind] to look for a minimum or maximum version to use
    /// given some constraint.
    pub fn known_versions(&self) -> impl Iterator<Item = MatrixVersion> + DoubleEndedIterator {
        self.versions
            .iter()
            // Parse, discard unknown versions
            .flat_map(|s| s.parse::<MatrixVersion>())
            // Map to key-value pairs where the key is the major-minor representation
            // (which can be used as a BTreeMap unlike MatrixVersion itself)
            .map(|v| (v.into_parts(), v))
            // Collect to BTreeMap
            .collect::<BTreeMap<_, _>>()
            // Return an iterator over just the values (`MatrixVersion`s)
            .into_values()
    }
}
/// `/v3/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3capabilities
// const METADATA: Metadata = metadata! {
//     method: GET,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/capabilities",
//         1.1 => "/_matrix/client/v3/capabilities",
//     }
// };

/// Response type for the `get_capabilities` endpoint.

#[derive(ToSchema, Serialize, Debug)]
pub struct CapabilitiesResBody {
    /// The capabilities the server supports
    pub capabilities: Capabilities,
}

impl CapabilitiesResBody {
    /// Creates a new `Response` with the given capabilities.
    pub fn new(capabilities: Capabilities) -> Self {
        Self { capabilities }
    }
}

// #[cfg(test)]
// mod tests {
//     use std::borrow::Cow;

//     use assert_matches2::assert_matches;
//     use serde_json::json;

//     use crate::MatrixVersion;
//     use super::Capabilities;

//     #[test]
//     fn capabilities_iter() -> serde_json::Result<()> {
//         let mut caps = Capabilities::new();
//         let custom_cap = json!({
//             "key": "value",
//         });
//         caps.set("m.some_random_capability", custom_cap)?;
//         let mut caps_iter = caps.iter();

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.change_password");
//         assert_eq!(iter_res.value(), Cow::Borrowed(&json!({ "enabled": true })));

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.room_versions");
//         assert_eq!(
//             iter_res.value(),
//             Cow::Borrowed(&json!({ "available": { "1": "stable" },"default" :"1" }))
//         );

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.set_display_name");
//         assert_eq!(iter_res.value(), Cow::Borrowed(&json!({ "enabled": true })));

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.set_avatar_url");
//         assert_eq!(iter_res.value(), Cow::Borrowed(&json!({ "enabled": true })));

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.3pid_changes");
//         assert_eq!(iter_res.value(), Cow::Borrowed(&json!({ "enabled": true })));

//         let iter_res = caps_iter.next().unwrap();
//         assert_eq!(iter_res.name(), "m.some_random_capability");
//         assert_eq!(iter_res.value(), Cow::Borrowed(&json!({ "key": "value" })));

//         assert_matches!(caps_iter.next(), None);
//         Ok(())
//     }

//     #[test]
//     fn known_versions() {
//         let none = Response::new(vec![]);
//         assert_eq!(none.known_versions().next(), None);

//         let single_known = Response::new(vec!["r0.6.0".to_owned()]);
//         assert_eq!(
//             single_known.known_versions().collect::<Vec<_>>(),
//             vec![MatrixVersion::V1_0]
//         );

//         let single_unknown = Response::new(vec!["v0.0".to_owned()]);
//         assert_eq!(single_unknown.known_versions().next(), None);
//     }

//     #[test]
//     fn known_versions_order() {
//         let sorted = Response::new(vec![
//             "r0.0.1".to_owned(),
//             "r0.5.0".to_owned(),
//             "r0.6.0".to_owned(),
//             "r0.6.1".to_owned(),
//             "v1.1".to_owned(),
//             "v1.2".to_owned(),
//         ]);
//         assert_eq!(
//             sorted.known_versions().collect::<Vec<_>>(),
//             vec![MatrixVersion::V1_0, MatrixVersion::V1_1, MatrixVersion::V1_2],
//         );

//         let sorted_reverse = Response::new(vec![
//             "v1.2".to_owned(),
//             "v1.1".to_owned(),
//             "r0.6.1".to_owned(),
//             "r0.6.0".to_owned(),
//             "r0.5.0".to_owned(),
//             "r0.0.1".to_owned(),
//         ]);
//         assert_eq!(
//             sorted_reverse.known_versions().collect::<Vec<_>>(),
//             vec![MatrixVersion::V1_0, MatrixVersion::V1_1, MatrixVersion::V1_2],
//         );

//         let random_order = Response::new(vec![
//             "v1.1".to_owned(),
//             "r0.6.1".to_owned(),
//             "r0.5.0".to_owned(),
//             "r0.6.0".to_owned(),
//             "r0.0.1".to_owned(),
//             "v1.2".to_owned(),
//         ]);
//         assert_eq!(
//             random_order.known_versions().collect::<Vec<_>>(),
//             vec![MatrixVersion::V1_0, MatrixVersion::V1_1, MatrixVersion::V1_2],
//         );
//     }
// }
