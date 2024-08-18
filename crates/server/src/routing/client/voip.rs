use std::time::Duration;
use std::time::SystemTime;

use base64::Engine;
use hmac::{Hmac, Mac};
use salvo::prelude::*;
use sha1::Sha1;

use crate::core::client::voip::TurnServerResBody;
use crate::core::UnixSeconds;
use crate::{hoops, json_ok, AuthArgs, DepotExt, JsonResult};

type HmacSha1 = Hmac<Sha1>;

pub fn authed_router() -> Router {
    Router::with_path("voip/turnServer")
        .hoop(hoops::limit_rate)
        .get(turn_server)
}

// #GET /_matrix/client/r0/voip/turnServer
/// TODO: Returns information about the recommended turn server.
#[endpoint]
async fn turn_server(_aa: AuthArgs, depot: &mut Depot) -> JsonResult<TurnServerResBody> {
    let authed = depot.authed_info()?;

    let turn_secret = crate::turn_secret().clone();

    let (username, password) = if !turn_secret.is_empty() {
        let expiry = UnixSeconds::from_system_time(SystemTime::now() + Duration::from_secs(crate::turn_ttl()))
            .expect("time is valid");

        let username = format!("{}:{}", expiry.get(), authed.user_id());

        let mut mac = HmacSha1::new_from_slice(turn_secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(username.as_bytes());

        let password: String = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        (username, password)
    } else {
        (crate::turn_username().to_owned(), crate::turn_password().to_owned())
    };

    json_ok(TurnServerResBody {
        username,
        password,
        uris: crate::turn_uris().to_vec(),
        ttl: Duration::from_secs(crate::turn_ttl()),
    })
}
