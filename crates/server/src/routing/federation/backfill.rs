use salvo::prelude::*;

use crate::PduEvent;
use crate::core::federation::backfill::{BackfillReqArgs, BackfillResBody};
use crate::core::{UnixMillis, user_id};
use crate::{AuthArgs, JsonResult, MatrixError, json_ok};

pub fn router() -> Router {
    Router::with_path("backfill/{room_id}").get(history)
}

/// #GET /_matrix/federation/v1/backfill/{room_id}
/// Retrieves events from before the sender joined the room, if the room's
/// history visibility allows.
#[endpoint]
async fn history(_aa: AuthArgs, args: BackfillReqArgs) -> JsonResult<BackfillResBody> {
    let server_name = &crate::config().server_name;
    debug!("Got backfill request from: {}", server_name);

    if !crate::room::is_server_in_room(server_name, &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room.").into());
    }

    crate::event::handler::acl_check(server_name, &args.room_id)?;

    let until = args
        .v
        .iter()
        .map(|event_id| crate::room::timeline::get_event_sn(event_id))
        .filter_map(|r| r.ok().flatten())
        .max()
        .ok_or(MatrixError::invalid_param("No known eventid in v"))?;

    let limit = args.limit.min(100);

    let all_events = crate::room::timeline::get_pdus_backward(
        &user_id!("@doesntmatter:palpo.im"),
        &args.room_id,
        until,
        limit,
        None,
    )?;

    let mut events = Vec::with_capacity(all_events.len());
    for (_, pdu) in all_events {
        if crate::room::state::server_can_see_event(server_name, &args.room_id, &pdu.event_id)? {
            if let Some(pdu_json) = crate::room::timeline::get_pdu_json(&pdu.event_id)? {
                events.push(PduEvent::convert_to_outgoing_federation_event(pdu_json));
            }
        }
    }

    json_ok(BackfillResBody {
        origin: server_name.to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdus: events,
    })
}
