use diesel::prelude::*;
use rand::seq::SliceRandom;
use serde_json::value::to_raw_value;

use crate::core::UnixMillis;
use crate::core::client::room::AliasResBody;
use crate::core::events::TimelineEventType;
use crate::core::events::room::canonical_alias::RoomCanonicalAliasEventContent;
use crate::core::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use crate::core::federation::query::RoomInfoResBody;
use crate::core::federation::query::directory_request;
use crate::core::identifiers::*;
use crate::room::StateEventType;
use crate::user::DbUser;
use crate::{AppError, AppResult, MatrixError, PduBuilder, db};
use crate::{GetUrlOrigin, schema::*};

pub(super) async fn remote_resolve(
    room_alias: &RoomAliasId,
    servers: Vec<OwnedServerName>,
) -> AppResult<(OwnedRoomId, Vec<OwnedServerName>)> {
    debug!(?room_alias, servers = ?servers, "remote resolve");
    let servers = [vec![room_alias.server_name().to_owned()], servers].concat();

    let mut resolved_servers = Vec::new();
    let mut resolved_room_id: Option<OwnedRoomId> = None;
    for server in servers {
        match remote_request(room_alias, &server).await {
            Err(e) => tracing::error!("Failed to query for {room_alias:?} from {server}: {e}"),
            Ok(RoomInfoResBody { room_id, servers }) => {
                debug!(
                    "Server {server} answered with {room_id:?} for {room_alias:?} servers: \
					 {servers:?}"
                );

                resolved_room_id.get_or_insert(room_id);
                add_server(&mut resolved_servers, server);

                if !servers.is_empty() {
                    add_servers(&mut resolved_servers, servers);
                    break;
                }
            }
        }
    }

    resolved_room_id
        .map(|room_id| (room_id, resolved_servers))
        .ok_or_else(|| MatrixError::not_found("No servers could assist in resolving the room alias").into())
}

async fn remote_request(room_alias: &RoomAliasId, server: &ServerName) -> AppResult<RoomInfoResBody> {
    let request = directory_request(&server.origin().await, room_alias)?.into_inner();
    crate::sending::send_federation_request(server, request)
        .await?
        .json::<RoomInfoResBody>()
        .await
        .map_err(Into::into)
}

fn add_servers(servers: &mut Vec<OwnedServerName>, new: Vec<OwnedServerName>) {
    for server in new {
        add_server(servers, server);
    }
}

fn add_server(servers: &mut Vec<OwnedServerName>, server: OwnedServerName) {
    if !servers.contains(&server) {
        servers.push(server);
    }
}
