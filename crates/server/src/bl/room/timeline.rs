use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use crate::core::client::filter::UrlFilter;
use crate::core::events::push_rules::PushRulesEventContent;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::create::RoomCreateEventContent;
use crate::core::events::room::encrypted::Relation;
use crate::core::events::room::member::MembershipState;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::events::{GlobalAccountDataEventType, StateEventType, TimelineEventType};
use crate::core::federation::backfill::backfill_request;
use crate::core::federation::backfill::BackfillResBody;
use crate::core::identifiers::*;
use crate::core::push::{Action, Ruleset, Tweak};
use crate::core::serde::{to_canonical_value, CanonicalJsonObject, CanonicalJsonValue, RawJsonValue};
use crate::core::state::Event;
use crate::core::{user_id, Direction, RoomVersion, UnixMillis};
use crate::event::{DbEventData, NewDbEvent};
use crate::room::state::CompressedStateEvent;
use crate::schema::*;
use crate::{db, utils, AppError, AppResult, MatrixError, SigningKeys};
use crate::{
    diesel_exists,
    event::{EventHash, PduBuilder, PduEvent},
    JsonValue,
};
use diesel::prelude::*;
use diesel::sql_types::Json;
use palpo_core::client::filter::RoomEventFilter;
use palpo_core::federation::backfill::BackfillReqArgs;
use serde::Deserialize;
use serde_json::value::to_raw_value;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use ulid::Ulid;

pub static LAST_TIMELINE_COUNT_CACHE: LazyLock<Mutex<HashMap<OwnedRoomId, i64>>> = LazyLock::new(Default::default);
// pub static PDU_CACHE: LazyLock<Mutex<LruCache<OwnedRoomId, Arc<PduEvent>>>> = LazyLock::new(Default::default);

#[tracing::instrument]
pub fn first_pdu_in_room(room_id: &RoomId) -> AppResult<Option<PduEvent>> {
    event_datas::table
        .filter(event_datas::room_id.eq(room_id))
        .order(event_datas::event_sn.asc())
        .select(event_datas::json_data)
        .first::<JsonValue>(&mut *db::connect()?)
        .optional()
        .map(|json| json.map(|json| serde_json::from_value(json).expect("PduEvent is valid")))
        .map_err(Into::into)
}

#[tracing::instrument]
pub fn last_event_sn(user_id: &UserId, room_id: &RoomId) -> AppResult<i64> {
    events::table
        .filter(events::room_id.eq(room_id))
        .select(events::sn)
        .order(events::sn.desc())
        .first::<i64>(&mut *db::connect()?)
        .map_err(Into::into)
}

/// Returns the `count` of this pdu's id.
pub fn get_event_sn(event_id: &EventId) -> AppResult<Option<i64>> {
    Ok(events::table
        .find(event_id)
        .select(events::sn)
        .first::<i64>(&mut *db::connect()?)
        .optional()?)
}

/// Returns the json of a pdu.
pub fn get_pdu_json(event_id: &EventId) -> AppResult<Option<CanonicalJsonObject>> {
    let query = events::table.filter(events::id.eq(event_id));
    if diesel_exists!(query, &mut *db::connect()?)? {
        event_datas::table
            .filter(event_datas::event_id.eq(event_id))
            .select(event_datas::json_data)
            .first::<JsonValue>(&mut *db::connect()?)
            .optional()?
            .map(|json| serde_json::from_value(json).map_err(|_| AppError::internal("Invalid PDU in db.")))
            .transpose()
    } else {
        Ok(None)
    }
}

/// Returns the pdu.
///
/// Checks the `eventid_outlierpdu` Tree if not found in the timeline.
pub fn get_non_outlier_pdu(event_id: &EventId) -> AppResult<Option<PduEvent>> {
    let query = events::table
        .filter(events::outlier.eq(false))
        .filter(events::id.eq(event_id));
    if diesel_exists!(query, &mut *db::connect()?)? {
        event_datas::table
            .filter(event_datas::event_id.eq(event_id))
            .select(event_datas::json_data)
            .first::<JsonValue>(&mut *db::connect()?)
            .optional()?
            .map(|json| serde_json::from_value(json).map_err(|_| AppError::internal("Invalid PDU in db.")))
            .transpose()
    } else {
        Ok(None)
    }
}

/// Returns the pdu.
///
/// Checks database if not found in the timeline.
// TODO: use cache
pub fn get_pdu(event_id: &EventId) -> AppResult<Option<PduEvent>> {
    // if let Some(p) = PDU_CACHE.lock().unwrap().get_mut(event_id) {
    //     return Ok(Some(Arc::clone(p)));
    // }
    if let Ok(Some(pdu)) = get_non_outlier_pdu(event_id) {
        // PDU_CACHE::lock().unwrap().insert(event_id.to_owned(), Arc::clone(&pdu));
        Ok(Some(pdu))
    } else {
        Ok(None)
    }
}

