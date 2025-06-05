use diesel::prelude::*;
use serde_json::json;

use crate::core::client::room::IncludeThreads;
use crate::core::events::relation::BundledThread;
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonValue;
use crate::data::connect;
use crate::data::room::DbThread;
use crate::data::schema::*;
use crate::room::{state, timeline};
use crate::{AppResult, MatrixError, SnPduEvent, PduEvent};

pub fn get_threads(
    room_id: &RoomId,
    include: &IncludeThreads,
    limit: i64,
    from_token: Option<i64>,
) -> AppResult<(Vec<(OwnedEventId, SnPduEvent)>, Option<i64>)> {
    let items = if let Some(from_token) = from_token {
        threads::table
            .filter(threads::room_id.eq(room_id))
            .filter(threads::event_sn.le(from_token))
            .select((threads::event_id, threads::event_sn))
            .order_by(threads::last_sn.desc())
            .limit(limit)
            .load::<(OwnedEventId, i64)>(&mut connect()?)?
    } else {
        threads::table
            .filter(threads::room_id.eq(room_id))
            .select((threads::event_id, threads::event_sn))
            .order_by(threads::last_sn.desc())
            .limit(limit)
            .load::<(OwnedEventId, i64)>(&mut connect()?)?
    };
    let next_token = items.last().map(|(_, sn)| *sn - 1);

    let mut events = Vec::with_capacity(items.len());
    for (event_id, _) in items {
        if let Ok(pdu) = timeline::get_sn_pdu(&event_id) {
            events.push((event_id, pdu));
        }
    }
    Ok((events, next_token))
}

pub fn add_to_thread(thread_id: &EventId, pdu: &SnPduEvent) -> AppResult<()> {
    let root_pdu = timeline::get_sn_pdu(thread_id)?;

    let mut root_pdu_json =
        timeline::get_pdu_json(thread_id)?.ok_or_else(|| MatrixError::invalid_param("Thread root pdu not found"))?;

    if let CanonicalJsonValue::Object(unsigned) = root_pdu_json
        .entry("unsigned".to_owned())
        .or_insert_with(|| CanonicalJsonValue::Object(Default::default()))
    {
        if let Some(mut relations) = unsigned
            .get("m.relations")
            .and_then(|r| r.as_object())
            .and_then(|r| r.get("m.thread"))
            .and_then(|relations| serde_json::from_value::<BundledThread>(relations.clone().into()).ok())
        {
            // Thread already existed
            relations.count += 1;
            relations.latest_event = pdu.to_message_like_event();

            let content = serde_json::to_value(relations).expect("to_value always works");

            unsigned.insert(
                "m.relations".to_owned(),
                json!({ "m.thread": content }).try_into().expect("thread is valid json"),
            );
        } else {
            // New thread
            let relations = BundledThread {
                latest_event: pdu.to_message_like_event(),
                count: 1,
                current_user_participated: true,
            };

            let content = serde_json::to_value(relations).expect("to_value always works");

            unsigned.insert(
                "m.relations".to_owned(),
                json!({ "m.thread": content }).try_into().expect("thread is valid json"),
            );
        }

        timeline::replace_pdu(thread_id, &root_pdu_json)?;
    }

    diesel::insert_into(threads::table)
        .values(DbThread {
            event_id: root_pdu.event_id.clone(),
            event_sn: root_pdu.event_sn.clone(),
            room_id: root_pdu.room_id.clone(),
            last_id: pdu.event_id.clone(),
            last_sn: pdu.event_sn,
        })
        .on_conflict(threads::event_id)
        .do_update()
        .set((threads::last_id.eq(&pdu.event_id), threads::last_sn.eq(pdu.event_sn)))
        .execute(&mut connect()?)?;
    Ok(())
}
