use salvo::http::header::AUTHORIZATION;
use salvo::http::headers::authorization::Credentials;

use crate::core::authorization::XMatrix;
use crate::core::{signatures, MatrixError, ServerName};
use crate::{AppError, AppResult};

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
