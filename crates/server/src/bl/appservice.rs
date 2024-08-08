use std::collections::BTreeMap;

use regex::RegexSet;

use crate::core::appservice::{Namespace, Registration};
use crate::core::identifiers::*;
use crate::AppResult;

/// Compiled regular expressions for a namespace.
#[derive(Clone, Debug)]
pub struct NamespaceRegex {
    pub exclusive: Option<RegexSet>,
    pub non_exclusive: Option<RegexSet>,
}

impl NamespaceRegex {
    /// Checks if this namespace has rights to a namespace
    pub fn is_match(&self, heystack: &str) -> bool {
        if self.is_exclusive_match(heystack) {
            return true;
        }

        if let Some(non_exclusive) = &self.non_exclusive {
            if non_exclusive.is_match(heystack) {
                return true;
            }
        }
        false
    }

    /// Checks if this namespace has exlusive rights to a namespace
    pub fn is_exclusive_match(&self, heystack: &str) -> bool {
        if let Some(exclusive) = &self.exclusive {
            if exclusive.is_match(heystack) {
                return true;
            }
        }
        false
    }
}

impl TryFrom<Vec<Namespace>> for NamespaceRegex {
    fn try_from(value: Vec<Namespace>) -> Result<Self, regex::Error> {
        let mut exclusive = vec![];
        let mut non_exclusive = vec![];

        for namespace in value {
            if namespace.exclusive {
                exclusive.push(namespace.regex);
            } else {
                non_exclusive.push(namespace.regex);
            }
        }

        Ok(NamespaceRegex {
            exclusive: if exclusive.is_empty() {
                None
            } else {
                Some(RegexSet::new(exclusive)?)
            },
            non_exclusive: if non_exclusive.is_empty() {
                None
            } else {
                Some(RegexSet::new(non_exclusive)?)
            },
        })
    }

    type Error = regex::Error;
}

/// Appservice registration combined with its compiled regular expressions.
#[derive(Clone, Debug)]
pub struct RegistrationInfo {
    pub registration: Registration,
    pub users: NamespaceRegex,
    pub aliases: NamespaceRegex,
    pub rooms: NamespaceRegex,
}

impl RegistrationInfo {
    /// Checks if a given user ID matches either the users namespace or the localpart specified in the appservice registration
    pub fn is_user_match(&self, user_id: &UserId) -> bool {
        self.users.is_match(user_id.as_str()) || self.registration.sender_localpart == user_id.localpart()
    }

    /// Checks if a given user ID exclusively matches either the users namespace or the localpart specified in the appservice registration
    pub fn is_exclusive_user_match(&self, user_id: &UserId) -> bool {
        self.users.is_exclusive_match(user_id.as_str()) || self.registration.sender_localpart == user_id.localpart()
    }
}

impl TryFrom<Registration> for RegistrationInfo {
    fn try_from(value: Registration) -> Result<RegistrationInfo, regex::Error> {
        Ok(RegistrationInfo {
            users: value.namespaces.users.clone().try_into()?,
            aliases: value.namespaces.aliases.clone().try_into()?,
            rooms: value.namespaces.rooms.clone().try_into()?,
            registration: value,
        })
    }

    type Error = regex::Error;
}

/// Registers an appservice and returns the ID to the caller
pub fn register_appservice(yaml: Registration) -> AppResult<String> {
    // TODO: fixme
    let id = yaml.id.as_str();
    // self.id_appserviceregistrations
    //     .insert(id.as_bytes(), serde_yaml::to_string(&yaml).unwrap().as_bytes())?;

    Ok(id.to_owned())
}

/// Remove an appservice registration
///
/// # Arguments
///
/// * `service_name` - the name you send to register the service previously
pub fn unregister_appservice(service_name: &str) -> AppResult<()> {
    // TODO: fixme
    // self.id_appserviceregistrations.remove(service_name.as_bytes())?;
    Ok(())
}

pub fn get_registration(id: &str) -> AppResult<Option<Registration>> {
    // TODO: fixme
    Ok(None)
    // self.id_appserviceregistrations
    //     .get(id.as_bytes())?
    //     .map(|bytes| {
    //         serde_yaml::from_slice(&bytes)
    //             .map_err(|_| AppError::public("Invalid registration bytes in id_appserviceregistrations."))
    //     })
    //     .transpose()
}
pub async fn find_from_token(token: &str) -> Option<RegistrationInfo> {
    // TODO: fixme
    panic!("TODO")
}

// Checks if a given user id matches any exclusive appservice regex
pub fn is_exclusive_user_id(user_id: &UserId) -> bool {
    // TODO: fixme
    false
}

// Checks if a given room alias matches any exclusive appservice regex
pub async fn is_exclusive_alias(alias: &RoomAliasId) -> bool {
    // TODO: fixme
    false
}

// Checks if a given room id matches any exclusive appservice regex
pub async fn is_exclusive_room_id(room_id: &RoomId) -> bool {
    // TODO: fixme
    false
}

pub fn all() -> AppResult<BTreeMap<String, RegistrationInfo>> {
    // TODO: fixme
    Ok(BTreeMap::new())
    // self.iter_ids()?
    //     .filter_map(|id| id.ok())
    //     .map(move |id| {
    //         Ok((
    //             id.clone(),
    //             get_registration(&id)?.expect("iter_ids only returns appservices that exist"),
    //         ))
    //     })
    //     .collect()
}
