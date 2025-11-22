use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::iter::once;
use std::sync::{LazyLock, Mutex};

use diesel::prelude::*;
use futures_util::stream;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::value::to_raw_value;
use ulid::Ulid;

use crate::core::client::filter::{RoomEventFilter, UrlFilter};
use crate::core::events::push_rules::PushRulesEventContent;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::encrypted::Relation;
use crate::core::events::room::member::MembershipState;
use crate::core::events::{GlobalAccountDataEventType, StateEventType, TimelineEventType};
use crate::core::federation::backfill::{BackfillReqArgs, BackfillResBody, backfill_request};
use crate::core::identifiers::*;
use crate::core::presence::PresenceState;
use crate::core::push::{Action, Ruleset, Tweak};
use crate::core::room_version_rules::RoomIdFormatVersion;
use crate::core::serde::{
    CanonicalJsonObject, CanonicalJsonValue, JsonValue, RawJsonValue, to_canonical_value,
    validate_canonical_json,
};
use crate::core::state::{Event, StateError, event_auth};
use crate::core::{Direction, Seqnum, UnixMillis};
use crate::data::room::{DbEvent, DbEventData, NewDbEvent};
use crate::data::schema::*;
use crate::data::{connect, diesel_exists};
use crate::event::{BatchToken, EventHash, PduBuilder, PduEvent, handler, parse_fetched_pdu};
use crate::room::{push_action, state, timeline};
use crate::utils::SeqnumQueueGuard;
use crate::{
    AppError, AppResult, GetUrlOrigin, MatrixError, RoomMutexGuard, SnPduEvent, config, data,
    membership, room, utils,
};

pub fn load_pdus_forward(
    user_id: Option<&UserId>,
    room_id: &RoomId,
    since_tk: Option<BatchToken>,
    until_tk: Option<BatchToken>,
    filter: Option<&RoomEventFilter>,
    limit: usize,
) -> AppResult<IndexMap<i64, SnPduEvent>> {
    load_pdus(
        user_id,
        room_id,
        since_tk,
        until_tk,
        limit,
        filter,
        Direction::Forward,
    )
}
pub fn load_pdus_backward(
    user_id: Option<&UserId>,
    room_id: &RoomId,
    since_tk: Option<BatchToken>,
    until_tk: Option<BatchToken>,
    filter: Option<&RoomEventFilter>,
    limit: usize,
) -> AppResult<IndexMap<i64, SnPduEvent>> {
    load_pdus(
        user_id,
        room_id,
        since_tk,
        until_tk,
        limit,
        filter,
        Direction::Backward,
    )
}

