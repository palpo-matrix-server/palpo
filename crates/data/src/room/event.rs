use std::collections::BTreeMap;

use diesel::prelude::*;

use crate::core::events::AnySyncEphemeralRoomEvent;
use crate::core::events::receipt::{Receipt, ReceiptEvent, ReceiptEventContent, ReceiptType};
use crate::core::identifiers::*;
use crate::core::serde::{JsonValue, RawJson};
use crate::core::{Seqnum, UnixMillis};
use crate::room::{DbReceipt, NewDbEventPushAction, NewDbReceipt};
use crate::schema::*;
use crate::{DataResult, connect, next_sn};

#[tracing::instrument]
pub fn upsert_push_action(action: &NewDbEventPushAction) -> DataResult<()> {
    diesel::insert_into(event_push_actions::table)
        .values(action)
        .on_conflict_do_nothing()
        .execute(&mut connect()?)?;
    Ok(())
}
