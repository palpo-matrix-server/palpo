use diesel::prelude::*;
use serde_json::json;

use crate::core::client::room::IncludeThreads;
use crate::core::events::relation::BundledThread;
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonValue;
use crate::schema::*;
use crate::{AppResult, MatrixError, PduEvent, db};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = threads, primary_key(event_id))]
pub struct DbThread {
    pub event_id: OwnedEventId,
    pub event_sn: i64,
    pub room_id: OwnedRoomId,
    pub last_id: OwnedEventId,
    pub last_sn: i64,
}

pub fn get_threads(
    room_id: &RoomId,
    include: &IncludeThreads,
    limit: i64,
    from_token: Option<i64>,
) -> AppResult<(Vec<(OwnedEventId, PduEvent)>, Option<i64>)> {
    let items = if let Some(from_token) = from_token {
        threads::table
            .filter(threads::room_id.eq(room_id))
            .filter(threads::event_sn.le(from_token))
            .select((threads::event_id, threads::event_sn))
            .order_by(threads::last_sn.desc())
            .limit(limit)
            .load::<(OwnedEventId, i64)>(&mut *db::connect()?)?
    } else {
        threads::table
            .filter(threads::room_id.eq(room_id))
            .select((threads::event_id, threads::event_sn))
            .order_by(threads::last_sn.desc())
            .limit(limit)
            .load::<(OwnedEventId, i64)>(&mut *db::connect()?)?
    };
    let next_token = items.last().map(|(_, sn)| *sn - 1);

    let mut events = Vec::with_capacity(items.len());
    for (event_id, _) in items {
        if let Some(pdu) = crate::room::timeline::get_pdu(&event_id)? {
            events.push((event_id, pdu));
        }
    }
    Ok((events, next_token))
}

pub fn add_to_thread(thread_id: &EventId, pdu: &PduEvent) -> AppResult<()> {
    let root_pdu = crate::room::timeline::get_pdu(thread_id)?
        .ok_or_else(|| MatrixError::invalid_param("Thread root pdu not found."))?;

    let mut root_pdu_json = crate::room::timeline::get_pdu_json(thread_id)?
        .ok_or_else(|| MatrixError::invalid_param("Thread root pdu not found"))?;

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

        crate::room::timeline::replace_pdu(thread_id, &root_pdu_json)?;
    }

    diesel::insert_into(threads::table)
        .values(DbThread {
            event_id: root_pdu.event_id.as_ref().to_owned(),
            event_sn: root_pdu.event_sn.clone(),
            room_id: root_pdu.room_id.clone(),
            last_id: pdu.event_id.as_ref().to_owned(),
            last_sn: pdu.event_sn,
        })
        .on_conflict(threads::event_id)
        .do_update()
        .set((
            threads::last_id.eq(pdu.event_id.as_ref()),
            threads::last_sn.eq(pdu.event_sn),
        ))
        .execute(&mut *db::connect()?)?;
    Ok(())
}
