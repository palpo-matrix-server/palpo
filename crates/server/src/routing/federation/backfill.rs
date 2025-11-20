use salvo::prelude::*;

use crate::core::federation::backfill::{BackfillReqArgs, BackfillResBody};
use crate::core::{UnixMillis, user_id};
use crate::room::{state, timeline};
use crate::{AuthArgs, DepotExt, JsonResult, MatrixError, config, json_ok};

pub fn router() -> Router {
    Router::with_path("backfill/{room_id}").get(get_history)
}

/// #GET /_matrix/federation/v1/backfill/{room_id}
/// Retrieves events from before the sender joined the room, if the room's
/// history visibility allows.
#[endpoint]
async fn get_history(
    _aa: AuthArgs,
    args: BackfillReqArgs,
    depot: &mut Depot,
) -> JsonResult<BackfillResBody> {
    let origin = depot.origin()?;
    debug!("got backfill request from: {}", origin);

    let until = args
        .v
        .iter()
        .filter_map(|event_id| crate::event::get_batch_token(event_id).ok())
        .max_by(|a, b| a.event_sn.cmp(&b.event_sn))
        .ok_or(MatrixError::invalid_param(
            "unknown event id in query string v",
        ))?;

    let limit = args.limit.min(100);

    println!(
        "==================================== start bbbackfill get_history  begin until: {until}  limit: {limit}  args: {:#?}",
        args
    );
    let all_events = timeline::get_pdus_backward(
        None,
        &args.room_id,
        until,
        None,
        None,
        limit,
        crate::room::EventOrderBy::TopologicalOrdering,
    )?;

    println!(
        "bbbbbbbbbbbbackfill get_history  all_events len {:?}  util: {until}  {args:#?}",
        all_events.len()
    );
    let mut events = Vec::with_capacity(all_events.len());
    println!(
        "=======================================events {:#?}",
        events
    );
    for (_, pdu) in all_events {
        if state::server_can_see_event(origin, &args.room_id, &pdu.event_id)?
            && let Some(pdu_json) = timeline::get_pdu_json(&pdu.event_id)?
        {
            events.push(crate::sending::convert_to_outgoing_federation_event(
                pdu_json,
            ));
        } else {
            println!(
                "bbbbbbbbbbbbackfill get_history  skipping event {:?}",
                pdu.event_id
            );
        }
    }
    println!(
        "=======================================end get history {}",
        events.len()
    );
    events.reverse();

    json_ok(BackfillResBody {
        origin: config::get().server_name.to_owned(),
        origin_server_ts: UnixMillis::now(),
        pdus: events,
    })
}
