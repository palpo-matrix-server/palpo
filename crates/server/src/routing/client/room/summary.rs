use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use salvo::prelude::*;

use crate::core::client::room::{SummaryMsc3266ReqArgs, SummaryMsc3266ResBody};
use crate::core::events::room::member::MembershipState;
use crate::core::federation::space::HierarchyReqArgs;
use crate::core::federation::space::HierarchyResBody;
use crate::core::federation::space::{SpaceHierarchyParentSummary, hierarchy_request};
use crate::core::identifiers::*;
use crate::core::space::SpaceRoomJoinRule;
use crate::room::state;
use crate::routing::prelude::*;
use crate::{GetUrlOrigin, config, data, sending};

/// # `GET /_matrix/client/unstable/im.nheko.summary/summary/{roomIdOrAlias}`
///
/// Returns a short description of the state of a room.
///
/// An implementation of [MSC3266](https://github.com/matrix-org/matrix-spec-proposals/pull/3266)
#[handler]
pub async fn get_summary_msc_3266(
    _aa: AuthArgs,
    args: SummaryMsc3266ReqArgs,
    depot: &mut Depot,
) -> JsonResult<SummaryMsc3266ResBody> {
    let authed = depot.authed_info().ok();
    let sender_id = authed.map(|a| &**a.user_id());

    let (room_id, servers) =
        crate::room::alias::resolve_with_servers(&args.room_id_or_alias, Some(args.via.clone())).await?;

    if data::room::is_disabled(&room_id)? {
        return Err(MatrixError::forbidden(None, "This room is banned on this homeserver.").into());
    }

    if crate::room::is_server_in_room(config::server_name(), &room_id)? {
        let res_body = local_room_summary(&room_id, sender_id).await?;
        json_ok(res_body)
    } else {
        let room = remote_room_summary_hierarchy(&room_id, &servers, sender_id).await?;

        json_ok(SummaryMsc3266ResBody {
            room_id: room_id.to_owned(),
            canonical_alias: room.canonical_alias,
            avatar_url: room.avatar_url,
            guest_can_join: room.guest_can_join,
            name: room.name,
            num_joined_members: room.num_joined_members,
            topic: room.topic,
            world_readable: room.world_readable,
            join_rule: room.join_rule,
            room_type: room.room_type,
            room_version: room.room_version,
            encryption: room.encryption,
            allowed_room_ids: room.allowed_room_ids,
            membership: sender_id.is_some().then_some(MembershipState::Leave),
        })
    }
}

async fn local_room_summary(room_id: &RoomId, sender_id: Option<&UserId>) -> AppResult<SummaryMsc3266ResBody> {
    trace!(?sender_id, "Sending local room summary response for {room_id:?}");
    let join_rule = state::get_join_rule(room_id)?;
    let world_readable = state::is_world_readable(room_id)?;
    let guest_can_join = state::guest_can_join(room_id)?;

    trace!("{join_rule:?}, {world_readable:?}, {guest_can_join:?}");

    require_user_can_see_summary(
        room_id,
        &join_rule.clone().into(),
        guest_can_join,
        world_readable,
        join_rule.allowed_rooms(),
        sender_id,
    )
    .await?;

    let canonical_alias = state::get_canonical_alias(room_id).ok().flatten();

    let name = state::get_name(room_id).ok();

    let topic = state::get_room_topic(room_id).ok();

    let room_type = state::get_room_type(room_id).ok().flatten();

    let avatar_url = state::get_avatar_url(room_id).ok().flatten();

    let room_version = state::get_room_version(room_id).ok();

    let encryption = state::get_room_encryption(room_id).ok();

    let num_joined_members = crate::room::joined_member_count(room_id).unwrap_or(0);

    let membership = sender_id
        .map(|sender_id| {
            state::get_member(room_id, sender_id).map_or(MembershipState::Leave, |content| content.membership)
        })
        .into();

    Ok(SummaryMsc3266ResBody {
        room_id: room_id.to_owned(),
        canonical_alias,
        avatar_url,
        guest_can_join,
        name,
        num_joined_members: num_joined_members.try_into().unwrap_or_default(),
        topic,
        world_readable,
        room_type,
        room_version,
        encryption,
        membership,
        allowed_room_ids: join_rule.allowed_rooms().map(Into::into).collect(),
        join_rule: join_rule.into(),
    })
}

