//! Errors that can be sent from the homeserver.

use std::error::Error as StdError;
use std::iter::FromIterator;
use std::{fmt, time::Duration};

use salvo::http::{Response, StatusCode, header};
use salvo::writing::Scribe;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

mod auth;
pub use auth::*;
mod kind;
/// Deserialize and Serialize implementations for ErrorKind.
/// Separate module because it's a lot of code.
mod kind_serde;
pub use kind::*;

use crate::RoomVersionId;

macro_rules! simple_kind_fns {
    ($($fname:ident, $kind:ident;)+) => {
        $(
            /// Create a new `MatrixError`.
            pub fn $fname(body: impl Into<ErrorBody>) -> Self {
                Self::new(ErrorKind::$kind, body)
            }
        )+
    }
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ErrorBody(JsonMap<String, JsonValue>);

impl From<String> for ErrorBody {
    fn from(message: String) -> Self {
        Self(JsonMap::from_iter(vec![("error".to_owned(), json!(message))]))
    }
}
impl From<&str> for ErrorBody {
    fn from(message: &str) -> Self {
        Self(JsonMap::from_iter(vec![("error".to_owned(), json!(message))]))
    }
}
impl From<JsonMap<String, JsonValue>> for ErrorBody {
    fn from(inner: JsonMap<String, JsonValue>) -> Self {
        Self(inner)
    }
}

/// A Matrix Error
#[derive(Debug, Clone)]
#[allow(clippy::exhaustive_structs)]
pub struct MatrixError {
    /// The http status code.
    pub status_code: Option<http::StatusCode>,

    /// The `WWW-Authenticate` header error message.
    pub authenticate: Option<AuthenticateError>,
    pub kind: ErrorKind,

    /// The http response's body.
    pub body: ErrorBody,
}
impl MatrixError {
    pub fn new(kind: ErrorKind, body: impl Into<ErrorBody>) -> Self {
        Self {
            status_code: None,
            authenticate: None,
            kind,
            body: body.into(),
        }
    }
    simple_kind_fns! {
        forbidden, Forbidden;
        missing_token, MissingToken;
        bad_json, BadJson;
        not_json, NotJson;
        not_found, NotFound;
        unknown, Unknown;
        unrecognized, Unrecognized;
        unauthorized, Unauthorized;
        user_deactivated, UserDeactivated;
        user_in_use, UserInUse;
        invalid_username, InvalidUsername;
        room_in_use, RoomInUse;
        invalid_room_state, InvalidRoomState;
        threepid_in_use, ThreepidInUse;
        threepid_not_found, ThreepidNotFound;
        threepid_auth_failed, ThreepidAuthFailed;
        threepid_denied, ThreepidDenied;
        server_not_trusted, ServerNotTrusted;
        unsupported_room_version, UnsupportedRoomVersion;
        bad_state, BadState;
        guest_access_forbidden, GuestAccessForbidden;
        captcha_needed, CaptchaNeeded;
        captcha_invalid, CaptchaInvalid;
        missing_param, MissingParam;
        invalid_param, InvalidParam;
        too_large, TooLarge;
        exclusive, Exclusive;
        cannot_leave_server_notice_room, CannotLeaveServerNoticeRoom;
        weak_password, WeakPassword;
        unable_to_authorize_join, UnableToAuthorizeJoin;
        unable_to_grant_join, UnableToGrantJoin;
        bad_alias, BadAlias;
        duplicate_annotation, DuplicateAnnotation;
        not_yet_uploaded, NotYetUploaded;
        cannot_overwrite_media, CannotOverwriteMedia;
        unknown_pos, UnknownPos;
        url_not_set, UrlNotSet;
        bad_status, BadStatus;
        connection_failed, ConnectionFailed;
        connection_timeout, ConnectionTimeout;
    }
    pub fn unknown_token(soft_logout: bool, body: impl Into<ErrorBody>) -> Self {
        Self::new(ErrorKind::UnknownToken { soft_logout }, body)
    }
    pub fn limit_exceeded(retry_after_ms: Option<Duration>, body: impl Into<ErrorBody>) -> Self {
        Self::new(ErrorKind::LimitExceeded { retry_after_ms }, body)
    }
    pub fn incompatible_room_version(room_version: RoomVersionId, body: impl Into<ErrorBody>) -> Self {
        Self::new(ErrorKind::IncompatibleRoomVersion { room_version }, body)
    }
    pub fn resource_limit_exceeded(admin_contact: String, body: impl Into<ErrorBody>) -> Self {
        Self::new(ErrorKind::ResourceLimitExceeded { admin_contact }, body)
    }
    pub fn wrong_room_keys_version(current_version: Option<String>, body: impl Into<ErrorBody>) -> Self {
        Self::new(ErrorKind::WrongRoomKeysVersion { current_version }, body)
    }
}
impl Serialize for MatrixError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.body.serialize(serializer)
    }
}

impl fmt::Display for MatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.status_code.unwrap_or(StatusCode::BAD_REQUEST).as_u16();
        write!(f, "[{code} / {}]", self.kind)
    }
}

