use salvo::prelude::*;

use crate::serde::StringEnum;
use crate::PrivOwnedStr;

/// Profile fields to specify in query.
///
/// This type can hold an arbitrary string. To build this with a custom value, convert it from a
/// string with `::from()` / `.into()`. To check for values that are not available as a
/// documented variant here, use its string representation, obtained through
/// [`.as_str()`](Self::as_str()).
#[derive(ToSchema, Clone, PartialEq, Eq, StringEnum)]
#[non_exhaustive]
pub enum ProfileField {
    /// Display name of the user.
    #[palpo_enum(rename = "display_name")]
    DisplayName,

    /// Avatar URL for the user's avatar.
    #[palpo_enum(rename = "avatar_url")]
    AvatarUrl,

    #[doc(hidden)]
    #[salvo(schema(value_type = String))]
    _Custom(PrivOwnedStr),
}
