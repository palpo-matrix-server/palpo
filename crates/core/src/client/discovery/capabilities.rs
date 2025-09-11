//! `GET /_matrix/client/*/capabilities`
//!
//! Get information about the server's supported feature set and other relevant capabilities
//! ([spec]).
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#capabilities-negotiation

use std::{
    borrow::Cow,
    collections::{BTreeMap, btree_map},
};

use maplit::btreemap;
use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, from_value as from_json_value, to_value as to_json_value};

use crate::{PrivOwnedStr, RoomVersionId, serde::StringEnum};

// /// `/v3/` ([spec])
// ///
// /// [spec]: https://spec.matrix.org/latest/client-server-api/#get_matrixclientv3capabilities
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

    /// Capability to indicate if the user can change the third-party
    /// identifiers associated with their account.
    #[serde(
        rename = "m.3pid_changes",
        default,
        skip_serializing_if = "ThirdPartyIdChangesCapability::is_default"
    )]
    pub thirdparty_id_changes: ThirdPartyIdChangesCapability,

    /// Any other custom capabilities that the server supports outside of the
    /// specification, labeled using the Java package naming convention and
    /// stored as arbitrary JSON values.
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
    /// Prefer to use the public fields of `Capabilities` where possible; this
    /// method is meant to be used for unsupported capabilities only.
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
    /// Prefer to use the public fields of `Capabilities` where possible; this
    /// method is meant to be used for unsupported capabilities only and
    /// does not allow setting arbitrary data for supported ones.
    pub fn set(&mut self, capability: &str, value: JsonValue) -> serde_json::Result<()> {
        match capability {
            "m.change_password" => self.change_password = from_json_value(value)?,
            "m.room_versions" => self.room_versions = from_json_value(value)?,
            "m.set_display_name" => self.set_display_name = from_json_value(value)?,
            "m.set_avatar_url" => self.set_avatar_url = from_json_value(value)?,
            "m.3pid_changes" => self.thirdparty_id_changes = from_json_value(value)?,
            _ => {
                self.custom_capabilities
                    .insert(capability.to_owned(), value);
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
    /// Creates a new `RoomVersionsCapability` with the given default room
    /// version ID and room version descriptions.
    pub fn new(
        default: RoomVersionId,
        available: BTreeMap<RoomVersionId, RoomVersionStability>,
    ) -> Self {
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
    /// `true` if the user can change the third-party identifiers associated
    /// with their account, `false` otherwise.
    pub enabled: bool,
}

impl ThirdPartyIdChangesCapability {
    /// Creates a new `ThirdPartyIdChangesCapability` with the given enabled
    /// flag.
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
// /// Iterator implementation for `Capabilities`

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
            _ => self
                .custom_caps_iterator
                .next()
                .map(|(name, value)| CapabilityRef {
                    name,
                    value: Some(value),
                    caps: self.caps,
                }),
        }
    }
}
