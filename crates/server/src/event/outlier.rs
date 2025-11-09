use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};

use diesel::prelude::*;

use crate::core::events::{StateEventType, TimelineEventType};
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonObject;
use crate::core::serde::RawJsonValue;
use crate::core::state::{Event, StateError, event_auth};
use crate::core::{self, Seqnum, UnixMillis};
use crate::data::room::{DbEventData, NewDbEvent};
use crate::data::schema::*;
use crate::data::{self, connect};
use crate::event::fetching::fetch_and_process_missing_state_by_ids;
use crate::event::handler::fetch_and_process_missing_prev_events;
use crate::event::{PduEvent, SnPduEvent, ensure_event_sn};
use crate::room::timeline;
use crate::utils::SeqnumQueueGuard;
use crate::{AppError, AppResult, MatrixError};

#[derive(Clone, Debug)]
pub struct OutlierPdu {
    pub pdu: PduEvent,
    pub json_data: CanonicalJsonObject,
    pub soft_failed: bool,

    pub remote_server: OwnedServerName,
    pub room_id: OwnedRoomId,
    pub room_version_id: RoomVersionId,
    pub event_sn: Option<Seqnum>,
}
impl AsRef<PduEvent> for OutlierPdu {
    fn as_ref(&self) -> &PduEvent {
        &self.pdu
    }
}
impl AsMut<PduEvent> for OutlierPdu {
    fn as_mut(&mut self) -> &mut PduEvent {
        &mut self.pdu
    }
}
impl DerefMut for OutlierPdu {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pdu
    }
}
impl Deref for OutlierPdu {
    type Target = PduEvent;

    fn deref(&self) -> &Self::Target {
        &self.pdu
    }
}

impl crate::core::state::Event for OutlierPdu {
    type Id = OwnedEventId;

    fn event_id(&self) -> &Self::Id {
        &self.event_id
    }

    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn sender(&self) -> &UserId {
        &self.sender
    }

    fn event_type(&self) -> &TimelineEventType {
        &self.event_ty
    }

    fn content(&self) -> &RawJsonValue {
        &self.content
    }

    fn origin_server_ts(&self) -> UnixMillis {
        self.origin_server_ts
    }

    fn state_key(&self) -> Option<&str> {
        self.state_key.as_deref()
    }

    fn prev_events(&self) -> &[Self::Id] {
        self.prev_events.deref()
    }

    fn auth_events(&self) -> &[Self::Id] {
        self.auth_events.deref()
    }

    fn redacts(&self) -> Option<&Self::Id> {
        self.redacts.as_ref()
    }

    fn rejected(&self) -> bool {
        self.pdu.rejected()
    }
}

impl OutlierPdu {
    pub fn save_without_fill_missing(
        self,
        known_events: &mut HashSet<OwnedEventId>,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        let Self {
            pdu,
            json_data,
            soft_failed,
            room_id,
            event_sn,
            ..
        } = self;
        if let Some(event_sn) = event_sn {
            return Ok((
                SnPduEvent {
                    pdu,
                    event_sn,
                    is_outlier: true,
                    soft_failed,
                },
                json_data,
                None,
            ));
        }
        let (event_sn, event_guard) = ensure_event_sn(&room_id, &pdu.event_id)?;
        let mut db_event = NewDbEvent::from_canonical_json(&pdu.event_id, event_sn, &json_data)?;
        db_event.is_outlier = true;
        db_event.soft_failed = soft_failed;
        db_event.is_rejected = pdu.rejection_reason.is_some();
        db_event.rejection_reason = pdu.rejection_reason.clone();
        db_event.save()?;
        DbEventData {
            event_id: pdu.event_id.clone(),
            event_sn,
            room_id: pdu.room_id.clone(),
            internal_metadata: None,
            json_data: serde_json::to_value(&json_data)?,
            format_version: None,
        }
        .save()?;

        let existed_events = events::table
            .filter(events::id.eq_any(&pdu.prev_events))
            .select(events::id)
            .load::<OwnedEventId>(&mut connect()?)?;
        let missing_events = pdu
            .prev_events
            .iter()
            .filter(|id| !known_events.contains(*id) && !existed_events.contains(id))
            .collect::<Vec<_>>();
        if !missing_events.is_empty() {
            data::room::add_timeline_gap(&room_id, event_sn)?;
        }

        Ok((
            SnPduEvent {
                pdu,
                event_sn,
                is_outlier: true,
                soft_failed,
            },
            json_data,
            event_guard,
        ))
    }
    pub async fn save_with_fill_missing(
        mut self,
        known_events: &mut HashSet<OwnedEventId>,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        let version_rules = crate::room::get_version_rules(&self.room_version_id)?;
        let auth_rules = &version_rules.authorization;

        let mut soft_failed = false;
        let mut rejection_reason = None;
        // 9. Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
        if let Err(e) = fetch_and_process_missing_prev_events(
            &self.remote_server,
            &self.room_id,
            &self.room_version_id,
            &self,
            known_events,
        )
        .await
        {
            if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
                if *kind == core::error::ErrorKind::BadJson {
                    rejection_reason = Some(format!("failed to bad prev events: {}", e));
                } else {
                    soft_failed = true;
                }
            } else {
                soft_failed = true;
            }
        }