/// Removes a pdu and creates a new one with the same id.
#[tracing::instrument]
pub fn replace_pdu(event_id: &EventId, pdu_json: &CanonicalJsonObject) -> AppResult<()> {
    diesel::update(event_datas::table.filter(event_datas::event_id.eq(event_id)))
        .set(event_datas::json_data.eq(serde_json::to_value(pdu_json)?))
        .execute(&mut *db::connect()?)?;
    // PDU_CACHE.lock().unwrap().remove(&(*pdu.event_id).to_owned());

    Ok(())
}

/// Creates a new persisted data unit and adds it to a room.
///
/// By this point the incoming event should be fully authenticated, no auth happens
/// in `append_pdu`.
///
/// Returns pdu id
#[tracing::instrument(skip(pdu, pdu_json, leaves))]
pub fn append_pdu(pdu: &PduEvent, mut pdu_json: CanonicalJsonObject, leaves: Vec<OwnedEventId>) -> AppResult<()> {
    let conf = crate::config();
    // Make unsigned fields correct. This is not properly documented in the spec, but state
    // events need to have previous content in the unsigned field, so clients can easily
    // interpret things like membership changes
    if let Some(state_key) = &pdu.state_key {
        if let CanonicalJsonValue::Object(unsigned) = pdu_json
            .entry("unsigned".to_owned())
            .or_insert_with(|| CanonicalJsonValue::Object(Default::default()))
        {
            if let Some(state_hash) = crate::room::state::get_pdu_frame_id(&pdu.event_id).unwrap() {
                if let Some(prev_state) =
                    crate::room::state::get_pdu(state_hash, &pdu.kind.to_string().into(), state_key).unwrap()
                {
                    unsigned.insert(
                        "prev_content".to_owned(),
                        CanonicalJsonValue::Object(
                            utils::to_canonical_object(prev_state.content.clone())
                                .expect("event is valid, we just created it"),
                        ),
                    );
                }
            }
        } else {
            error!("Invalid unsigned type in pdu.");
        }
    }
    crate::room::state::set_forward_extremities(&pdu.room_id, leaves)?;
    // Mark as read first so the sending client doesn't get a notification even if appending
    // fails
    crate::room::receipt::set_private_read(&pdu.room_id, &pdu.sender, &pdu.event_id, pdu.event_sn)?;
    crate::room::user::reset_notification_counts(&pdu.sender, &pdu.room_id)?;

    // Insert pdu
    diesel::insert_into(event_datas::table)
        .values(DbEventData {
            event_id: (&*pdu.event_id).to_owned(),
            event_sn: pdu.event_sn,
            room_id: pdu.room_id.to_owned(),
            internal_metadata: None,
            json_data: serde_json::to_value(&pdu_json)?,
            format_version: None,
        })
        .execute(&mut db::connect()?)?;

    // See if the event matches any known pushers
    let power_levels: RoomPowerLevelsEventContent =
        crate::room::state::get_state(&pdu.room_id, &StateEventType::RoomPowerLevels, "")?
            .map(|ev| {
                serde_json::from_str(ev.content.get())
                    .map_err(|_| AppError::internal("invalid m.room.power_levels event"))
            })
            .transpose()?
            .unwrap_or_default();

    let sync_pdu = pdu.to_sync_room_event();

    let mut notifies = Vec::new();
    let mut highlights = Vec::new();

    for user in crate::room::get_our_real_users(&pdu.room_id)?.iter() {
        // Don't notify the user of their own events
        if user == &pdu.sender {
            continue;
        }

        let rules_for_user = crate::user::get_data::<PushRulesEventContent>(
            user,
            None,
            &GlobalAccountDataEventType::PushRules.to_string(),
        )?
        .map(|content: PushRulesEventContent| content.global)
        .unwrap_or_else(|| Ruleset::server_default(user));

        let mut highlight = false;
        let mut notify = false;

        for action in crate::user::pusher::get_actions(user, &rules_for_user, &power_levels, &sync_pdu, &pdu.room_id)? {
            match action {
                Action::Notify => notify = true,
                Action::SetTweak(Tweak::Highlight(true)) => {
                    highlight = true;
                }
                _ => {}
            };
        }

        if notify {
            notifies.push(user.clone());
        }

        if highlight {
            highlights.push(user.clone());
        }

        for push_key in crate::user::pusher::get_push_keys(user)? {
            crate::sending::send_push_pdu(&pdu.event_id, user, push_key)?;
        }
    }
    increment_notification_counts(&pdu.room_id, notifies, highlights)?;

    match pdu.kind {
        TimelineEventType::RoomRedaction => {
            if let Some(redact_id) = &pdu.redacts {
                redact_pdu(redact_id, pdu)?;
            }
        }
        TimelineEventType::SpaceChild => {
            if let Some(_state_key) = &pdu.state_key {
                crate::room::space::ROOM_ID_SPACE_CHUNK_CACHE
                    .lock()
                    .unwrap()
                    .remove(&pdu.room_id);
            }
        }
        TimelineEventType::RoomMember => {
            if let Some(state_key) = &pdu.state_key {
                #[derive(Deserialize)]
                struct ExtractMembership {
                    membership: MembershipState,
                }

                // if the state_key fails
                let target_user_id = UserId::parse(state_key.clone()).expect("This state_key was previously validated");

                let content = serde_json::from_str::<ExtractMembership>(pdu.content.get())
                    .map_err(|_| AppError::internal("Invalid content in pdu."))?;

                let invite_state = match content.membership {
                    MembershipState::Invite => {
                        let state = crate::room::state::calculate_invite_state(pdu)?;
                        Some(state)
                    }
                    _ => None,
                };

                //  Update our membership info, we do this here incase a user is invited
                // and immediately leaves we need the DB to record the invite event for auth
                crate::room::update_membership(
                    &pdu.event_id,
                    pdu.event_sn,
                    &pdu.room_id,
                    &target_user_id,
                    content.membership,
                    &pdu.sender,
                    invite_state,
                )?;
            }
        }
        TimelineEventType::RoomMessage => {
            #[derive(Deserialize)]
            struct ExtractBody {
                body: Option<String>,
            }

            let content = serde_json::from_str::<ExtractBody>(pdu.content.get())
                .map_err(|_| AppError::internal("Invalid content in pdu."))?;

            if let Some(body) = content.body {
                let admin_room = crate::room::resolve_local_alias(
                    <&RoomAliasId>::try_from(format!("#admins:{}", &conf.server_name).as_str())
                        .expect("#admins:server_name is a valid room alias"),
                )?;
                let server_user = format!("@palpo:{}", &conf.server_name);

                let to_palpo = body.starts_with(&format!("{server_user}: "))
                    || body.starts_with(&format!("{server_user} "))
                    || body == format!("{server_user}:")
                    || body == format!("{server_user}");

                // This will evaluate to false if the emergency password is set up so that
                // the administrator can execute commands as palpo
                let from_palpo = pdu.sender == server_user && conf.emergency_password.is_none();

                if to_palpo && !from_palpo && admin_room.as_ref() == Some(&pdu.room_id) {
                    crate::admin::process_message(body);
                }
            }
        }
        _ => {}
    }

    // Update Relationships
    #[derive(Deserialize)]
    struct ExtractRelatesTo {
        #[serde(rename = "m.relates_to")]
        relates_to: Relation,
    }

    #[derive(Clone, Debug, Deserialize)]
    struct ExtractEventId {
        event_id: OwnedEventId,
    }
    #[derive(Clone, Debug, Deserialize)]
    struct ExtractRelatesToEventId {
        #[serde(rename = "m.relates_to")]
        relates_to: ExtractEventId,
    }

    // if let Ok(content) = serde_json::from_str::<ExtractRelatesToEventId>(pdu.content.get()) {
    //     crate::room::pdu_metadata::add_relation(&pdu.room_id,
    //         &pdu.event_id,
    //         &content.relates_to.event_id,
    //         content.relates_to.rel_type(),
    //     )?;
    // }

    if let Ok(content) = serde_json::from_str::<ExtractRelatesTo>(pdu.content.get()) {
        let rel_type = content.relates_to.rel_type();
        match content.relates_to {
            Relation::Reply { in_reply_to } => {
                // We need to do it again here, because replies don't have
                // event_id as a top level field
                let child_event_type = events::table
                    .find(&in_reply_to.event_id)
                    .select(events::event_type)
                    .first::<String>(&mut *db::connect()?)?;
                crate::room::pdu_metadata::add_relation(
                    &pdu.room_id,
                    &pdu.event_id,
                    &in_reply_to.event_id,
                    child_event_type,
                    rel_type,
                )?;
            }
            Relation::Thread(thread) => {
                crate::room::thread::add_to_thread(&thread.event_id, pdu)?;
            }
            _ => {} // TODO: Aggregate other types
        }
    }

    for appservice in crate::appservice::all()?.values() {
        if crate::room::appservice_in_room(&pdu.room_id, &appservice)? {
            crate::sending::send_pdu_appservice(appservice.registration.id.clone(), &pdu.event_id)?;
            continue;
        }

        // If the RoomMember event has a non-empty state_key, it is targeted at someone.
        // If it is our appservice user, we send this PDU to it.
        if pdu.kind == TimelineEventType::RoomMember {
            if let Some(state_key_uid) = &pdu
                .state_key
                .as_ref()
                .and_then(|state_key| UserId::parse(state_key.as_str()).ok())
            {
                if let Some(appservice_uid) =
                    UserId::parse_with_server_name(&*appservice.registration.sender_localpart, &conf.server_name).ok()
                {
                    if state_key_uid == &appservice_uid {
                        crate::sending::send_pdu_appservice(appservice.registration.id.clone(), &pdu.event_id)?;
                        continue;
                    }
                }
            }
        }

        let matching_users = || {
            crate::server_name() == pdu.sender.server_name() && appservice.is_user_match(&pdu.sender)
                || pdu.kind == TimelineEventType::RoomMember
                    && pdu.state_key.as_ref().map_or(false, |state_key| {
                        UserId::parse(state_key).map_or(false, |user_id| {
                            crate::server_name() == user_id.server_name() && appservice.is_user_match(&user_id)
                        })
                    })
        };
        let matching_aliases = |conn: &mut PgConnection| {
            crate::room::local_aliases_for_room(&pdu.room_id)
                .unwrap_or_default()
                .iter()
                .any(|room_alias| appservice.aliases.is_match(room_alias.as_str()))
                || if let Ok(Some(pdu)) =
                    crate::room::state::get_state(&pdu.room_id, &StateEventType::RoomCanonicalAlias, "")
                {
                    serde_json::from_str::<RoomCanonicalAliasEventContent>(pdu.content.get()).map_or(false, |content| {
                        content
                            .alias
                            .map_or(false, |alias| appservice.aliases.is_match(alias.as_str()))
                            || content
                                .alt_aliases
                                .iter()
                                .any(|alias| appservice.aliases.is_match(alias.as_str()))
                    })
                } else {
                    false
                }
        };

        if matching_aliases(&mut *db::connect()?) || appservice.rooms.is_match(pdu.room_id.as_str()) || matching_users()
        {
            crate::sending::send_pdu_appservice(appservice.registration.id.clone(), &pdu.event_id)?;
        }
    }
    Ok(())
}

