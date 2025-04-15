use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::ops::Deref;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};
use std::{future, iter};

use diesel::prelude::*;
use futures_util::{FutureExt, StreamExt, stream::FuturesUnordered};
use hickory_resolver::Resolver as HickoryResolver;
use hickory_resolver::config::*;
use hickory_resolver::name_server::TokioConnectionProvider;
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name as HyperName};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use salvo::oapi::ToSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, broadcast, watch::Receiver};
use tower_service::Service as TowerService;

use crate::core::client::sync_events;
use crate::core::federation::discovery::{OldVerifyKey, ServerSigningKeys, VerifyKey};
use crate::core::identifiers::*;
use crate::core::serde::{Base64, CanonicalJsonObject, JsonValue, RawJsonValue};
use crate::core::signatures::Ed25519KeyPair;
use crate::core::{Seqnum, UnixMillis};
use crate::data::connect;
use crate::data::schema::*;
use crate::{AppResult, MatrixError, ServerConfig};

pub async fn watch(user_id: &UserId, device_id: &DeviceId) -> AppResult<()> {
    let inbox_id = device_inboxes::table
        .filter(device_inboxes::user_id.eq(user_id))
        .filter(device_inboxes::device_id.eq(device_id))
        .order_by(device_inboxes::id.desc())
        .select(device_inboxes::id)
        .first::<i64>(&mut connect()?)
        .unwrap_or_default();
    let key_change_id = e2e_key_changes::table
        .filter(e2e_key_changes::user_id.eq(user_id))
        .order_by(e2e_key_changes::id.desc())
        .select(e2e_key_changes::id)
        .first::<i64>(&mut connect()?)
        .unwrap_or_default();
    let room_user_id = room_users::table
        .filter(room_users::user_id.eq(user_id))
        .order_by(room_users::id.desc())
        .select(room_users::id)
        .first::<i64>(&mut connect()?)
        .unwrap_or_default();

    let room_ids = crate::user::joined_rooms(user_id, 0)?;
    let last_event_sn = event_points::table
        .filter(event_points::room_id.eq_any(&room_ids))
        .filter(event_points::frame_id.is_not_null())
        .order_by(event_points::event_sn.desc())
        .select(event_points::event_sn)
        .first::<Seqnum>(&mut connect()?)
        .unwrap_or_default();

    let push_rule_sn = user_datas::table
        .filter(user_datas::user_id.eq(user_id))
        .filter(user_datas::data_type.eq("m.push_rules"))
        .order_by(user_datas::occur_sn.desc())
        .select(user_datas::occur_sn)
        .first::<i64>(&mut connect()?)
        .unwrap_or_default();

    let mut futures: FuturesUnordered<Pin<Box<dyn Future<Output = AppResult<()>> + Send>>> = FuturesUnordered::new();

    for room_id in room_ids.clone() {
        futures.push(Box::into_pin(Box::new(async move {
            crate::room::typing::wait_for_update(&room_id).await
        })));
    }
    futures.push(Box::into_pin(Box::new(async move {
        loop {
            if inbox_id
                < device_inboxes::table
                    .filter(device_inboxes::user_id.eq(user_id))
                    .filter(device_inboxes::device_id.eq(device_id))
                    .order_by(device_inboxes::id.desc())
                    .select(device_inboxes::id)
                    .first::<i64>(&mut connect()?)
                    .unwrap_or_default()
            {
                return Ok(());
            }
            if key_change_id
                < e2e_key_changes::table
                    .filter(e2e_key_changes::user_id.eq(user_id))
                    .order_by(e2e_key_changes::id.desc())
                    .select(e2e_key_changes::id)
                    .first::<i64>(&mut connect()?)
                    .unwrap_or_default()
            {
                return Ok(());
            }
            if room_user_id
                < room_users::table
                    .filter(room_users::user_id.eq(user_id))
                    .order_by(room_users::id.desc())
                    .select(room_users::id)
                    .first::<i64>(&mut connect()?)
                    .unwrap_or_default()
            {
                return Ok(());
            }
            if last_event_sn
                < event_points::table
                    .filter(event_points::room_id.eq_any(&room_ids))
                    .filter(event_points::frame_id.is_not_null())
                    .order_by(event_points::event_sn.desc())
                    .select(event_points::event_sn)
                    .first::<Seqnum>(&mut connect()?)
                    .unwrap_or_default()
            {
                return Ok(());
            }
            if push_rule_sn
                < user_datas::table
                    .filter(user_datas::user_id.eq(user_id))
                    .filter(user_datas::data_type.eq("m.push_rules"))
                    .order_by(user_datas::occur_sn.desc())
                    .select(user_datas::occur_sn)
                    .first::<i64>(&mut connect()?)
                    .unwrap_or_default()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })));
    // Wait until one of them finds something
    futures.next().await;
    Ok(())
}
