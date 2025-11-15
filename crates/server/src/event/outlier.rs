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
use crate::event::fetching::{
    fetch_and_process_auth_chain, fetch_and_process_missing_prev_events,
    fetch_and_process_missing_state, fetch_and_process_missing_state_by_ids,
};
use crate::event::{PduEvent, SnPduEvent, ensure_event_sn};
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

    pub async fn process_incoming(
        mut self,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        let version_rules = crate::room::get_version_rules(&self.room_version)?;
        let auth_rules = &version_rules.authorization;

        if self.soft_failed {
            // Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
            match fetch_and_process_missing_prev_events(
                &self.remote_server,
                &self.room_id,
                &self.room_version,
                &self,
            )
            .await
            {
                Ok(failed_ids) => {
                    self.soft_failed = !failed_ids.is_empty();
                    println!(
                        "==================================soft failed 2 {}",
                        self.soft_failed
                    );
                }
                Err(e) => {
                    if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
                        println!("========================zzzz {e}");
                        if *kind == core::error::ErrorKind::BadJson {
                            self.rejection_reason =
                                Some(format!("failed to bad prev events: {}", e));
                                return self.save_to_database();
                        } else {
                            println!("==================================soft failed 3 {e}");
                            self.soft_failed = true;
                        }
                    } else {
                        println!("==================================soft failed 4  {e}");
                        self.soft_failed = true;
                    }
                }
            }
        }
        self.process_pulled().await
    }

    pub async fn process_pulled(
        mut self,
    ) -> AppResult<(SnPduEvent, CanonicalJsonObject, Option<SeqnumQueueGuard>)> {
        let version_rules = crate::room::get_version_rules(&self.room_version)?;
        let auth_rules = &version_rules.authorization;

        if self.soft_failed {
            // Fetch any missing prev events doing all checks listed here starting at 1. These are timeline events
            let missing_events = match fetch_and_process_missing_state_by_ids(
                &self.remote_server,
                &self.room_id,
                &self.room_version,
                &self.pdu.event_id,
            )
            .await
            {
                Ok(missing_events) => {
                    self.soft_failed = !missing_events.is_empty();
                    println!(
                        "==================================soft failed 2 {}",
                        self.soft_failed
                    );
                    missing_events
                }
                Err(e) => {
                    if let AppError::Matrix(MatrixError { ref kind, .. }) = e {
                        println!("========================zzzz {e}");
                        if *kind == core::error::ErrorKind::BadJson {
                            println!("LLLLL");
                            self.rejection_reason =
                                Some(format!("failed to bad prev events: {}", e));
                        } else {
                            println!("==================================soft failed 3 {e}");
                            self.soft_failed = true;
                        }
                    } else {
                        println!("==================================soft failed 4  {e}");
                        self.soft_failed = true;
                    }
                    vec![]
                }
            };

            if !missing_events.is_empty() {
                println!(
                    "=======call=====fetch_and_process_missing_state {}  {:#?}",
                    self.room_id, self.pdu
                );
                for event_id in &missing_events {
                    if let Err(e) = fetch_and_process_auth_chain(
                        &self.remote_server,
                        &self.room_id,
                        &self.room_version,
                        event_id,
                    )
                    .await
                    {
                        println!("error fetching auth chain for {}: {}", event_id, e);
                    }
                }
                // if let Err(e) = fetch_and_process_missing_state(
                //     &self.remote_server,
                //     &self.room_id,
                //     &self.room_version,
                //     &self.pdu.event_id,
                // )
                // .await
                // {
                //     error!("failed to fetch missing auth events: {}", e);
                // } else {
                //     self.soft_failed = false;
                // }
            }
            println!("xxxxxxxxxxxxxxxxxxxxdre");
        }

        self.save_to_database()
    }
}