fn increment_notification_counts(
    room_id: &RoomId,
    notifies: Vec<OwnedUserId>,
    highlights: Vec<OwnedUserId>,
) -> AppResult<()> {
    for user_id in notifies {
        diesel::update(
            event_push_summaries::table
                .filter(event_push_summaries::user_id.eq(&user_id))
                .filter(event_push_summaries::room_id.eq(room_id)),
        )
        .set(event_push_summaries::notification_count.eq(event_push_summaries::notification_count + 1))
        .execute(&mut db::connect()?)?;
    }
    for user_id in highlights {
        diesel::update(
            event_push_summaries::table
                .filter(event_push_summaries::user_id.eq(&user_id))
                .filter(event_push_summaries::room_id.eq(room_id)),
        )
        .set(event_push_summaries::highlight_count.eq(event_push_summaries::highlight_count + 1))
        .execute(&mut db::connect()?)?;
    }

    Ok(())
}

pub fn create_hash_and_sign_event(
    pdu_builder: PduBuilder,
    sender_id: &UserId,
    room_id: &RoomId,
) -> AppResult<(PduEvent, CanonicalJsonObject)> {
    let PduBuilder {
        event_type,
        content,
        unsigned,
        state_key,
        redacts,
    } = pdu_builder;

    let conf = crate::config();
    let prev_events: Vec<_> = crate::room::state::get_forward_extremities(room_id)?
        .into_iter()
        .take(20)
        .collect();

    let create_event = crate::room::state::get_state(room_id, &StateEventType::RoomCreate, "")?;

    let create_event_content: Option<RoomCreateEventContent> = create_event
        .as_ref()
        .map(|create_event| {
            serde_json::from_str(create_event.content.get()).map_err(|e| {
                warn!("Invalid create event: {}", e);
                AppError::internal("Invalid create event in database.")
            })
        })
        .transpose()?;

    // If there was no create event yet, assume we are creating a room with the default
    // version right now
    let room_version_id =
        create_event_content.map_or(conf.room_version.clone(), |create_event| create_event.room_version);
    let room_version = RoomVersion::new(&room_version_id).expect("room version is supported");

    let auth_events =
        crate::room::state::get_auth_events(room_id, &event_type, sender_id, state_key.as_deref(), &content)?;

    // Our depth is the maximum depth of prev_events + 1
    let depth = prev_events
        .iter()
        .filter_map(|event_id| Some(crate::room::timeline::get_pdu(event_id).ok()??.depth))
        .max()
        .unwrap_or_else(|| 0)
        + 1;

    let mut unsigned = unsigned.unwrap_or_default();

    if let Some(state_key) = &state_key {
        if let Some(prev_pdu) = crate::room::state::get_state(room_id, &event_type.to_string().into(), state_key)? {
            unsigned.insert(
                "prev_content".to_owned(),
                serde_json::from_str(prev_pdu.content.get()).expect("string is valid json"),
            );
            unsigned.insert(
                "prev_sender".to_owned(),
                serde_json::to_value(&prev_pdu.sender).expect("UserId::to_value always works"),
            );
        }
    }

    let event_id = OwnedEventId::try_from(format!("$will_fill_{}", Ulid::new().to_string())).unwrap();
    let content_value: JsonValue = serde_json::from_str(&content.get())?;
    let new_db_event = NewDbEvent {
        id: event_id.to_owned(),
        event_type: event_type.to_string(),
        room_id: room_id.to_owned(),
        unrecognized_keys: None,
        depth: depth as i64,
        origin_server_ts: Some(UnixMillis::now()),
        received_at: None,
        sender_id: Some(sender_id.to_owned()),
        contains_url: content_value.get("url").is_some(),
        worker_id: None,
        state_key: state_key.clone(),
        processed: false,
        outlier: false,
        soft_failed: false,
        rejection_reason: None,
    };
    let event_sn = diesel::insert_into(events::table)
        .values(&new_db_event)
        .on_conflict(events::id)
        .do_update()
        .set(&new_db_event)
        .returning(events::sn)
        .get_result::<i64>(&mut *db::connect()?)?;

    let mut pdu = PduEvent {
        event_id: event_id.into(),
        event_sn,
        room_id: room_id.to_owned(),
        sender: sender_id.to_owned(),
        origin_server_ts: UnixMillis::now(),
        kind: event_type,
        content,
        state_key,
        prev_events,
        depth,
        auth_events: auth_events.values().map(|pdu| pdu.event_id.clone()).collect(),
        redacts,
        unsigned: if unsigned.is_empty() {
            None
        } else {
            Some(to_raw_value(&unsigned).expect("to_raw_value always works"))
        },
        hashes: EventHash {
            sha256: "aaa".to_owned(),
        },
        signatures: None,
    };

    let auth_checked = crate::core::state::event_auth::auth_check(
        &room_version,
        &pdu,
        None::<PduEvent>, // TODO: third_party_invite
        |k, s| auth_events.get(&(k.clone(), s.to_owned())),
    )
    .map_err(|e| {
        error!("{:?}", e);
        AppError::internal("Auth check failed when hash and sign event")
    })?;

    if !auth_checked {
        return Err(MatrixError::forbidden("Event is not authorized.").into());
    }

    // Hash and sign
    let mut pdu_json = utils::to_canonical_object(&pdu).expect("event is valid, we just created it");

    pdu_json.remove("event_id");

    // Add origin because synapse likes that (and it's required in the spec)
    pdu_json.insert(
        "origin".to_owned(),
        to_canonical_value(&conf.server_name).expect("server name is a valid CanonicalJsonValue"),
    );

    match crate::core::signatures::hash_and_sign_event(
        conf.server_name.as_str(),
        crate::keypair(),
        &mut pdu_json,
        &room_version_id,
    ) {
        Ok(_) => {}
        Err(e) => {
            return match e {
                crate::core::signatures::Error::PduSize => Err(MatrixError::too_large("Message is too long").into()),
                _ => Err(MatrixError::unknown("Signing event failed").into()),
            }
        }
    }

    // Generate event id
    let event_id = EventId::parse_arc(format!(
        "${}",
        crate::core::signatures::reference_hash(&pdu_json, &room_version_id)
            .expect("palpo can calculate reference hashes")
    ))
    .expect("palpo's reference hashes are valid event ids");
    pdu.event_id = event_id.clone();

    diesel::update(events::table.filter(events::sn.eq(event_sn)))
        .set(events::id.eq(&OwnedEventId::from(event_id)))
        .execute(&mut db::connect()?)?;

    pdu_json.insert(
        "event_id".to_owned(),
        CanonicalJsonValue::String(pdu.event_id.as_str().to_owned()),
    );

    // Generate short event id
    let _point_id =
        crate::room::state::ensure_point(room_id, &pdu.event_id, crate::event::get_event_sn(&pdu.event_id)?)?;

    Ok((pdu, pdu_json))
}

