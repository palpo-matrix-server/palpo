use std::ops::{Deref, DerefMut};

use diesel::prelude::*;

use crate::core::events::TimelineEventType;
use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, RawJsonValue};
use crate::core::state::{Event, StateError};
use crate::core::{self, Seqnum, UnixMillis};
use crate::data::room::{DbEventData, NewDbEvent};
use crate::data::{connect, diesel_exists, schema::*};
use crate::event::fetching::{
    fetch_and_process_auth_chain, fetch_and_process_missing_events,
    fetch_and_process_missing_state_by_ids,
};
use crate::event::handler::auth_check;
use crate::event::{PduEvent, SnPduEvent, ensure_event_sn};
use crate::room::state::update_backward_extremities;
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
    pub room_version: RoomVersionId,
    pub event_sn: Option<Seqnum>,
    pub rejected_auth_events: Vec<OwnedEventId>,
    pub rejected_prev_events: Vec<OwnedEventId>,
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
    pub fn save_to_database(
        self,
        backfilled: bool,
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
                    backfilled,
                },
                json_data,
                None,
            ));
        }
        let (event_sn, event_guard) = ensure_event_sn(&room_id, &pdu.event_id)?;
        let mut db_event =
            NewDbEvent::from_canonical_json(&pdu.event_id, event_sn, &json_data, backfilled)?;
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
        let pdu = SnPduEvent {
            pdu,
            event_sn,
            is_outlier: true,
            soft_failed,
            backfilled,
        };
        update_backward_extremities(&pdu)?;
        Ok((pdu, json_data, event_guard))
    }

    pub async fn process_incoming(
        mut self,
        backfilled: bool,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        print!(
            "=============process_incoming_pdu 0 {backfilled} {:#?}",
            self.pdu
        );
        if (!self.soft_failed && !self.rejected())
            || (self.rejected()
                && self.rejected_prev_events.is_empty()
                && self.rejected_auth_events.is_empty())
        {
            print!("=============process_incoming_pdu 1 {backfilled}");
            return self.save_to_database(backfilled);
        }

        // Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
        if let Err(e) = fetch_and_process_missing_events(
            &self.remote_server,
            &self.room_id,
            &self.room_version,
            &self,
        )
        .await
        {
            print!("=============process_incoming_pdu 2 {backfilled}");
            if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
                if *kind == core::error::ErrorKind::BadJson {
                    self.rejection_reason = Some(format!("bad prev events: {}", e));
                    return self.save_to_database(backfilled);
                } else {
                    self.soft_failed = true;
                }
            } else {
                self.soft_failed = true;
            }
        }

        print!("=============process_incoming_pdu 3 {backfilled}");
        self.process_pulled(backfilled).await
    }

    fn any_auth_event_rejected(&self) -> AppResult<bool> {
        let query = events::table
            .filter(events::id.eq_any(&self.pdu.auth_events))
            .filter(events::is_rejected.eq(true));
        Ok(diesel_exists!(query, &mut connect()?)?)
    }
    fn any_prev_event_rejected(&self) -> AppResult<bool> {
        let query = events::table
            .filter(events::id.eq_any(&self.pdu.prev_events))
            .filter(events::is_rejected.eq(true));
        Ok(diesel_exists!(query, &mut connect()?)?)
    }

    pub async fn process_pulled(
        mut self,
        backfilled: bool,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        let version_rules = crate::room::get_version_rules(&self.room_version)?;

        print!("=============process_pulled 0 {backfilled}");
        if !self.soft_failed || self.rejected() {
            print!("=============process_pulled 1 {backfilled}");
            return self.save_to_database(backfilled);
        }

        if self.any_prev_event_rejected()? {
            print!("=============process_pulled 2 {backfilled}");
            self.rejection_reason = Some("one or more prev events are rejected".to_string());
            return self.save_to_database(backfilled);
        }
        if self.any_auth_event_rejected()?
            && let Err(e) = fetch_and_process_auth_chain(
                &self.remote_server,
                &self.room_id,
                &self.room_version,
                &self.pdu.event_id,
            )
            .await
        {
            print!("=============process_pulled 3 {backfilled}");
            if let AppError::HttpStatus(_) = e {
                self.soft_failed = true;
            } else {
                self.rejection_reason = Some("one or more auth events are rejected".to_string());
            }
            return self.save_to_database(backfilled);
        }
        let (_prev_events, missing_prev_event_ids) =
            timeline::get_may_missing_pdus(&self.room_id, &self.pdu.prev_events)?;
        if !missing_prev_event_ids.is_empty() {
            for event_id in &missing_prev_event_ids {
                let missing_events = match fetch_and_process_missing_state_by_ids(
                    &self.remote_server,
                    &self.room_id,
                    &self.room_version,
                    event_id,
                )
                .await
                {
                    Ok(missing_events) => {
                        self.soft_failed = !missing_events.is_empty();
                        missing_events
                    }
                    Err(e) => {
                        if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
                            if *kind == core::error::ErrorKind::BadJson {
                                self.rejection_reason =
                                    Some(format!("failed to bad prev events: {}", e));
                            } else {
                                self.soft_failed = true;
                            }
                        } else {
                            self.soft_failed = true;
                        }
                        vec![]
                    }
                };
                if !missing_events.is_empty() {
                    for event_id in &missing_events {
                        if let Err(e) = fetch_and_process_auth_chain(
                            &self.remote_server,
                            &self.room_id,
                            &self.room_version,
                            event_id,
                        )
                        .await
                        {
                            warn!("error fetching auth chain for {}: {}", event_id, e);
                        }
                    }
                }
            }
        }

        print!("=============process_pulled 4 {backfilled}");
        if self.pdu.rejection_reason.is_none() {
            if let Err(e) = auth_check(&self.pdu, &self.room_id, &version_rules, None).await {
                match e {
                    AppError::State(StateError::Forbidden(brief)) => {
                        self.pdu.rejection_reason = Some(brief);
                    }
                    _ => {
                        self.soft_failed = true;
                    }
                }
            } else {
                self.soft_failed = false;
            }
        }
        print!("=============process_pulled 5 {backfilled}");
        self.save_to_database(backfilled)
    }
}
