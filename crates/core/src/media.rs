/// Endpoints for the media repository.
use salvo::oapi::ToSchema;

use crate::PrivOwnedStr;
use crate::serde::StringEnum;

/// The desired resizing method.
#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(ToSchema, StringEnum, Clone)]
#[palpo_enum(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Method {
    /// Crop the original to produce the requested image dimensions.
    Crop,

    /// Maintain the original aspect ratio of the source image.
    Scale,

    #[doc(hidden)]
    #[salvo(schema(value_type = String))]
    _Custom(PrivOwnedStr),
}

// #[cfg(test)]
// mod tests {
//     use crate::RawJsonValue;
//     use assert_matches2::assert_matches;
//     use serde_json::{from_value as from_json_value, json, value::to_raw_value as to_raw_json_value};

//     // Since BTreeMap<String, Box<RawJsonValue>> deserialization doesn't seem to
//     // work, test that Option<RawJsonValue> works
//     #[test]
//     fn raw_json_deserialize() {
//         type OptRawJson = Option<Box<RawJsonValue>>;

//         assert_matches!(from_json_value::<OptRawJson>(json!(null)).unwrap(), None);
//         from_json_value::<OptRawJson>(json!("test")).unwrap().unwrap();
//         from_json_value::<OptRawJson>(json!({ "a": "b" })).unwrap().unwrap();
//     }

//     // For completeness sake, make sure serialization works too
//     #[test]
//     fn raw_json_serialize() {
//         to_raw_json_value(&json!(null)).unwrap();
//         to_raw_json_value(&json!("string")).unwrap();
//         to_raw_json_value(&json!({})).unwrap();
//         to_raw_json_value(&json!({ "a": "b" })).unwrap();
//     }
// }