/// Creates a new persisted data unit and adds it to a room.
#[tracing::instrument]
pub fn build_and_append_pdu(pdu_builder: PduBuilder, sender: &UserId, room_id: &RoomId) -> AppResult<PduEvent> {
    let (pdu, pdu_json) = create_hash_and_sign_event(pdu_builder, sender, room_id)?;
    let conf = crate::config();
    let admin_room = crate::room::resolve_local_alias(
        <&RoomAliasId>::try_from(format!("#admins:{}", &conf.server_name).as_str())
            .expect("#admins:server_name is a valid room alias"),
    )?;
    if admin_room.filter(|v| v == room_id).is_some() {
        match pdu.event_type() {
            TimelineEventType::RoomEncryption => {
                warn!("Encryption is not allowed in the admins room");
                return Err(MatrixError::forbidden("Encryption is not allowed in the admins room.").into());
            }
            TimelineEventType::RoomMember => {
                #[derive(Deserialize)]
                struct ExtractMembership {
                    membership: MembershipState,
                }

                let target = pdu
                    .state_key
                    .clone()
                    .filter(|v| v.starts_with("@"))
                    .unwrap_or(sender.as_str().to_owned());
                let server_name = &conf.server_name;
                let server_user = format!("@palpo:{}", server_name);
                let content = serde_json::from_str::<ExtractMembership>(pdu.content.get())
                    .map_err(|_| AppError::internal("Invalid content in pdu."))?;

                if content.membership == MembershipState::Leave {
                    if target == server_user {
                        warn!("Palpo user cannot leave from admins room");
                        return Err(MatrixError::forbidden("Palpo user cannot leave from admins room.").into());
                    }

                    let count = crate::room::get_joined_users(room_id)?
                        .iter()
                        .filter(|m| m.server_name() == server_name)
                        .filter(|m| m.as_str() != target)
                        .count();
                    if count < 2 {
                        warn!("Last admin cannot leave from admins room");
                        return Err(MatrixError::forbidden("Last admin cannot leave from admins room.").into());
                    }
                }

                if content.membership == MembershipState::Ban && pdu.state_key().is_some() {
                    if target == server_user {
                        warn!("Palpo user cannot be banned in admins room");
                        return Err(MatrixError::forbidden("Palpo user cannot be banned in admins room.").into());
                    }

                    let count = crate::room::get_joined_users(room_id)?
                        .iter()
                        .filter(|m| m.server_name() == server_name)
                        .filter(|m| m.as_str() != target)
                        .count();
                    if count < 2 {
                        warn!("Last admin cannot be banned in admins room");
                        return Err(MatrixError::forbidden("Last admin cannot be banned in admins room.").into());
                    }
                }
            }
            _ => {}
        }
    }

    // We append to state before appending the pdu, so we don't have a moment in time with the
    // pdu without it's state. This is okay because append_pdu can't fail.
    let frame_id = crate::room::state::append_to_state(&pdu)?;

    append_pdu(
        &pdu,
        pdu_json,
        // Since this PDU references all pdu_leaves we can update the leaves
        // of the room
        vec![(*pdu.event_id).to_owned()],
    )?;

    // We set the room state after inserting the pdu, so that we never have a moment in time
    // where events in the current room state do not exist
    crate::room::state::set_room_state(room_id, frame_id)?;

    let mut servers: HashSet<OwnedServerName> = crate::room::participating_servers(room_id)?.into_iter().collect();

    // In case we are kicking or banning a user, we need to inform their server of the change
    if pdu.kind == TimelineEventType::RoomMember {
        if let Some(state_key_uid) = &pdu
            .state_key
            .as_ref()
            .and_then(|state_key| UserId::parse(state_key.as_str()).ok())
        {
            servers.insert(state_key_uid.server_name().to_owned());
        }
    }

    // Remove our server from the server list since it will be added to it by room_servers() and/or the if statement above
    servers.remove(&conf.server_name);

    crate::sending::send_pdu(servers.into_iter(), &pdu.event_id)?;

    Ok(pdu)
}

