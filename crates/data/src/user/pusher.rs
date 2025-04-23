use std::fmt::Debug;

use diesel::prelude::*;
use palpo_core::push::PusherIds;

use crate::core::UnixMillis;
use crate::core::events::AnySyncTimelineEvent;
use crate::core::events::room::power_levels::RoomPowerLevelsEventContent;
use crate::core::identifiers::*;
use crate::core::push::{Action, PushConditionPowerLevelsCtx, PushConditionRoomCtx, Pusher, PusherKind, Ruleset};
use crate::core::serde::{JsonValue, RawJson};
use crate::schema::*;
use crate::{DataError, DataResult, connect};

#[derive(Identifiable, Queryable, Debug, Clone)]
#[diesel(table_name = pushers)]
pub struct DbPusher {
    pub id: i64,

    pub user_id: OwnedUserId,
    pub kind: String,
    pub app_id: String,
    pub app_display_name: String,
    pub device_id: OwnedDeviceId,
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
#[diesel(table_name = pushers)]
pub struct NewDbPusher {
    pub user_id: OwnedUserId,
    pub kind: String,
    pub app_id: String,
    pub app_display_name: String,
    pub device_id: OwnedDeviceId,
    pub device_display_name: String,
    pub access_token_id: Option<i64>,
    pub profile_tag: Option<String>,
    pub pushkey: String,
    pub lang: String,
    pub data: JsonValue,
    pub enabled: bool,
    pub created_at: UnixMillis,
}
impl TryInto<Pusher> for DbPusher {
    type Error = DataError;
    fn try_into(self) -> DataResult<Pusher> {
        let Self {
            user_id,
            profile_tag,
            kind,
            app_id,
            app_display_name,
            device_display_name,
            pushkey,
            lang,
            data,
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

pub fn get_pusher(user_id: &UserId, pushkey: &str) -> DataResult<Option<Pusher>> {
    let pusher = pushers::table
        .filter(pushers::user_id.eq(user_id))
        .filter(pushers::pushkey.eq(pushkey))
        .order_by(pushers::id.desc())
        .first::<DbPusher>(&mut connect()?)
        .optional()?;
    if let Some(pusher) = pusher {
        pusher.try_into().map(Option::Some)
    } else {
        Ok(None)
    }
}

pub fn get_pushers(user_id: &UserId) -> DataResult<Vec<DbPusher>> {
    pushers::table
        .filter(pushers::user_id.eq(user_id))
        .order_by(pushers::id.desc())
        .load::<DbPusher>(&mut connect()?)
        .map_err(Into::into)
}

pub fn get_actions<'a>(
    user: &UserId,
    ruleset: &'a Ruleset,
    power_levels: &RoomPowerLevelsEventContent,
    pdu: &RawJson<AnySyncTimelineEvent>,
    room_id: &RoomId,
) -> DataResult<&'a [Action]> {
    let power_levels = PushConditionPowerLevelsCtx {
        users: power_levels.users.clone(),
        users_default: power_levels.users_default,
        notifications: power_levels.notifications.clone(),
    };
    let ctx = PushConditionRoomCtx {
        room_id: room_id.to_owned(),
        member_count: 10_u32.into(), // TODO: get member count efficiently
        user_id: user.to_owned(),
        user_display_name: crate::user::display_name(user)
            .ok()
            .flatten()
            .unwrap_or_else(|| user.localpart().to_owned()),
        power_levels: Some(power_levels),
        supported_features: vec![],
    };

    Ok(ruleset.get_actions(pdu, &ctx))
}

pub fn get_push_keys(user_id: &UserId) -> DataResult<Vec<String>> {
    pushers::table
        .filter(pushers::user_id.eq(user_id))
        .select(pushers::pushkey)
        .load::<String>(&mut connect()?)
        .map_err(Into::into)
}
