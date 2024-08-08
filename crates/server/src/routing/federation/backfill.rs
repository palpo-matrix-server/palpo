use salvo::prelude::*;

use crate::core::client::uiaa::AuthData;
use crate::core::federation::backfill::BackfillReqArgs;
use crate::core::federation::backfill::BackfillResBody;
use crate::core::{user_id, UnixMillis};
use crate::PduEvent;
use crate::{db, empty_ok, hoops, json_ok, AppError, AuthArgs, AuthedInfo, DepotExt, JsonResult, MatrixError};

pub fn router() -> Router {
    Router::with_path("backfill/<room_id>").get(history)
}

// #GET /_matrix/federation/v1/backfill/<room_id>
/// Retrieves events from before the sender joined the room, if the room's
/// history visibility allows.
#[endpoint]
async fn history(_aa: AuthArgs, args: BackfillReqArgs, depot: &mut Depot) -> JsonResult<BackfillResBody> {
    let authed = depot.authed_info()?;
    debug!("Got backfill request from: {}", authed.server_name());

    if !crate::room::is_server_in_room(authed.server_name(), &args.room_id)? {
        return Err(MatrixError::forbidden("Server is not in room.").into());
    }

    crate::event::handler::acl_check(authed.server_name(), &args.room_id)?;

    let until = args
        .v
        .iter()
        .map(|event_id| crate::room::timeline::get_event_sn(event_id))
        .filter_map(|r| r.ok().flatten())
        .max()
        .ok_or(MatrixError::invalid_param("No known eventid in v"))?;

    let limit = args.limit.min(100);

    let all_events =
        crate::room::timeline::get_pdus_backward(&user_id!("@doesntmatter:palpo.im"), &args.room_id, until, limit)?;

    let mut events = Vec::with_capacity(all_events.len());
    for (_, pdu) in all_events {
        if crate::room::state::server_can_see_event(authed.server_name(), &args.room_id, &pdu.event_id)? {
            if let Some(pdu_json) = crate::room::timeline::get_pdu_json(&pdu.event_id)? {
                events.push(PduEvent::convert_to_outgoing_federation_event(pdu_json));
            }
        }
    }

    json_ok(BackfillResBody {
        origin: crate::config().server_name.to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdus: events,
    })
}
