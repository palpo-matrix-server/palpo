//! Endpoint for pinging the application service.

//! `PUT /_matrix/app/*/ping`
//!
//! Endpoint to ping the application service.
//! `/v1/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/application-service-api/#post_matrixappv1ping
use salvo::oapi::ToSchema;
use serde::Deserialize;

use crate::OwnedTransactionId;

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: false,
//     authentication: AccessToken,
//     history: {
//         unstable => "/_matrix/app/unstable/fi.mau.msc2659/ping",
//         1.7 => "/_matrix/app/v1/ping",
//     }
// };

/// Request type for the `send_ping` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SendPingReqBody {
    /// A transaction ID for the ping, copied directly from the `POST
    /// /_matrix/client/v1/appservice/{appserviceId}/ping` call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<OwnedTransactionId>,
}