/// Returns an iterator over all events and their tokens in a room that happened before the
/// event with id `until` in reverse-chronological order.
/// Skips events before user joined the room.
#[tracing::instrument]
pub fn load_pdus(
    user_id: Option<&UserId>,
    room_id: &RoomId,
    mut since_tk: Option<BatchToken>,
    mut until_tk: Option<BatchToken>,
    limit: usize,
    filter: Option<&RoomEventFilter>,
    dir: Direction,
) -> AppResult<IndexMap<Seqnum, SnPduEvent>> {
    let mut list: IndexMap<Seqnum, SnPduEvent> = IndexMap::with_capacity(limit.clamp(10, 100));
    let mut offset = 0;
    println!(
        "============ Starting load_pdus since_tk: {since_tk:?} until_tk: {until_tk:?} limit: {limit} dir: {dir:?}"
    );

    while list.len() < limit {
        let mut query = events::table
            .filter(events::room_id.eq(room_id))
            .into_boxed();
        if dir == Direction::Forward {
            if let Some(since_tk) = since_tk {
                if let Some(topological_ordering) = since_tk.topological_ordering {
                    query = query.filter(
                        events::topological_ordering.ge(topological_ordering).or(
                            events::topological_ordering
                                .eq(topological_ordering)
                                .and(events::stream_ordering.ge(since_tk.stream_ordering)),
                        ),
                    );
                } else {
                    query = query.filter(events::stream_ordering.ge(since_tk.stream_ordering));
                }
            }
            if let Some(until_tk) = until_tk {
                if let Some(topological_ordering) = until_tk.topological_ordering {
                    query = query.filter(
                        events::topological_ordering.le(topological_ordering).or(
                            events::topological_ordering
                                .eq(topological_ordering)
                                .and(events::stream_ordering.le(until_tk.stream_ordering)),
                        ),
                    );
                } else {
                    query = query.filter(events::stream_ordering.le(until_tk.stream_ordering));
                }
            }
        } else {
            if let Some(since_tk) = since_tk {
                if let Some(topological_ordering) = since_tk.topological_ordering {
                    query = query.filter(
                        events::topological_ordering.le(topological_ordering).or(
                            events::topological_ordering
                                .eq(topological_ordering)
                                .and(events::stream_ordering.le(since_tk.stream_ordering)),
                        ),
                    );
                } else {
                    query = query.filter(events::stream_ordering.le(since_tk.stream_ordering));
                }
            }
            if let Some(until_tk) = until_tk {
                if let Some(topological_ordering) = until_tk.topological_ordering {
                    query = query.filter(
                        events::topological_ordering.ge(topological_ordering).or(
                            events::topological_ordering
                                .eq(topological_ordering)
                                .and(events::stream_ordering.ge(until_tk.stream_ordering)),
                        ),
                    );
                } else {
                    query = query.filter(events::stream_ordering.ge(until_tk.stream_ordering));
                }
            }
        }

        if let Some(filter) = filter {
            if let Some(url_filter) = &filter.url_filter {
                match url_filter {
                    UrlFilter::EventsWithUrl => query = query.filter(events::contains_url.eq(true)),
                    UrlFilter::EventsWithoutUrl => {
                        query = query.filter(events::contains_url.eq(false))
                    }
                }
            }
            if !filter.not_types.is_empty() {
                query = query.filter(events::ty.ne_all(&filter.not_types));
            }
            if !filter.not_rooms.is_empty() {
                query = query.filter(events::room_id.ne_all(&filter.not_rooms));
            }
            if let Some(rooms) = &filter.rooms
                && !rooms.is_empty()
            {
                query = query.filter(events::room_id.eq_any(rooms));
            }
            if let Some(senders) = &filter.senders
                && !senders.is_empty()
            {
                query = query.filter(events::sender_id.eq_any(senders));
            }
            if let Some(types) = &filter.types
                && !types.is_empty()
            {
                query = query.filter(events::ty.eq_any(types));
            }
        }
        let events: Vec<(OwnedEventId, Seqnum)> = if dir == Direction::Forward {
            query
                .order(events::topological_ordering.asc())
                .offset(offset)
                .limit(utils::usize_to_i64(limit))
                .select((events::id, events::sn))
                .load::<(OwnedEventId, Seqnum)>(&mut connect()?)?
                .into_iter()
                .rev()
                .collect()
        } else {
            let query = query
                .order(events::topological_ordering.desc())
                .offset(offset)
                .limit(utils::usize_to_i64(limit))
                .select((events::id, events::sn));
            crate::data::print_query!(&query);
            query
                .load::<(OwnedEventId, Seqnum)>(&mut connect()?)?
                .into_iter()
                .collect()
        };
        if events.is_empty() {
            break;
        }
        let count = events.len();
        offset += count as i64;

        for (event_id, event_sn) in events {
            println!(
                "Loading PDU for event_id: {}          {}",
                event_id, event_sn
            );
            if let Ok(mut pdu) = super::get_pdu(&event_id) {
                println!("=========");
                if let Some(user_id) = user_id {
                    if !pdu.user_can_see(user_id)? {
                        continue;
                    }
                    if pdu.sender != user_id {
                        pdu.remove_transaction_id()?;
                    }
                    pdu.add_unsigned_membership(user_id)?;
                }
                pdu.add_age()?;
                list.insert(event_sn, pdu);
                if list.len() >= limit {
                    break;
                }
            }
        }
        if count < limit {
            break;
        }
    }

    println!(
        "Loaded  PDUs {:#?}",
        events::table
            .order_by(events::sn.desc())
            .load::<DbEvent>(&mut connect()?)?
    );
    println!("============list: {:#?}", list);
    Ok(list)
}
