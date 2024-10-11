use std::sync::LazyLock;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use diesel::prelude::*;
use lru_cache::LruCache;
use tracing::{error, warn};

use crate::core::identifiers::*;
use crate::schema::*;
use crate::{db, AppResult, MatrixError};

#[derive(Insertable, Identifiable, AsChangeset, Queryable, Debug, Clone)]
#[diesel(table_name = event_auth_chains, primary_key(event_id))]
pub struct DbEventAuthChain {
    pub event_id: OwnedEventId,
    pub chain_id: i64,
    pub sequence_number: i64,
}

static AUTH_CHAIN_CACHE: LazyLock<Mutex<LruCache<Arc<OwnedEventId>, Arc<HashSet<i64>>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(100_000)));

pub fn get_cached_event_auth_chain(event_id: &EventId) -> AppResult<Option<Arc<HashSet<i64>>>> {
    // Check RAM cache
    if let Some(result) = AUTH_CHAIN_CACHE.lock().unwrap().get_mut(&event_id.to_owned()) {
        return Ok(Some(Arc::clone(result)));
    }

    let chain_id = event_auth_chains::table
        .find(event_id)
        .select(event_auth_chains::chain_id)
        .first::<i64>(&mut *db::connect()?)
        .optional()?;

    if let Some(chain_id) = chain_id {
        let auth_chain: Arc<HashSet<i64>> = Arc::new(vec![chain_id].into_iter().collect());
        // Cache in RAM
        AUTH_CHAIN_CACHE
            .lock()
            .unwrap()
            .insert(Arc::new(event_id.to_owned()), auth_chain.clone());

        return Ok(Some(auth_chain));
    }

    Ok(None)
}

pub fn cache_auth_chain(event_id: &EventId, auth_chain: Arc<HashSet<i64>>) -> AppResult<()> {
    for chain_id in auth_chain.iter() {
        let chain = DbEventAuthChain {
            event_id: event_id.to_owned(),
            chain_id: *chain_id,
            sequence_number: 1,
        };
        diesel::insert_into(event_auth_chains::table)
            .values(&chain)
            .on_conflict((event_auth_chains::event_id))
            .do_update()
            .set(&chain)
            .execute(&mut db::connect()?).ok();
    }

    // Cache in RAM
    AUTH_CHAIN_CACHE
        .lock()
        .unwrap()
        .insert(Arc::new(event_id.to_owned()), auth_chain);

    Ok(())
}

pub fn get_auth_chain(room_id: &RoomId, event_id: &EventId) -> AppResult<HashSet<Arc<EventId>>> {
    let mut full_auth_chain = HashSet::new();

    if let Some(cached) = crate::room::auth_chain::get_cached_event_auth_chain(event_id)? {
        full_auth_chain.extend(cached.iter().copied());
    } else {
        let mut todo = vec![Arc::from(event_id)];
        let mut found = HashSet::new();

        while let Some(event_id) = todo.pop() {
            match crate::room::timeline::get_pdu(&event_id) {
                Ok(Some(pdu)) => {
                    if pdu.room_id != room_id {
                        return Err(MatrixError::forbidden("Evil event in db").into());
                    }
                    for auth_event in &pdu.auth_events {
                        let point_id = crate::room::state::ensure_point(
                            room_id,
                            auth_event,
                            crate::event::get_event_sn(&auth_event)?,
                        )?;

                        if !found.contains(&point_id) {
                            found.insert(point_id);
                            todo.push(auth_event.clone());
                        }
                    }
                }
                Ok(None) => {
                    warn!(?event_id, "Could not find pdu mentioned in auth events");
                }
                Err(error) => {
                    error!(?event_id, ?error, "Could not load event in auth chain");
                }
            }
        }
        let auth_chain = Arc::new(found);
        crate::room::auth_chain::cache_auth_chain(&event_id, Arc::clone(&auth_chain))?;
        full_auth_chain.extend(auth_chain.iter().copied());
    }

    Ok(full_auth_chain
        .into_iter()
        .filter_map(move |sid| crate::room::state::get_point_event_id(sid).ok())
        .collect())
}
