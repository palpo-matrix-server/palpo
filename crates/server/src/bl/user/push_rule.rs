use std::{fmt::Debug, mem};

use bytes::BytesMut;
use diesel::prelude::*;
use palpo_core::push::PusherIds;
use tracing::{info, warn};

use crate::core::client::push::pusher::PusherAction;
use crate::core::client::push::PusherPostData;
use crate::core::identifiers::*;
use crate::core::{
    client::push::{Device, Notification, NotificationCounts, NotificationPriority},
    events::{
        room::power_levels::RoomPowerLevelsEventContent, AnySyncTimelineEvent, StateEventType, TimelineEventType,
    },
    push::{self, Pusher, PusherKind},
    push::{Action, PushConditionRoomCtx, PushFormat, Ruleset, Tweak},
    serde::RawJson,
    MatrixVersion, SendAccessToken,
};
use crate::pdu::PduEvent;
use crate::schema::*;
use crate::{db, AppError, AppResult, JsonValue, MatrixError, BAD_QUERY_RATE_LIMITER};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = user_push_rules)]
pub struct DbPushRule {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub kind: String,
    pub app_id: String,
    pub app_display_name: String,
    pub device_display_name: String,
    pub access_token_id: Option<i64>,
    pub profile_tag: Option<String>,
    pub pushkey: String,
    pub lang: String,
    pub data: JsonValue,
    pub enabled: bool,
    pub last_stream_ordering: Option<i64>,
    pub last_success: Option<i64>,
    pub failing_since: Option<i64>,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Debug, Clone)]
#[diesel(table_name = user_push_rules)]
pub struct NewDbPushRule {
    pub user_id: OwnedUserId,
    pub kind: String,
    pub app_id: String,
    pub app_display_name: String,
    pub device_display_name: String,
    pub access_token_id: Option<i64>,
    pub profile_tag: Option<String>,
    pub pushkey: String,
    pub lang: String,
    pub data: JsonValue,
    pub enabled: bool,
}
impl TryInto<PushRule> for DbPushRule {
    type Error = AppError;
    fn try_into(self) -> AppResult<Pusher> {
        let Self {
            user_id,
            access_token_id,
            profile_tag,
            kind,
            app_id,
            app_display_name,
            device_display_name,
            pushkey,
            lang,
            data,
            enabled,
            last_stream_ordering,
            last_success,
            failing_since,
            created_at,
            ..
        } = self;
        Ok(Pusher {
            ids: PusherIds { app_id, pushkey },
            profile_tag,
            kind: PusherKind::try_new(&kind, data)?,
            app_display_name,
            device_display_name,
            lang,
        })
    }
}

pub fn set_pusher(user_id: &UserId, pusher: PusherAction) -> AppResult<()> {
    match pusher {
        PusherAction::Post(data) => {
            let PusherPostData {
                pusher:
                    Pusher {
                        ids: PusherIds { app_id, pushkey },
                        kind,
                        app_display_name,
                        device_display_name,
                        lang,
                        profile_tag,
                        ..
                    },
                append,
            } = data;
            if !append {
                diesel::delete(
                    user_pushers::table
                        .filter(user_pushers::user_id.eq(&user_id))
                        .filter(user_pushers::pushkey.eq(&pushkey))
                        .filter(user_pushers::app_id.eq(&app_id)),
                )
                .execute(&mut db::connect()?)?;
            }
            diesel::insert_into(user_pushers::table)
                .values(&NewDbPusher {
                    user_id: user_id.to_owned(),
                    access_token_id: None, //TODO
                    profile_tag,
                    kind: kind.name().to_owned(),
                    app_id,
                    app_display_name,
                    device_display_name,
                    pushkey,
                    lang,
                    data: kind.json_data()?,
                    enabled: true, // TODO
                })
                .execute(&mut db::connect()?)?;
        }
        PusherAction::Delete((ids)) => {
            diesel::delete(
                user_pushers::table
                    .filter(user_pushers::user_id.eq(user_id))
                    .filter(user_pushers::pushkey.eq(ids.pushkey))
                    .filter(user_pushers::app_id.eq(ids.app_id)),
            )
            .execute(&mut db::connect()?)?;
        }
    }
    Ok(())
}

pub fn get_pusher(user_id: &UserId, pushkey: &str) -> AppResult<Option<DbPusher>> {
    user_pushers::table
        .filter(user_pushers::user_id.eq(user_id))
        .filter(user_pushers::pushkey.eq(pushkey))
        .order_by(user_pushers::id.desc())
        .first::<DbPusher>(&mut *db::connect()?)
        .optional()
        .map_err(Into::into)
}

pub fn get_pushers(user_id: &UserId) -> AppResult<Vec<DbPusher>> {
    user_pushers::table
        .filter(user_pushers::user_id.eq(user_id))
        .order_by(user_pushers::id.desc())
        .load::<DbPusher>(&mut *db::connect()?)
        .map_err(Into::into)
}