/// Append the incoming event setting the state snapshot to the state from the
/// server that sent the event.
#[tracing::instrument(skip_all)]
pub fn append_incoming_pdu(
    pdu: &PduEvent,
    pdu_json: CanonicalJsonObject,
    new_room_leaves: Vec<OwnedEventId>,
    state_ids_compressed: Arc<HashSet<CompressedStateEvent>>,
    soft_fail: bool,
) -> AppResult<()> {
    let event_sn = crate::event::get_event_sn(&pdu.event_id)?;
    crate::room::state::ensure_point(&pdu.room_id, &pdu.event_id, event_sn)?;
    // We append to state before appending the pdu, so we don't have a moment in time with the
    // pdu without it's state. This is okay because append_pdu can't fail.
    crate::room::state::save_state(&pdu.room_id, state_ids_compressed)?;

    if soft_fail {
        // crate::room::pdu_metadata::mark_as_referenced(&pdu.room_id, &pdu.prev_events)?;
        crate::room::state::set_forward_extremities(&pdu.room_id, new_room_leaves)?;
        return Ok(());
    }

    crate::room::timeline::append_pdu(pdu, pdu_json, new_room_leaves)
}

/// Returns an iterator over all PDUs in a room.
pub fn all_pdus(user_id: &UserId, room_id: &RoomId) -> AppResult<Vec<(i64, PduEvent)>> {
    get_pdus_forward(user_id, room_id, 0, usize::MAX, None)
}
pub fn get_pdus_forward(
    user_id: &UserId,
    room_id: &RoomId,
    occur_sn: i64,
    limit: usize,
    filter: Option<&RoomEventFilter>,
) -> AppResult<Vec<(i64, PduEvent)>> {
    get_pdus(user_id, room_id, occur_sn, limit, filter, Direction::Forward)
}
pub fn get_pdus_backward(
    user_id: &UserId,
    room_id: &RoomId,
    occur_sn: i64,
    limit: usize,
    filter: Option<&RoomEventFilter>,
) -> AppResult<Vec<(i64, PduEvent)>> {
    get_pdus(user_id, room_id, occur_sn, limit, filter, Direction::Backward)
}

