use std::{collections::BTreeMap, iter::FromIterator, str};

use diesel::prelude::*;
use palpo_core::UnixMillis;
use salvo::http::{
    headers::{
        authorization::{Authorization, Credentials},
        HeaderMapExt,
    },
    HeaderValue,
};
use salvo::prelude::*;

use crate::core::serde::CanonicalJsonValue;
use crate::core::{signatures, OwnedServerName};
use crate::schema::*;
use crate::user::{DbAccessToken, DbUser, DbUserDevice};
use crate::{db, AppResult, AuthArgs, AuthedInfo, MatrixError};

#[handler]
pub async fn auth_by_access_token_or_signatures(aa: AuthArgs, req: &mut Request, depot: &mut Depot) -> AppResult<()> {
    if let Some(authorization) = &aa.authorization {
        if authorization.starts_with("Bearer ") {
            auth_by_access_token_inner(aa, depot).await
        } else {
            auth_by_signatures_inner(req, depot).await
        }
    } else {
        Err(MatrixError::missing_token("Missing token.").into())
    }
}

#[handler]
pub async fn auth_by_access_token(aa: AuthArgs, depot: &mut Depot) -> AppResult<()> {
    auth_by_access_token_inner(aa, depot).await
}
#[handler]
pub async fn auth_by_signatures(_aa: AuthArgs, req: &mut Request, depot: &mut Depot) -> AppResult<()> {
    auth_by_signatures_inner(req, depot).await
}

async fn auth_by_access_token_inner(aa: AuthArgs, depot: &mut Depot) -> AppResult<()> {
    let token = aa.require_access_token()?;

    let access_token = user_access_tokens::table
        .filter(user_access_tokens::token.eq(token))
        .first::<DbAccessToken>(&mut *db::connect()?)
        .ok();
    if let Some(access_token) = access_token {
        let user = users::table
            .find(&access_token.user_id)
            .first::<DbUser>(&mut *db::connect()?)
            .map_err(|_| MatrixError::unknown_token(true, "User not found"))?;
        let user_device = user_devices::table
            .filter(user_devices::device_id.eq(&access_token.device_id))
            .filter(user_devices::user_id.eq(&user.id))
            .first::<DbUserDevice>(&mut *db::connect()?)
            .map_err(|_| MatrixError::unknown_token(true, "User device not found"))?;

        depot.inject(AuthedInfo {
            user,
            user_device,
            access_token_id: Some(access_token.id),
            appservice: None,
        });
        Ok(())
    } else {
        Err(MatrixError::unknown_token(true, "Unknown access token").into())
    }
}

async fn auth_by_signatures_inner(req: &mut Request, depot: &mut Depot) -> AppResult<()> {
    let Some(Authorization(x_matrix)) = req.headers().typed_get::<Authorization<XMatrix>>() else {
        warn!("Missing or invalid Authorization header");
        return Err(MatrixError::forbidden("Missing or invalid authorization header").into());
    };

    let origin_signatures = BTreeMap::from_iter([(x_matrix.key.clone(), CanonicalJsonValue::String(x_matrix.sig))]);

    let signatures = BTreeMap::from_iter([(
        x_matrix.origin.as_str().to_owned(),
        CanonicalJsonValue::Object(origin_signatures),
    )]);

    let mut request_map = BTreeMap::from_iter([
        (
            "destination".to_owned(),
            CanonicalJsonValue::String(crate::server_name().as_str().to_owned()),
        ),
        (
            "method".to_owned(),
            CanonicalJsonValue::String(req.method().to_string()),
        ),
        (
            "origin".to_owned(),
            CanonicalJsonValue::String(x_matrix.origin.as_str().to_owned()),
        ),
        (
            "uri".to_owned(),
            format!(
                "{}{}",
                req.uri().path(),
                req.uri().query().map(|q| format!("?{q}")).unwrap_or_default()
            )
            .into(),
        ),
        ("signatures".to_owned(), CanonicalJsonValue::Object(signatures)),
    ]);

    let json_body = req
        .payload()
        .await
        .ok()
        .and_then(|payload| serde_json::from_slice::<CanonicalJsonValue>(payload).ok());

    if let Some(json_body) = &json_body {
        request_map.insert("content".to_owned(), json_body.clone());
    };

    let keys_result = crate::event::handler::fetch_signing_keys(&x_matrix.origin, vec![x_matrix.key.to_owned()]).await;

    let keys = match keys_result {
        Ok(b) => b,
        Err(e) => {
            warn!("Failed to fetch signing keys: {}", e);
            return Err(MatrixError::forbidden("Failed to fetch signing keys.").into());
        }
    };

    // Only verify_keys that are currently valid should be used for validating requests
    // as per MSC4029
    let pub_key_map = BTreeMap::from_iter([(
        x_matrix.origin.as_str().to_owned(),
        if keys.valid_until_ts > UnixMillis::now() {
            keys.verify_keys.into_iter().map(|(id, key)| (id, key.key)).collect()
        } else {
            BTreeMap::new()
        },
    )]);

    if let Err(e) = signatures::verify_json(&pub_key_map, &request_map) {
        warn!(
            "Failed to verify json request from {}: {}\n{:?}",
            x_matrix.origin, e, request_map
        );

        if req.uri().to_string().contains('@') {
            warn!(
                "Request uri contained '@' character. Make sure your \
                                         reverse proxy gives Palpo the raw uri (apache: use \
                                         nocanon)"
            );
        }

        Err(MatrixError::forbidden("Failed to verify X-Matrix signatures.").into())
    } else {
        Ok(())
    }
}

struct XMatrix {
    origin: OwnedServerName,
    key: String, // KeyName?
    sig: String,
}

impl Credentials for XMatrix {
    const SCHEME: &'static str = "X-Matrix";

    fn decode(value: &HeaderValue) -> Option<Self> {
        debug_assert!(
            value.as_bytes().starts_with(b"X-Matrix "),
            "HeaderValue to decode should start with \"X-Matrix ..\", received = {value:?}",
        );

        let parameters = str::from_utf8(&value.as_bytes()["X-Matrix ".len()..])
            .ok()?
            .trim_start();

        let mut origin = None;
        let mut key = None;
        let mut sig = None;

        for entry in parameters.split_terminator(',') {
            let (name, value) = entry.split_once('=')?;

            // It's not at all clear why some fields are quoted and others not in the spec,
            // let's simply accept either form for every field.
            let value = value
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .unwrap_or(value);

            // FIXME: Catch multiple fields of the same name
            match name {
                "origin" => origin = Some(value.try_into().ok()?),
                "key" => key = Some(value.to_owned()),
                "sig" => sig = Some(value.to_owned()),
                _ => debug!("Unexpected field `{}` in X-Matrix Authorization header", name),
            }
        }

        Some(Self {
            origin: origin?,
            key: key?,
            sig: sig?,
        })
    }

    fn encode(&self) -> HeaderValue {
        todo!()
    }
}