/// used by MSC3266 to fetch a room's info if we do not know about it
async fn remote_room_summary_hierarchy(
    room_id: &RoomId,
    servers: &[OwnedServerName],
    sender_id: Option<&UserId>,
) -> AppResult<SpaceHierarchyParentSummary> {
    trace!(
        ?sender_id,
        ?servers,
        "Sending remote room summary response for {room_id:?}"
    );
    let conf = crate::config();
    if !conf.allow_federation {
        return Err(MatrixError::forbidden(None, "Federation is disabled.").into());
    }

    if crate::room::is_disabled(room_id)? {
        return Err(MatrixError::forbidden(
            None,
            "Federaton of room {room_id} is currently disabled on this server.",
        )
        .into());
    }

    let mut requests: FuturesUnordered<_> = FuturesUnordered::new();
    for server in servers {
        let Ok(request) = hierarchy_request(
            &server.origin().await,
            HierarchyReqArgs {
                room_id: room_id.to_owned(),
                suggested_only: false,
            },
        ) else {
            continue;
        };
        requests.push(sending::send_federation_request(server, request.into_inner()));
    }

    while let Some(Ok(response)) = requests.next().await {
        trace!("{response:?}");
        let Ok(res_body) = response.json::<HierarchyResBody>().await else {
            continue;
        };
        if res_body.room.room_id != room_id {
            tracing::warn!(
                "Room ID {} returned does not belong to the requested room ID {}",
                res_body.room.room_id,
                room_id
            );
            continue;
        }

        return require_user_can_see_summary(
            room_id,
            &res_body.room.join_rule,
            res_body.room.guest_can_join,
            res_body.room.world_readable,
            res_body.room.allowed_room_ids.iter().map(|r| &**r),
            sender_id,
        )
        .await
        .map(|()| res_body.room);
    }

    Err(MatrixError::not_found(
        "Room is unknown to this server and was unable to fetch over federation with the \
		 provided servers available",
    )
    .into())
}

async fn require_user_can_see_summary<'a, I>(
    room_id: &RoomId,
    join_rule: &SpaceRoomJoinRule,
    guest_can_join: bool,
    world_readable: bool,
    mut allowed_room_ids: I,
    sender_id: Option<&UserId>,
) -> AppResult<()>
where
    I: Iterator<Item = &'a RoomId> + Send,
{
    let is_public_room = matches!(
        join_rule,
        SpaceRoomJoinRule::Public | SpaceRoomJoinRule::Knock | SpaceRoomJoinRule::KnockRestricted
    );
    match sender_id {
        Some(sender_id) => {
            let user_can_see_state_events = state::user_can_see_state_events(sender_id, room_id)?;
            let is_guest = data::user::is_deactivated(sender_id).unwrap_or(false);
            let user_in_allowed_restricted_room =
                allowed_room_ids.any(|room| crate::room::user::is_joined(sender_id, room).unwrap_or(false));

            if user_can_see_state_events
                || (is_guest && guest_can_join)
                || is_public_room
                || user_in_allowed_restricted_room
            {
                return Ok(());
            }

            Err(MatrixError::forbidden(
                None,
                "Room is not world readable, not publicly accessible/joinable, restricted room \
				 conditions not met, and guest access is forbidden. Not allowed to see details \
				 of this room.",
            )
            .into())
        }
        None => {
            if is_public_room || world_readable {
                return Ok(());
            }

            Err(MatrixError::forbidden(
                None,
                "Room is not world readable or publicly accessible/joinable, authentication is \
				 required",
            )
            .into())
        }
    }
}