/// Returns an iterator over all events and their tokens in a room that happened before the
/// event with id `until` in reverse-chronological order.
#[tracing::instrument]
pub fn get_pdus(
    user_id: &UserId,
    room_id: &RoomId,
    occur_sn: i64,
    limit: usize,
    filter: Option<&RoomEventFilter>,
    dir: Direction,
) -> AppResult<Vec<(i64, PduEvent)>> {
    let mut query = events::table.filter(events::room_id.eq(room_id)).into_boxed();
    if dir == Direction::Forward {
        query = query.filter(events::sn.ge(occur_sn));
    } else {
        query = query.filter(events::sn.le(occur_sn));
    };

    if let Some(filter) = filter {
        if let Some(url_filter) = &filter.url_filter {
            match url_filter {
                UrlFilter::EventsWithUrl => query = query.filter(events::contains_url.eq(true)),
                UrlFilter::EventsWithoutUrl => query = query.filter(events::contains_url.eq(false)),
            }
        }
        if !filter.not_types.is_empty() {
            query = query.filter(events::event_type.ne_all(&filter.not_types));
        }
        if !filter.not_rooms.is_empty() {
            query = query.filter(events::room_id.ne_all(&filter.not_rooms));
        }
        if let Some(rooms) = &filter.rooms {
            if !rooms.is_empty() {
                query = query.filter(events::room_id.eq_any(rooms));
            }
        }
        if let Some(senders) = &filter.senders {
            if !senders.is_empty() {
                query = query.filter(events::sender_id.eq_any(senders));
            }
        }
        if let Some(types) = &filter.types {
            if !types.is_empty() {
                query = query.filter(events::event_type.eq_any(types));
            }
        }
    }

    let datas = if dir == Direction::Forward {
        event_datas::table
            .filter(event_datas::event_id.eq_any(query.select(events::id)))
            .order(event_datas::event_sn.asc())
            .limit(utils::usize_to_i64(limit))
            .select((event_datas::event_sn, event_datas::json_data))
            .load::<(i64, JsonValue)>(&mut *db::connect()?)?
    } else {
        event_datas::table
            .filter(event_datas::event_id.eq_any(query.select(events::id)))
            .order(event_datas::event_sn.desc()).limit(utils::usize_to_i64(limit))
            .select((event_datas::event_sn, event_datas::json_data))
            .load::<(i64, JsonValue)>(&mut *db::connect()?)?
    };

    let list = datas.into_iter().filter_map(|(sn, v)| {
        let mut pdu = serde_json::from_value::<PduEvent>(v).ok()?;

        if crate::room::state::user_can_see_event(user_id, room_id, &pdu.event_id).ok()? {
            if pdu.sender != user_id {
                pdu.remove_transaction_id().ok()?;
            }
            pdu.add_age().ok()?;
            Some((sn, pdu))
        } else {
            None
        }
    });
    // if dir == Direction::Forward {
    //     Ok(list.rev().collect::<Vec<(_, _)>>())
    // } else {
    Ok(list.collect::<Vec<(_, _)>>())
    // }
}

