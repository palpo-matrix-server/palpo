use salvo::http::header::AUTHORIZATION;
use salvo::http::headers::authorization::Credentials;

use crate::core::authorization::XMatrix;
use crate::core::events::StateEventType;
use crate::core::events::room::join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent};
use crate::core::identifiers::*;
use crate::core::serde::CanonicalJsonObject;
use crate::core::{MatrixError, signatures};
use crate::{AppError, AppResult};

mod access_check;
pub use access_check::access_check;

#[tracing::instrument(skip(request))]
pub(crate) async fn send_request(
    destination: &ServerName,
    mut request: reqwest::Request,
) -> AppResult<reqwest::Response> {
    if !crate::config().allow_federation {
        return Err(AppError::public("Federation is disabled."));
    }

    if destination == crate::server_name() {
        return Err(AppError::public("Won't send federation request to ourselves"));
    }

    debug!("Preparing to send request to {destination}");
    let mut request_map = serde_json::Map::new();

    if let Some(body) = request.body() {
        request_map.insert(
            "content".to_owned(),
            serde_json::from_slice(body.as_bytes().unwrap_or_default())
                .expect("body is valid json, we just created it"),
        );
    };

    request_map.insert("method".to_owned(), request.method().to_string().into());
    request_map.insert(
        "uri".to_owned(),
        format!(
            "{}{}",
            request.url().path(),
            request.url().query().map(|q| format!("?{q}")).unwrap_or_default()
        )
        .into(),
    );
    request_map.insert("origin".to_owned(), crate::server_name().as_str().into());
    request_map.insert("destination".to_owned(), destination.as_str().into());

    let mut request_json = serde_json::from_value(request_map.into()).expect("valid JSON is valid BTreeMap");

    signatures::sign_json(crate::server_name().as_str(), crate::keypair(), &mut request_json)
        .expect("our request json is what ruma expects");

    let request_json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&serde_json::to_vec(&request_json).unwrap()).unwrap();

    let signatures = request_json["signatures"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_object().unwrap().iter().map(|(k, v)| (k, v.as_str().unwrap())));

    for signature_server in signatures {
        for s in signature_server {
            request.headers_mut().insert(
                AUTHORIZATION,
                XMatrix::parse(&format!(
                    "X-Matrix origin=\"{}\",destination=\"{}\",key=\"{}\",sig=\"{}\"",
                    crate::server_name(),
                    destination,
                    s.0,
                    s.1
                ))
                .expect("When signs JSON, it produces a valid base64 signature. All other types are valid ServerNames or OwnedKeyId")
                .encode(),
            );
        }
    }

    let url = request.url().clone();

    debug!("Sending request to {destination} at {url}");
    // if url.to_string().contains("federation/v1/event/") {
    //     panic!("sdddddddddddddddddd");
    // }
    let response = crate::federation_client().execute(request).await;

    match response {
        Ok(response) => {
            let status = response.status();

            if status == 200 {
                Ok(response)
            } else {
                let body = response.text().await.unwrap_or_default();
                warn!("{} {}: {}", url, status, body);
                let err_msg = format!("Answer from {destination}: {body}");
                debug!("Returning error from {destination}");
                Err(MatrixError::unknown(err_msg).into())
            }
        }
        Err(e) => {
            warn!("Could not send request to {} at {}: {}", destination, url, e);
            Err(e.into())
        }
    }
}

/// Checks whether the given user can join the given room via a restricted join.
pub(crate) async fn user_can_perform_restricted_join(
    user_id: &UserId,
    room_id: &RoomId,
    room_version_id: &RoomVersionId,
) -> AppResult<bool> {
    use RoomVersionId::*;

    // restricted rooms are not supported on <=v7
    if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
        return Ok(false);
    }

    if crate::room::is_joined(user_id, room_id).unwrap_or(false) {
        // joining user is already joined, there is nothing we need to do
        return Ok(false);
    }

    let Ok(join_rules_event_content) = crate::room::state::get_room_state_content::<RoomJoinRulesEventContent>(
        room_id,
        &StateEventType::RoomJoinRules,
        "",
    ) else {
        return Ok(false);
    };

    let (JoinRule::Restricted(r) | JoinRule::KnockRestricted(r)) = join_rules_event_content.join_rule else {
        return Ok(false);
    };

    if r.allow.is_empty() {
        tracing::info!("{room_id} is restricted but the allow key is empty");
        return Ok(false);
    }

    if r.allow
        .iter()
        .filter_map(|rule| {
            if let AllowRule::RoomMembership(membership) = rule {
                Some(membership)
            } else {
                None
            }
        })
        .any(|m| crate::room::is_joined(user_id, &m.room_id).unwrap_or(false))
    {
        Ok(true)
    } else {
        Err(MatrixError::unable_to_authorize_join("Joining user is not known to be in any required room.").into())
    }
}

pub(crate) fn maybe_strip_event_id(pdu_json: &mut CanonicalJsonObject, room_version_id: &RoomVersionId) -> bool {
    match room_version_id {
        RoomVersionId::V1 | RoomVersionId::V2 => false,
        _ => {
            pdu_json.remove("event_id");
            true
        }
    }
}
