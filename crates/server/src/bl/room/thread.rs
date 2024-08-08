use std::str::FromStr;

use diesel::prelude::*;
use serde_json::json;

use crate::core::client::room::IncludeThreads;
use crate::core::events::relation::BundledThread;
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonValue;
use crate::schema::*;
use crate::{db, utils, AppError, AppResult, MatrixError, PduEvent};

#[derive(Insertable, Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = room_threads)]
pub struct RoomThread {
    pub id: OwnedEventId,
    pub room_id: OwnedRoomId,
    pub latest_event_id: OwnedEventId,
    pub topological_ordering: i64,
    pub stream_ordering: i64,
}

#[derive(Clone, Debug)]
pub struct ThreadsNextBatch {
    topological_ordering: i64,
    stream_ordering: i64,
}
impl ToString for ThreadsNextBatch {
    fn to_string(&self) -> String {
        format!("{}-{}", self.topological_ordering, self.stream_ordering)
    }
}
impl FromStr for ThreadsNextBatch {
    type Err = MatrixError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.splitn(2, '-');
        let topological_ordering = parts
            .next()
            .ok_or_else(|| MatrixError::invalid_param("Invalid next batch"))?
            .parse()
            .map_err(|_| MatrixError::invalid_param("Invalid next batch"))?;
        let stream_ordering = parts
            .next()
            .ok_or_else(|| MatrixError::invalid_param("Invalid next batch"))?
            .parse()
            .map_err(|_| MatrixError::invalid_param("Invalid next batch"))?;

        Ok(ThreadsNextBatch {
            topological_ordering,
            stream_ordering,
        })
    }
}

pub fn get_threads(
    room_id: &RoomId,
    user_id: &UserId,
    include: &IncludeThreads,
    limit: i64,
    from_token: Option<ThreadsNextBatch>,
) -> AppResult<(Vec<(OwnedEventId, PduEvent)>, Option<ThreadsNextBatch>)> {
    let room_threads = if let Some(from_token) = from_token {
        room_threads::table
            .filter(room_threads::room_id.eq(room_id))
            .filter(room_threads::topological_ordering.le(from_token.topological_ordering))
            .filter(room_threads::stream_ordering.lt(from_token.stream_ordering))
            .order_by(room_threads::topological_ordering.desc())
            .order_by(room_threads::stream_ordering.desc())
            .limit(limit)
            .load::<RoomThread>(&mut *db::connect()?)?
    } else {
        room_threads::table
            .filter(room_threads::room_id.eq(room_id))
            .order_by(room_threads::topological_ordering.desc())
            .order_by(room_threads::stream_ordering.desc())
            .limit(limit)
            .load::<RoomThread>(&mut *db::connect()?)?
    };
    let next_token = if let Some(last) = room_threads.last() {
        Some(ThreadsNextBatch {
            topological_ordering: last.topological_ordering,
            stream_ordering: last.stream_ordering,
        })
    } else {
        None
    };

    let mut events = Vec::with_capacity(room_threads.len());
    for room_thread in &room_threads {
        if let Some(pdu) = crate::room::timeline::get_pdu(&room_thread.id)? {
            events.push((room_thread.id.clone(), pdu));
        }
    }
    Ok((events, next_token))
}

pub fn add_to_thread(root_event_id: &EventId, pdu: &PduEvent) -> AppResult<()> {
    let root_pdu = crate::room::timeline::get_pdu(root_event_id)?
        .ok_or_else(|| MatrixError::invalid_param("Thread root pdu not found."))?;

    let mut root_pdu_json = crate::room::timeline::get_pdu_json(root_event_id)?
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

        crate::room::timeline::replace_pdu(root_event_id, &root_pdu_json)?;
    }

    let mut users = Vec::new();
    if let Some(user_ids) = get_participants(&root_event_id)? {
        users.extend_from_slice(&user_ids);
        users.push(pdu.sender.clone());
    } else {
        users.push(root_pdu.sender);
        users.push(pdu.sender.clone());
    }
    // TODO: fixme
    // update_participants(root_event_id, &users)?;
    Ok(())
}

fn get_participants(root_id: &EventId) -> AppResult<Option<Vec<OwnedUserId>>> {
    // TODO: fixme
    panic!("fixme")
    // if let Some(users) = self.threadid_user_ids.get(root_id)? {
    //     Ok(Some(
    //         users
    //             .split(|b| *b == 0xff)
    //             .map(|bytes| {
    //                 UserId::parse(
    //                     utils::string_from_bytes(bytes)
    //                         .map_err(|_| AppError::public("Invalid UserId bytes in threadid_user_ids."))?,
    //                 )
    //                 .map_err(|_| AppError::public("Invalid UserId in threadid_user_ids."))
    //             })
    //             .collect(),
    //     ))
    // } else {
    //     Ok(None)
    // }
}
