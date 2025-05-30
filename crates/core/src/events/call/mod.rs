//! Modules for events in the `m.call` namespace.
//!
//! This module also contains types shared by events in its child namespaces.

pub mod answer;
pub mod candidates;
pub mod hangup;
pub mod invite;
pub mod member;
pub mod negotiate;
pub mod notify;
pub mod reject;
pub mod sdp_stream_metadata_changed;
pub mod select_answer;

use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::{PrivOwnedStr, serde::StringEnum};

/// A VoIP session description.
///
/// This is the same type as WebRTC's [`RTCSessionDescriptionInit`].
///
/// [`RTCSessionDescriptionInit`]: (https://www.w3.org/TR/webrtc/#dom-rtcsessiondescriptioninit):
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct SessionDescription {
    /// The type of session description.
    ///
    /// This is the `type` field of `RTCSessionDescriptionInit`.
    #[serde(rename = "type")]
    pub session_type: String,

    /// The SDP text of the session description.
    ///
    /// Defaults to an empty string.
    #[serde(default)]
    pub sdp: String,
}

impl SessionDescription {
    /// Creates a new `SessionDescription` with the given session type and SDP
    /// text.
    pub fn new(session_type: String, sdp: String) -> Self {
        Self { session_type, sdp }
    }
}
/// Metadata about a VoIP stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamMetadata {
    /// The purpose of the stream.
    pub purpose: StreamPurpose,

    /// Whether the audio track of the stream is muted.
    ///
    /// Defaults to `false`.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub audio_muted: bool,

    /// Whether the video track of the stream is muted.
    ///
    /// Defaults to `false`.
    #[serde(default, skip_serializing_if = "crate::serde::is_default")]
    pub video_muted: bool,
}

impl StreamMetadata {
    /// Creates a new `StreamMetadata` with the given purpose.
    pub fn new(purpose: StreamPurpose) -> Self {
        Self {
            purpose,
            audio_muted: false,
            video_muted: false,
        }
    }
}

/// The purpose of a VoIP stream.
#[doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/doc/string_enum.md"))]
#[derive(Clone, PartialEq, Eq, StringEnum)]
#[palpo_enum(rename_all = "m.lowercase")]
#[non_exhaustive]
pub enum StreamPurpose {
    /// `m.usermedia`.
    ///
    /// A stream that contains the webcam and/or microphone tracks.
    UserMedia,

    /// `m.screenshare`.
    ///
    /// A stream with the screen-sharing tracks.
    ScreenShare,

    #[doc(hidden)]
    _Custom(PrivOwnedStr),
}

/// The capabilities of a client in a VoIP call.
#[cfg(feature = "unstable-msc2747")]
#[derive(ToSchema, Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallCapabilities {
    /// Whether this client supports [DTMF].
    ///0
    /// Defaults to `false`.
    ///
    /// [DTMF]: https://w3c.github.io/webrtc-pc/#peer-to-peer-dtmf
    #[serde(rename = "m.call.dtmf", default)]
    pub dtmf: bool,
}

#[cfg(feature = "unstable-msc2747")]
impl CallCapabilities {
    /// Creates a default `CallCapabilities`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this `CallCapabilities` only contains default values.
    pub fn is_default(&self) -> bool {
        !self.dtmf
    }
}
