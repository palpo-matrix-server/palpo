use salvo::prelude::*;

use crate::core::UnixMillis;
use crate::core::federation::backfill::{BackfillReqArgs, BackfillResBody};
use crate::room::{state, timeline};
use crate::{AuthArgs, DepotExt, JsonResult, MatrixError, config, json_ok};

pub fn router() -> Router {
    Router::with_path("backfill/{room_id}").get(get_backfill)
}

/// #GET /_matrix/federation/v1/backfill/{room_id}
/// Retrieves events from before the sender joined the room, if the room's
/// history visibility allows.
#[endpoint]
async fn get_backfill(
    _aa: AuthArgs,
    args: BackfillReqArgs,
    depot: &mut Depot,
) -> JsonResult<BackfillResBody> {
    let origin = depot.origin()?;
    debug!("got backfill request from: {}", origin);

    // TODO: WRONG implementation
    let until_tk = args
        .v
        .iter()
        .filter_map(|event_id| crate::event::get_historic_token(event_id).ok())
        .max_by(|a, b| a.stream_ordering().cmp(&b.stream_ordering()))
        .ok_or(MatrixError::invalid_param(
            "unknown event id in query string v",
        ))?;
    println!("=====gggggggggget_history backfill until_tk: {:?}  args: {args:?}", until_tk);

    let limit = args.limit.min(100);

    let all_events = timeline::topolo::load_pdus_backward(
        None,
        &args.room_id,
        Some(until_tk),
        None,
        None,
        limit,
    )?;

    let mut events = Vec::with_capacity(all_events.len());
    for (_, pdu) in all_events {
        if state::server_can_see_event(origin, &args.room_id, &pdu.event_id)?
            && let Some(pdu_json) = timeline::get_pdu_json(&pdu.event_id)?
        {
            events.push(crate::sending::convert_to_outgoing_federation_event(
                pdu_json,
            ));
        }
    }
    events.reverse();

    json_ok(BackfillResBody {
        origin: config::get().server_name.to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdus: events,
    })
}