/// Replace a PDU with the redacted form.
#[tracing::instrument(skip(reason))]
pub fn redact_pdu(event_id: &EventId, reason: &PduEvent) -> AppResult<()> {
    // TODO: Don't reserialize, keep original json
    if let Some(mut pdu) = get_pdu(event_id)? {
        pdu.redact(reason)?;
        replace_pdu(&event_id, &utils::to_canonical_object(&pdu)?)?;
    }
    // If event does not exist, just noop
    Ok(())
}

#[tracing::instrument(skip(room_id))]
pub async fn backfill_if_required(room_id: &RoomId, from: i64) -> AppResult<()> {
    let pdus = all_pdus(&user_id!("@doesntmatter:palpo.im"), &room_id)?;
    let first_pdu = pdus.first();

    let Some(first_pdu) = first_pdu else { return Ok(()) };
    if first_pdu.0 < from {
        // No backfill required, there are still events between them
        return Ok(());
    }

    let power_levels: RoomPowerLevelsEventContent =
        crate::room::state::get_state(&room_id, &StateEventType::RoomPowerLevels, "")?
            .map(|ev| {
                serde_json::from_str(ev.content.get())
                    .map_err(|_| AppError::internal("invalid m.room.power_levels event"))
            })
            .transpose()?
            .unwrap_or_default();
    let mut admin_servers = power_levels
        .users
        .iter()
        .filter(|(_, level)| **level > power_levels.users_default)
        .map(|(user_id, _)| user_id.server_name())
        .collect::<HashSet<_>>();
    admin_servers.remove(&crate::server_name());

    // Request backfill
    for backfill_server in admin_servers {
        info!("Asking {backfill_server} for backfill");
        let response = backfill_request(
            backfill_server,
            BackfillReqArgs {
                room_id: room_id.to_owned(),
                v: vec![(&*first_pdu.1.event_id).to_owned()],
                limit: 100,
            },
        )?
        .send::<BackfillResBody>()
        .await;
        match response {
            Ok(response) => {
                let mut pub_key_map = RwLock::new(BTreeMap::new());
                for pdu in response.pdus {
                    if let Err(e) = backfill_pdu(backfill_server, pdu, &mut pub_key_map).await {
                        warn!("Failedcar to add backfilled pdu: {e}");
                    }
                }
                return Ok(());
            }
            Err(e) => {
                warn!("{backfill_server} could not provide backfill: {e}");
            }
        }
    }

    info!("No servers could backfill");
    Ok(())
}

