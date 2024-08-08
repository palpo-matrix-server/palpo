//! Errors that can be sent from the homeserver.

use std::{collections::BTreeMap, fmt, time::Duration};

use serde_json::Value as JsonValue;

use crate::{PrivOwnedStr, RoomVersionId};

/// An enum for the error kind.
///
/// Items may contain additional information.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// M_FORBIDDEN
    Forbidden,

    /// M_UNKNOWN_TOKEN
    UnknownToken {
        /// If this is `true`, the client can acquire a new access token by specifying the device
        /// ID it is already using to the login API.
        ///
        /// For more information, see [the spec].
        ///
        /// [the spec]: https://spec.matrix.org/latest/client-server-api/#soft-logout
        soft_logout: bool,
    },

    /// M_MISSING_TOKEN
    MissingToken,

    /// M_BAD_JSON
    BadJson,

    /// M_NOT_JSON
    NotJson,

    /// M_NOT_FOUND
    NotFound,

    /// M_LIMIT_EXCEEDED
    LimitExceeded {
        /// How long a client should wait in milliseconds before they can try again.
        retry_after_ms: Option<Duration>,
    },

    /// M_UNKNOWN
    Unknown,

    /// M_UNRECOGNIZED
    Unrecognized,

    /// M_UNAUTHORIZED
    Unauthorized,

    /// M_USER_DEACTIVATED
    UserDeactivated,

    /// M_USER_IN_USE
    UserInUse,

    /// M_INVALID_USERNAME
    InvalidUsername,

    /// M_ROOM_IN_USE
    RoomInUse,

    /// M_INVALID_ROOM_STATE
    InvalidRoomState,

    /// M_THREEPID_IN_USE
    ThreepidInUse,

    /// M_THREEPID_NOT_FOUND
    ThreepidNotFound,

    /// M_THREEPID_AUTH_FAILED
    ThreepidAuthFailed,

    /// M_THREEPID_DENIED
    ThreepidDenied,

    /// M_SERVER_NOT_TRUSTED
    ServerNotTrusted,

    /// M_UNSUPPORTED_ROOM_VERSION
    UnsupportedRoomVersion,

    /// M_INCOMPATIBLE_ROOM_VERSION
    IncompatibleRoomVersion {
        /// The room's version.
        room_version: RoomVersionId,
    },

    /// M_BAD_STATE
    BadState,

    /// M_GUEST_ACCESS_FORBIDDEN
    GuestAccessForbidden,

    /// M_CAPTCHA_NEEDED
    CaptchaNeeded,

    /// M_CAPTCHA_INVALID
    CaptchaInvalid,

    /// M_MISSING_PARAM
    MissingParam,

    /// M_INVALID_PARAM
    InvalidParam,

    /// M_TOO_LARGE
    TooLarge,

    /// M_EXCLUSIVE
    Exclusive,

    /// M_RESOURCE_LIMIT_EXCEEDED
    ResourceLimitExceeded {
        /// A URI giving a contact method for the server administrator.
        admin_contact: String,
    },

    /// M_CANNOT_LEAVE_SERVER_NOTICE_ROOM
    CannotLeaveServerNoticeRoom,

    /// M_WEAK_PASSWORD
    WeakPassword,

    /// M_UNABLE_TO_AUTHORISE_JOIN
    UnableToAuthorizeJoin,

    /// M_UNABLE_TO_GRANT_JOIN
    UnableToGrantJoin,

    /// M_BAD_ALIAS
    BadAlias,

    /// M_DUPLICATE_ANNOTATION
    DuplicateAnnotation,

    /// M_NOT_YET_UPLOADED
    NotYetUploaded,

    /// M_CANNOT_OVERWRITE_MEDIA
    CannotOverwriteMedia,

    /// M_UNKNOWN_POS for sliding sync
    UnknownPos,

    /// M_URL_NOT_SET
    UrlNotSet,

    /// M_BAD_STATUS
    BadStatus,

    /// M_CONNECTION_FAILED
    ConnectionFailed,

    /// M_CONNECTION_TIMEOUT
    ConnectionTimeout,

    /// M_WRONG_ROOM_KEYS_VERSION
    WrongRoomKeysVersion {
        /// The currently active backup version.
        current_version: Option<String>,
    },

    #[doc(hidden)]
    _Custom {
        errcode: PrivOwnedStr,
        extra: BTreeMap<String, JsonValue>,
    },
}