impl StdError for MatrixError {}

impl Scribe for MatrixError {
    fn render(self, res: &mut Response) {
        res.add_header(header::CONTENT_TYPE, "application/json", true).ok();

        if res.status_code.map(|c| c.is_success()).unwrap_or(true) {
            let code = self.status_code.unwrap_or_else(|| {
                use ErrorKind::*;
                match self.kind.clone() {
                    Forbidden | GuestAccessForbidden | ThreepidAuthFailed | ThreepidDenied => StatusCode::FORBIDDEN,
                    Unauthorized | UnknownToken { .. } | MissingToken => StatusCode::UNAUTHORIZED,
                    NotFound | Unrecognized => StatusCode::NOT_FOUND,
                    LimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
                    UserDeactivated => StatusCode::FORBIDDEN,
                    TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                    CannotOverwriteMedia => StatusCode::CONFLICT,
                    NotYetUploaded => StatusCode::GATEWAY_TIMEOUT,
                    _ => StatusCode::BAD_REQUEST,
                }
            });
            res.status_code(code);
        }

        if let Some(auth_error) = &self.authenticate {
            res.add_header(header::WWW_AUTHENTICATE, auth_error, true).ok();
        };

        let Self { kind, mut body, .. } = self;
        body.0.insert("errcode".to_owned(), kind.to_string().into());

        let bytes: Vec<u8> = crate::serde::json_to_buf(&body.0).unwrap();
        res.write_body(bytes).ok();
    }
}

/// An error that happens when Palpo cannot understand a Matrix version.
#[derive(Debug)]
pub struct UnknownVersionError;

impl fmt::Display for UnknownVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "version string was unknown")
    }
}

impl StdError for UnknownVersionError {}

// #[cfg(test)]
// mod tests {
//     use assert_matches2::assert_matches;
//     use serde_json::{from_value as from_json_value, json};

//     use super::{ErrorKind, StandardErrorBody};

//     #[test]
//     fn deserialize_forbidden() {
//         let deserialized: StandardErrorBody = from_json_value(json!({
//             "errcode": "M_FORBIDDEN",
//             "error": "You are not authorized to ban users in this room.",
//         }))
//         .unwrap();

//         assert_eq!(deserialized.kind, ErrorKind::Forbidden);
//         assert_eq!(
//             deserialized.message,
//             "You are not authorized to ban users in this room."
//         );
//     }

//     #[test]
//     fn deserialize_wrong_room_key_version() {
//         let deserialized: StandardErrorBody = from_json_value(json!({
//             "current_version": "42",
//             "errcode": "M_WRONG_ROOM_KEYS_VERSION",
//             "error": "Wrong backup version."
//         }))
//         .expect("We should be able to deserialize a wrong room keys version error");

//         assert_matches!(deserialized.kind, ErrorKind::WrongRoomKeysVersion { current_version });
//         assert_eq!(current_version.as_deref(), Some("42"));
//         assert_eq!(deserialized.message, "Wrong backup version.");
//     }

//     #[test]
//     fn custom_authenticate_error_sanity() {
//         use super::AuthenticateError;

//         let s = "Bearer error=\"custom_error\", misc=\"some content\"";

//         let error = AuthenticateError::from_str(s).unwrap();
//         let error_header = http::HeaderValue::try_from(&error).unwrap();

//         assert_eq!(error_header.to_str().unwrap(), s);
//     }

//     #[test]
//     fn serialize_insufficient_scope() {
//         use super::AuthenticateError;

//         let error = AuthenticateError::InsufficientScope {
//             scope: "something_privileged".to_owned(),
//         };
//         let error_header = http::HeaderValue::try_from(&error).unwrap();

//         assert_eq!(
//             error_header.to_str().unwrap(),
//             "Bearer error=\"insufficient_scope\", scope=\"something_privileged\""
//         );
//     }

//     #[test]
//     fn deserialize_insufficient_scope() {
//         use super::{AuthenticateError, Error, ErrorBody};
//         use crate::api::EndpointError;

//         let response = http::Response::builder()
//             .header(
//                 http::header::WWW_AUTHENTICATE,
//                 "Bearer error=\"insufficient_scope\", scope=\"something_privileged\"",
//             )
//             .status(http::StatusCode::UNAUTHORIZED)
//             .body(
//                 serde_json::to_string(&json!({
//                     "errcode": "M_FORBIDDEN",
//                     "error": "Insufficient privilege",
//                 }))
//                 .unwrap(),
//             )
//             .unwrap();
//         let error = Error::from_http_response(response);

//         assert_eq!(error.status_code, http::StatusCode::UNAUTHORIZED);
//         assert_matches!(error.body, ErrorBody::Standard { kind, message });
//         assert_eq!(kind, ErrorKind::Forbidden);
//         assert_eq!(message, "Insufficient privilege");
//         assert_matches!(error.authenticate, Some(AuthenticateError::InsufficientScope { scope }));
//         assert_eq!(scope, "something_privileged");
//     }
// }