        let (_auth_events, missing_auth_event_ids) =
            match timeline::get_may_missing_pdus(&self.room_id, &self.auth_events) {
                Ok(s) => s,
                Err(e) => {
                    info!("error getting auth events for {}: {}", self.event_id, e);
                    soft_failed = true;
                    (vec![], vec![])
                }
            };

        if !missing_auth_event_ids.is_empty() {
            if soft_failed {
                if let Err(e) = fetch_and_process_missing_state_by_ids(
                    &self.remote_server,
                    &self.room_id,
                    &self.room_version_id,
                    &self.event_id,
                )
                .await
                {
                    error!(
                        "failed to fetch missing auth events for {}: {:?}",
                        self.event_id, e
                    );
                }
                // } else {
                //     if let Err(_e) =
                //         fetch_and_process_auth_chain(&self.remote_server, &self.room_id, &self.event_id)
                //             .await
                //     {
                //         soft_failed = true;
                //     }
            }
        }
        let (auth_events, missing_auth_event_ids) =
            timeline::get_may_missing_pdus(&self.room_id, &self.auth_events)?;
        if !missing_auth_event_ids.is_empty() {
            warn!(
                "missing auth events for {}: {:?}",
                self.event_id, missing_auth_event_ids
            );
            soft_failed = true;
        } else {
            let rejected_auth_events = auth_events
                .iter()
                .filter_map(|pdu| {
                    if pdu.rejected() {
                        Some(pdu.event_id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if !rejected_auth_events.is_empty() {
                rejection_reason = Some(format!(
                    "event's auth events rejected: {rejected_auth_events:?}"
                ))
            }
        }

        let auth_events = auth_events
            .into_iter()
            .map(|auth_event| {
                (
                    (
                        auth_event.event_ty.to_string().into(),
                        auth_event.state_key.clone().unwrap_or_default(),
                    ),
                    auth_event,
                )
            })
            .collect::<HashMap<_, _>>();

        // The original create event must be in the auth events
        if !matches!(
            auth_events.get(&(StateEventType::RoomCreate, "".to_owned())),
            Some(_) | None
        ) {
            rejection_reason = Some(format!("incoming event refers to wrong create event"));
        }

        if let Err(_e) = event_auth::auth_check(
            &auth_rules,
            &self.pdu,
            &async |event_id| {
                timeline::get_pdu(&event_id).map(|p|p.into_inner())
                    .map_err(|_| StateError::other("missing pdu 1"))
            },
            &async |k, s| {
                if let Some(pdu) = auth_events
                    .get(&(k.to_string().into(), s.to_owned()))
                {
                    return Ok(pdu.pdu.clone());
                }
                if auth_rules.room_create_event_id_as_room_id && k == StateEventType::RoomCreate {
                    let pdu = crate::room::get_create(&self.room_id)
                        .map_err(|_| StateError::other("missing create event"))?
                        .into_inner();
                    if pdu.room_id != *self.room_id {
                        Err(StateError::other("mismatched room id in create event"))
                    } else {
                        Ok(pdu.into_inner())
                    }
                } else {
                    Err(StateError::other(format!(
                        "failed auth check when process to outlier pdu, missing state event, event_type: {k}, state_key:{s}"
                    )))
                }
            },
        )
        .await
            && rejection_reason.is_none()
        {
            soft_failed = true;
            // rejection_reason = Some(e.to_string())
        };
        debug!("validation successful");

        self.soft_failed = soft_failed;
        self.rejection_reason = rejection_reason;
        self.save_without_fill_missing(known_events)
    }
}
