use salvo::prelude::*;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct as _};

/// Specifies options for [lazy-loading membership events][lazy-loading] on
/// supported endpoints
///
/// [lazy-loading]: https://spec.matrix.org/latest/client-server-api/#lazy-loading-room-members
#[derive(ToSchema, Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(from = "LazyLoadJsonRepr")]
#[allow(clippy::exhaustive_enums)]
pub enum LazyLoadOptions {
    /// Disables lazy-loading of membership events.
    #[default]
    Disabled,

    /// Enables lazy-loading of events.
    Enabled {
        /// If `true`, sends all membership events for all events, even if they
        /// have already been sent to the client.
        ///
        /// Defaults to `false`.
        include_redundant_members: bool,
    },
}

impl LazyLoadOptions {
    /// Returns `true` is `self` is `Disabled`.
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn is_enabled(&self) -> bool {
        !self.is_disabled()
    }
}

impl Serialize for LazyLoadOptions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state;
        match *self {
            Self::Enabled {
                include_redundant_members: true,
            } => {
                state = serializer.serialize_struct("LazyLoad", 2)?;
                state.serialize_field("lazy_load_members", &true)?;
                state.serialize_field("include_redundant_members", &true)?;
            }
            Self::Enabled { .. } => {
                state = serializer.serialize_struct("LazyLoad", 1)?;
                state.serialize_field("lazy_load_members", &true)?;
            }
            Self::Disabled => {
                state = serializer.serialize_struct("LazyLoad", 0)?;
            }
        }
        state.end()
    }
}

#[derive(Deserialize)]
struct LazyLoadJsonRepr {
    lazy_load_members: Option<bool>,
    include_redundant_members: Option<bool>,
}

impl From<LazyLoadJsonRepr> for LazyLoadOptions {
    fn from(opts: LazyLoadJsonRepr) -> Self {
        if opts.lazy_load_members.unwrap_or(false) {
            Self::Enabled {
                include_redundant_members: opts.include_redundant_members.unwrap_or(false),
            }
        } else {
            Self::Disabled
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value as from_json_value, json, to_value as to_json_value};

    use super::LazyLoadOptions;

    #[test]
    fn serialize_disabled() {
        let lazy_load_options = LazyLoadOptions::Disabled;
        assert_eq!(to_json_value(lazy_load_options).unwrap(), json!({}));
    }

    #[test]
    fn serialize_no_redundant() {
        let lazy_load_options = LazyLoadOptions::Enabled {
            include_redundant_members: false,
        };
        assert_eq!(
            to_json_value(lazy_load_options).unwrap(),
            json!({ "lazy_load_members": true })
        );
    }

    #[test]
    fn serialize_with_redundant() {
        let lazy_load_options = LazyLoadOptions::Enabled {
            include_redundant_members: true,
        };
        assert_eq!(
            to_json_value(lazy_load_options).unwrap(),
            json!({ "lazy_load_members": true, "include_redundant_members": true })
        );
    }

    #[test]
    fn deserialize_no_lazy_load() {
        let json = json!({});
        assert_eq!(
            from_json_value::<LazyLoadOptions>(json).unwrap(),
            LazyLoadOptions::Disabled
        );

        let json = json!({ "include_redundant_members": true });
        assert_eq!(
            from_json_value::<LazyLoadOptions>(json).unwrap(),
            LazyLoadOptions::Disabled
        );
    }
}