impl AsRef<str> for ErrorKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::Forbidden => "M_FORBIDDEN",
            Self::UnknownToken { .. } => "M_UNKNOWN_TOKEN",
            Self::MissingToken => "M_MISSING_TOKEN",
            Self::BadJson => "M_BAD_JSON",
            Self::NotJson => "M_NOT_JSON",
            Self::NotFound => "M_NOT_FOUND",
            Self::LimitExceeded { .. } => "M_LIMIT_EXCEEDED",
            Self::Unknown => "M_UNKNOWN",
            Self::Unrecognized => "M_UNRECOGNIZED",
            Self::Unauthorized => "M_UNAUTHORIZED",
            Self::UserDeactivated => "M_USER_DEACTIVATED",
            Self::UserInUse => "M_USER_IN_USE",
            Self::InvalidUsername => "M_INVALID_USERNAME",
            Self::RoomInUse => "M_ROOM_IN_USE",
            Self::InvalidRoomState => "M_INVALID_ROOM_STATE",
            Self::ThreepidInUse => "M_THREEPID_IN_USE",
            Self::ThreepidNotFound => "M_THREEPID_NOT_FOUND",
            Self::ThreepidAuthFailed => "M_THREEPID_AUTH_FAILED",
            Self::ThreepidDenied => "M_THREEPID_DENIED",
            Self::ServerNotTrusted => "M_SERVER_NOT_TRUSTED",
            Self::UnsupportedRoomVersion => "M_UNSUPPORTED_ROOM_VERSION",
            Self::IncompatibleRoomVersion { .. } => "M_INCOMPATIBLE_ROOM_VERSION",
            Self::BadState => "M_BAD_STATE",
            Self::GuestAccessForbidden => "M_GUEST_ACCESS_FORBIDDEN",
            Self::CaptchaNeeded => "M_CAPTCHA_NEEDED",
            Self::CaptchaInvalid => "M_CAPTCHA_INVALID",
            Self::MissingParam => "M_MISSING_PARAM",
            Self::InvalidParam => "M_INVALID_PARAM",
            Self::TooLarge => "M_TOO_LARGE",
            Self::Exclusive => "M_EXCLUSIVE",
            Self::ResourceLimitExceeded { .. } => "M_RESOURCE_LIMIT_EXCEEDED",
            Self::CannotLeaveServerNoticeRoom => "M_CANNOT_LEAVE_SERVER_NOTICE_ROOM",
            Self::WeakPassword => "M_WEAK_PASSWORD",
            Self::UnableToAuthorizeJoin => "M_UNABLE_TO_AUTHORISE_JOIN",
            Self::UnableToGrantJoin => "M_UNABLE_TO_GRANT_JOIN",
            Self::BadAlias => "M_BAD_ALIAS",
            Self::DuplicateAnnotation => "M_DUPLICATE_ANNOTATION",
            Self::NotYetUploaded => "M_NOT_YET_UPLOADED",
            Self::CannotOverwriteMedia => "M_CANNOT_OVERWRITE_MEDIA",
            Self::UnknownPos => "M_UNKNOWN_POS",
            Self::UrlNotSet => "M_URL_NOT_SET",
            Self::BadStatus { .. } => "M_BAD_STATUS",
            Self::ConnectionFailed => "M_CONNECTION_FAILED",
            Self::ConnectionTimeout => "M_CONNECTION_TIMEOUT",
            Self::WrongRoomKeysVersion { .. } => "M_WRONG_ROOM_KEYS_VERSION",
            Self::_Custom { errcode, .. } => &errcode.0,
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}