#[tracing::instrument(skip(pdu))]
pub async fn backfill_pdu(
    origin: &ServerName,
    pdu: Box<RawJsonValue>,
    pub_key_map: &RwLock<BTreeMap<String, SigningKeys>>,
) -> AppResult<()> {
    let (event_id, value, room_id) = crate::parse_incoming_pdu(&pdu)?;

    // Skip the PDU if we already have it as a timeline event
    if let Some(pdu_id) = crate::room::timeline::get_pdu(&event_id)? {
        info!("We already know {event_id} at {pdu_id:?}");
        return Ok(());
    }

    crate::event::handler::handle_incoming_pdu(origin, &event_id, &room_id, value, false, pub_key_map).await?;

    let value = get_pdu_json(&event_id)?.expect("We just created it");
    let pdu = get_pdu(&event_id)?.expect("We just created it");

    // // Insert pdu
    // prepend_backfill_pdu(&pdu, &event_id, &value)?;

    if pdu.kind == TimelineEventType::RoomMessage {
        #[derive(Deserialize)]
        struct ExtractBody {
            body: Option<String>,
        }

        let content = serde_json::from_str::<ExtractBody>(pdu.content.get())
            .map_err(|_| AppError::internal("Invalid content in pdu."))?;
    }

    info!("Prepended backfill pdu");
    Ok(())
}

// fn prepend_backfill_pdu(
//     pdu_id: i64,
//     event_id: &EventId,
//     json: &CanonicalJsonObject,
//
// ) -> AppResult<()> {
// self.pduid_pdu.insert(
//     pdu_id,
//     &serde_json::to_vec(json).expect("CanonicalJsonObject is always a valid"),
// )?;

// self.eventid_pduid.insert(event_id.as_bytes(), pdu_id)?;
// self.eventid_outlierpdu.remove(event_id.as_bytes())?;

//     Ok(())
// }
