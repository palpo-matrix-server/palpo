#![cfg(feature = "client")]

use http::HeaderMap;
use palpo_core::client::discovery::discover_homeserver;
use crate::api::{MatrixVersion, OutgoingRequest as _, SendAccessToken};

#[test]
fn get_request_headers() {
    let req: http::Request<Vec<u8>> = discover_homeserver::Request::new()
        .try_into_http_request(
            "https://homeserver.tld",
            SendAccessToken::None,
            &[MatrixVersion::V1_1],
        )
        .unwrap();

    assert_eq!(*req.headers(), HeaderMap::default());
}
