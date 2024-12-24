use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use diesel::prelude::*;
use futures_util::{stream::FuturesUnordered, StreamExt};
use palpo_core::federation::event::EventResBody;
use tokio::sync::{RwLock, RwLockWriteGuard, Semaphore};

use crate::core::directory::QueryCriteria;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::server_acl::RoomServerAclEventContent;
use crate::core::events::StateEventType;
use crate::core::federation::directory::{
    remote_server_keys_batch_request, remote_server_keys_request, RemoteServerKeysBatchReqBody,
    RemoteServerKeysBatchResBody, RemoteServerKeysReqArgs, ServerKeysResBody,
};
use crate::core::federation::event::{
    get_events_request, room_state_ids_request, RoomStateAtEventReqArgs, RoomStateIdsResBody,
};
use crate::core::federation::key::get_server_key_request;
use crate::core::federation::membership::{SendJoinResBodyV1, SendJoinResBodyV2};
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, CanonicalJsonValue, RawJsonValue};
use crate::core::state::{self, RoomVersion, StateMap};
use crate::core::{OwnedServerName, ServerName, UnixMillis};
use crate::event::{NewDbEvent, PduEvent};
use crate::room::state::DeltaInfo;
use crate::room::state::{CompressedState, DbRoomStateField, FrameInfo};
use crate::{db, exts::*, schema::*, AppError, AppResult, MatrixError, SigningKeys};

/// Call /state_ids to find out what the state at this pdu is. We trust the
/// server's response to some extend (sic), but we still do a lot of checks
/// on the events
pub(super) async fn fetch_state(
    origin: &ServerName,
    create_event: &PduEvent,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
    event_id: &EventId,
) -> AppResult<Option<HashMap<i64, Arc<EventId>>>> {
    debug!("Calling /state_ids");
    // Call /state_ids to find out what the state at this pdu is. We trust the server's
    // response to some extend, but we still do a lot of checks on the events
    let request = room_state_ids_request(
        &origin.origin().await,
        RoomStateAtEventReqArgs {
            room_id: room_id.to_owned(),
            event_id: event_id.to_owned(),
        },
    )?
    .into_inner();
    let res = crate::sending::send_federation_request(origin, request)
        .await?
        .json::<RoomStateIdsResBody>()
        .await?;
    debug!("Fetching state events at event.");
    let state_vec = super::fetch_and_handle_outliers(
        origin,
        &res.pdu_ids.iter().map(|x| Arc::from(&**x)).collect::<Vec<_>>(),
        create_event,
        room_id,
        room_version_id,
    )
    .await?;

    let mut state: HashMap<_, Arc<EventId>> = HashMap::new();
    for (pdu, _) in state_vec {
        let state_key = pdu
            .state_key
            .clone()
            .ok_or_else(|| AppError::internal("Found non-state pdu in state events."))?;

        let state_key_id = crate::room::state::ensure_field_id(&pdu.event_ty.to_string().into(), &state_key)?;

        match state.entry(state_key_id) {
            hash_map::Entry::Vacant(v) => {
                v.insert(Arc::from(&*pdu.event_id));
            }
            hash_map::Entry::Occupied(_) => {
                return Err(AppError::internal(
                    "State event's type and state_key combination exists multiple times.",
                ))
            }
        }
    }

    // The original create event must still be in the state
    let create_state_key_id = crate::room::state::ensure_field_id(&StateEventType::RoomCreate, "")?;

    if state.get(&create_state_key_id).map(|id| id.as_ref()) != Some(&create_event.event_id) {
        return Err(AppError::internal("Incoming event refers to wrong create event."));
    }

    Ok(Some(state))
}
