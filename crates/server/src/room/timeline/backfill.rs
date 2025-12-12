use diesel::dsl::count;
use diesel::prelude::*;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::core::Seqnum;
use crate::core::events::TimelineEventType;
use crate::core::federation::backfill::{BackfillReqArgs, BackfillResBody, backfill_request};
use crate::core::identifiers::*;
use crate::core::serde::RawJsonValue;
use crate::data::connect;
use crate::data::schema::*;
use crate::event::BatchToken;
use crate::event::{handler, parse_fetched_pdu};
use crate::{AppError, AppResult, GetUrlOrigin, SnPduEvent, room};

#[tracing::instrument(skip_all)]
pub async fn backfill_if_required(
    room_id: &RoomId,
    from_tk: &BatchToken,
    pdus: &IndexMap<Seqnum, SnPduEvent>,
    limit: usize,
) -> AppResult<Vec<SnPduEvent>> {
    let mut depths = pdus
        .values()
        .map(|p| (p.event_id.clone(), p.depth as i64))
        .collect::<Vec<_>>();
    if let Some(topological_ordering) = from_tk.topological_ordering() {
        if let Ok(event_id) = events::table
            .filter(events::room_id.eq(room_id))
            .filter(events::topological_ordering.eq(topological_ordering))
            .select(events::id)
            .first::<OwnedEventId>(&mut connect()?)
        {
            depths.push((event_id, topological_ordering.abs()));
        }
    }
    depths.sort_unstable_by(|a, b| b.1.cmp(&a.1));

    let (prev_event, prev_depth) = if let Some(depth) = depths.first() {
        depth
    } else {
        return Ok(vec![]);
    };

    let mut prev_depth = *prev_depth;
    let mut prev_event = prev_event;
    let last_depth = depths.last().map(|&(_, d)| d).unwrap_or_default() as i64;
    if prev_depth == last_depth {
        return Ok(vec![]);
    }

    let depths = events::table
        .filter(events::depth.lt(prev_depth))
        .filter(events::depth.ge(last_depth))
        .order(events::depth.desc())
        .select((events::id, events::depth))
        .load::<(OwnedEventId, i64)>(&mut connect()?)?;

    let mut found_big_gap = false;
    let mut number_of_gaps = 0;
    let mut fill_from = None;
    for &(ref event_id, depth) in depths.iter() {
        let delta = prev_depth - depth;
        if delta > 1 {
            number_of_gaps += 1;
            if fill_from.is_none() {
                fill_from = Some(prev_event);
            }
        }
        if delta >= 2 {
            found_big_gap = true;
            if fill_from.is_none() {
                fill_from = Some(prev_event);
            }
            break;
        }
        prev_depth = depth;
        prev_event = event_id;
    }

    if number_of_gaps < 3 && !found_big_gap {
        return Ok(vec![]);
    };
    let Some(fill_from) = fill_from else {
        return Ok(vec![]);
    };

    let admin_servers = room::admin_servers(room_id, false)?;

    let room_version = room::get_version(room_id)?;
    for backfill_server in &admin_servers {
        info!("asking {backfill_server} for backfill");
        let request = backfill_request(
            &backfill_server.origin().await,
            BackfillReqArgs {
                room_id: room_id.to_owned(),
                v: vec![fill_from.to_owned()],
                limit,
            },
        )?
        .into_inner();
        match crate::sending::send_federation_request(backfill_server, request, None)
            .await?
            .json::<BackfillResBody>()
            .await
        {
            Ok(response) => {
                let mut events = Vec::new();
                for pdu in response.pdus {
                    match backfill_pdu(backfill_server, room_id, &room_version, pdu).await {
                        Ok(pdu) => {
                            events.push(pdu);
                        }
                        Err(e) => {
                            warn!("failed to add backfilled pdu: {e}");
                        }
                    }
                }
                return Ok(events);
            }
            Err(e) => {
                warn!("{backfill_server} could not provide backfill: {e}");
            }
        }
    }

    info!("no servers could backfill");
    Ok(vec![])
}

#[tracing::instrument(skip(pdu))]
pub async fn backfill_pdu(
    origin: &ServerName,
    room_id: &RoomId,
    room_version: &RoomVersionId,
    pdu: Box<RawJsonValue>,
) -> AppResult<SnPduEvent> {
    let (event_id, value) = parse_fetched_pdu(room_id, room_version, &pdu)?;

    // Skip the PDU if we already have it as a timeline event
    if let Ok(pdu) = super::get_pdu(&event_id) {
        info!("we already know {event_id}, skipping backfill");
        return Ok(pdu);
    }
    handler::process_incoming_pdu(origin, &event_id, room_id, room_version, value, true, true)
        .await?;

    let _value = super::get_pdu_json(&event_id)?.expect("we just created it");
    let pdu = super::get_pdu(&event_id)?;

    if pdu.event_ty == TimelineEventType::RoomMessage {
        #[derive(Deserialize)]
        struct ExtractBody {
            body: Option<String>,
        }

        let _content = pdu
            .get_content::<ExtractBody>()
            .map_err(|_| AppError::internal("invalid content in pdu."))?;
    }

    Ok(pdu)
}
