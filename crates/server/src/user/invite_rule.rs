use diesel::prelude::*;
use palpo_core::push::PusherIds;
use url::Url;

use crate::core::UnixMillis;
use crate::core::client::push::{PusherAction, PusherPostData};
use crate::core::events::TimelineEventType;
use crate::core::identifiers::*;
use crate::core::push::push_gateway::{
    Device, Notification, NotificationCounts, NotificationPriority, SendEventNotificationReqBody,
};
use crate::core::push::{Action, PushFormat, Pusher, PusherKind, Ruleset, Tweak};
use crate::data::connect;
use crate::data::schema::*;
use crate::data::user::pusher::NewDbPusher;
use crate::event::PduEvent;
use crate::{AppError, AppResult, AuthedInfo, data, room};

pub fn get_invte_rule(user_id: &UserId) -> AppResult<InviteRule> {
    Ok(())
}
