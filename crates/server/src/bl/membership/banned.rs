use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::once;
use std::sync::Arc;
use std::time::Duration;

use diesel::prelude::*;
use palpo_core::appservice::third_party;
use salvo::conn::SocketAddr;
use salvo::http::StatusError;
use tokio::sync::RwLock;

use crate::core::client::membership::{JoinRoomResBody, ThirdPartySigned};
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent};
use crate::core::events::room::member::{MembershipState, RoomMemberEventContent};
use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::federation::membership::{
    InviteUserResBodyV2, MakeJoinReqArgs, MakeLeaveResBody, SendJoinArgs, SendJoinResBodyV2, SendLeaveReqBody,
    make_leave_request,
};
use crate::core::identifiers::*;
use crate::core::serde::{
    CanonicalJsonObject, CanonicalJsonValue, RawJsonValue, to_canonical_value, to_raw_json_value,
};
use crate::core::{UnixMillis, federation};

use crate::appservice::RegistrationInfo;
use crate::event::{DbEventData, NewDbEvent, PduBuilder, PduEvent, gen_event_id_canonical_json};
use crate::federation::maybe_strip_event_id;
use crate::membership::federation::membership::{
    InviteUserReqArgs, InviteUserReqBodyV2, MakeJoinResBody, RoomStateV1, RoomStateV2, SendJoinReqBody,
    SendLeaveReqArgsV2, send_leave_request_v2,
};
use crate::membership::state::DeltaInfo;
use crate::room::state::{self, CompressedEvent};
use crate::schema::*;
use crate::user::DbUser;
use crate::{AppError, AppResult, GetUrlOrigin, IsRemoteOrLocal, MatrixError, Seqnum, SigningKeys, db, diesel_exists};

/// Checks if the room is banned in any way possible and the sender user is not
/// an admin.
///
/// Performs automatic deactivation if `auto_deactivate_banned_room_attempts` is
/// enabled
#[tracing::instrument]
pub fn banned_room_check(
    user_id: &UserId,
    room_id: Option<&RoomId>,
    server_name: Option<&ServerName>,
    client_addr: &SocketAddr,
) -> AppResult<()> {
    // TODO: NOW
    return Ok(());
}
