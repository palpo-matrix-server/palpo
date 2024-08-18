use std::collections::BTreeMap;
use std::sync::LazyLock;

use tokio::sync::{broadcast, RwLock};

use crate::core::events::typing::TypingEventContent;
use crate::core::events::SyncEphemeralRoomEvent;
use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::{AppError, AppResult};

pub static TYPING: LazyLock<RwLock<BTreeMap<OwnedRoomId, BTreeMap<OwnedUserId, u64>>>> =
    LazyLock::new(Default::default); // u64 is unix timestamp of timeout
pub static LAST_TYPING_UPDATE: LazyLock<RwLock<BTreeMap<OwnedRoomId, i64>>> = LazyLock::new(Default::default); // timestamp of the last change to typing users
pub static TYPING_UPDATE_SENDER: LazyLock<broadcast::Sender<OwnedRoomId>> = LazyLock::new(|| broadcast::channel(100).0);

/// Sets a user as typing until the timeout timestamp is reached or roomremove_typing is
/// called.
pub async fn add_typing(user_id: &UserId, room_id: &RoomId, timeout: u64) -> AppResult<()> {
    TYPING
        .write()
        .await
        .entry(room_id.to_owned())
        .or_default()
        .insert(user_id.to_owned(), timeout);
    let event_sn = crate::next_sn()?;
    LAST_TYPING_UPDATE.write().await.insert(room_id.to_owned(), event_sn);

    let current_frame_id = if let Some(s) = crate::room::state::get_room_frame_id(room_id)? {
        s
    } else {
        error!("Room {} has no state", room_id);
        return Err(AppError::public("Room has no state"));
    };
    // // Save the state after this sync so we can send the correct state diff next sync
    // let point_id = crate::room::state::ensure_point(&room_id, &OwnedEventId::from_str(&Ulid::new().to_string())?, event_sn as i64)?;
    // crate::room::state::update_point_frame_id(point_id, current_frame_id)?;

    let _ = TYPING_UPDATE_SENDER.send(room_id.to_owned());
    Ok(())
}

/// Removes a user from typing before the timeout is reached.
pub async fn remove_typing(user_id: &UserId, room_id: &RoomId) -> AppResult<()> {
    TYPING
        .write()
        .await
        .entry(room_id.to_owned())
        .or_default()
        .remove(user_id);
    LAST_TYPING_UPDATE
        .write()
        .await
        .insert(room_id.to_owned(), crate::next_sn()?);
    let _ = TYPING_UPDATE_SENDER.send(room_id.to_owned());
    Ok(())
}

pub async fn wait_for_update(room_id: &RoomId) -> AppResult<()> {
    let mut receiver = TYPING_UPDATE_SENDER.subscribe();
    while let Ok(next) = receiver.recv().await {
        if next == room_id {
            break;
        }
    }

    Ok(())
}

/// Makes sure that typing events with old timestamps get removed.
async fn maintain_typings(room_id: &RoomId) -> AppResult<()> {
    let current_timestamp = UnixMillis::now();
    let mut removable = Vec::new();
    {
        let typing = TYPING.read().await;
        let Some(room) = typing.get(room_id) else {
            return Ok(());
        };
        for (user, timeout) in room {
            if *timeout < current_timestamp.get() {
                removable.push(user.clone());
            }
        }
        drop(typing);
    }
    if !removable.is_empty() {
        let typing = &mut TYPING.write().await;
        let room = typing.entry(room_id.to_owned()).or_default();
        for user in removable {
            room.remove(&user);
        }
        LAST_TYPING_UPDATE
            .write()
            .await
            .insert(room_id.to_owned(), crate::next_sn()?);
        let _ = TYPING_UPDATE_SENDER.send(room_id.to_owned());
    }
    Ok(())
}

/// Returns the count of the last typing update in this room.
pub async fn last_typing_update(room_id: &RoomId) -> AppResult<i64> {
    maintain_typings(room_id).await?;
    Ok(LAST_TYPING_UPDATE
        .read()
        .await
        .get(room_id)
        .copied()
        .unwrap_or_default())
}

/// Returns a new typing EDU.
pub async fn all_typings(room_id: &RoomId) -> AppResult<SyncEphemeralRoomEvent<TypingEventContent>> {
    Ok(SyncEphemeralRoomEvent {
        content: TypingEventContent {
            user_ids: TYPING
                .read()
                .await
                .get(room_id)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default(),
        },
    })
}
