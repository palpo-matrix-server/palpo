/// `POST /_matrix/identity/*/sign-ed25519`
///
/// Sign invitation details.

/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#post_matrixidentityv2sign-ed25519
use crate::{serde::Base64, OwnedUserId, ServerSignatures};

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/sign-ed25519",
//     }
// };

/// Request type for the `sign_invitation_ed25519` endpoint.

#[derive(ToSchema, Deserialize, Debug)]
pub struct SignInvitationReqBody {
    /// The Matrix user ID of the user accepting the invitation.
    pub mxid: OwnedUserId,

    /// The token from the call to store-invite.
    pub token: String,

    /// The private key, encoded as unpadded base64.
    pub private_key: Base64,
}

/// Response type for the `sign_invitation_ed25519` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct SignInvitationResBody {
    /// The Matrix user ID of the user accepting the invitation.
    pub mxid: OwnedUserId,

    /// The Matrix user ID of the user who sent the invitation.
    pub sender: OwnedUserId,

    /// The signature of the mxid, sender and token.
    pub signatures: ServerSignatures,

    /// The token for the invitation.
    pub token: String,
}
impl SignInvitationResBody {
    /// Creates a `Response` with the given Matrix user ID, sender user ID, signatures and
    /// token.
    pub fn new(mxid: OwnedUserId, sender: OwnedUserId, signatures: ServerSignatures, token: String) -> Self {
        Self {
            mxid,
            sender,
            signatures,
            token,
        }
    }
}

/// `POST /_matrix/identity/*/store-invite`
///
/// Store pending invitations to a user's third-party ID.

/// `/v2/` ([spec])
///
/// [spec]: https://spec.matrix.org/latest/identity-service-api/#post_matrixidentityv2store-invite
use crate::{room::RoomType, thirdparty::Medium, OwnedMxcUri, OwnedRoomAliasId, OwnedRoomId, OwnedUserId};
use serde::{ser::SerializeSeq, Deserialize, Serialize};

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/identity/v2/store-invite",
//     }
// };

/// Request type for the `store_invitation` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct StoreInvitationReqBody {
    /// The type of the third party identifier for the invited user.
    ///
    /// Currently, only `Medium::Email` is supported.
    pub medium: Medium,

    /// The email address of the invited user.
    pub address: String,

    /// The Matrix room ID to which the user is invited.
    pub room_id: OwnedRoomId,

    /// The Matrix user ID of the inviting user.
    pub sender: OwnedUserId,

    /// The Matrix room alias for the room to which the user is invited.
    ///
    /// This should be retrieved from the `m.room.canonical` state event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_alias: Option<OwnedRoomAliasId>,

    /// The Content URI for the room to which the user is invited.
    ///
    /// This should be retrieved from the `m.room.avatar` state event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_avatar_url: Option<OwnedMxcUri>,

    /// The `join_rule` for the room to which the user is invited.
    ///
    /// This should be retrieved from the `m.room.join_rules` state event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_join_rules: Option<String>,

    /// The name of the room to which the user is invited.
    ///
    /// This should be retrieved from the `m.room.name` state event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_name: Option<String>,

    /// The type of the room to which the user is invited.
    ///
    /// This should be retrieved from the `m.room.create` state event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_type: Option<RoomType>,

    /// The display name of the user ID initiating the invite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,

    /// The Content URI for the avater of the user ID initiating the invite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_avatar_url: Option<OwnedMxcUri>,
}

/// Response type for the `store_invitation` endpoint.
#[derive(ToSchema,Serialize, Debug)]
pub struct StoreInvitationResBody {
    /// The generated token.
    ///
    /// Must be a string consisting of the characters `[0-9a-zA-Z.=_-]`. Its length must not
    /// exceed 255 characters and it must not be empty.
    pub token: String,

    /// A list of [server's long-term public key, generated ephemeral public key].
    pub public_keys: PublicKeys,

    /// The generated (redacted) display_name.
    ///
    /// An example is `f...@b...`.
    pub display_name: String,
}

impl StoreInvitationResBody {
    /// Creates a new `Response` with the given token, public keys and display name.
    pub fn new(token: String, public_keys: PublicKeys, display_name: String) -> Self {
        Self {
            token,
            public_keys,
            display_name,
        }
    }
}

/// The server's long-term public key and generated ephemeral public key.
#[derive(Debug, Clone)]
#[allow(clippy::exhaustive_structs)]
pub struct PublicKeys {
    /// The server's long-term public key.
    pub server_key: PublicKey,

    /// The generated ephemeral public key.
    pub ephemeral_key: PublicKey,
}

/// A server's long-term or ephemeral public key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PublicKey {
    /// The public key, encoded using [unpadded Base64](https://spec.matrix.org/latest/appendices/#unpadded-base64).
    pub public_key: String,

    /// The URI of an endpoint where the validity of this key can be checked by passing it as a
    /// `public_key` query parameter.
    pub key_validity_url: String,
}

impl PublicKey {
    /// Constructs a new `PublicKey` with the given encoded public key and key validity URL.
    pub fn new(public_key: String, key_validity_url: String) -> Self {
        Self {
            public_key,
            key_validity_url,
        }
    }
}
