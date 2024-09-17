use salvo::oapi::ToParameters;
use serde::Deserialize;

use crate::appservice::RegistrationInfo;
use crate::core::identifiers::*;
use crate::core::serde::default_true;
use crate::core::{MatrixError, ServerName};
use crate::user::{DbUser, DbUserDevice};

#[derive(Clone, Debug)]
pub struct AuthedInfo {
    pub user: DbUser,
    pub user_device: DbUserDevice,
    pub access_token_id: Option<i64>,
    pub appservice: Option<RegistrationInfo>,
}
impl AuthedInfo {
    pub fn user(&self) -> &DbUser {
        &self.user
    }
    pub fn user_id(&self) -> &OwnedUserId {
        &self.user.id
    }
    pub fn device_id(&self) -> &OwnedDeviceId {
        &self.user_device.device_id
    }
    pub fn access_token_id(&self) -> Option<i64> {
        self.access_token_id
    }
    pub fn server_name(&self) -> &ServerName {
        self.user.id.server_name()
    }
    pub fn is_admin(&self) -> bool {
        self.user.is_admin
    }
}

#[derive(Debug, Clone, Deserialize, ToParameters)]
#[salvo(parameters(default_parameter_in = Query))]
pub struct AuthArgs {
    pub user_id: Option<String>,
    pub device_id: Option<String>,
    pub access_token: Option<String>,
    #[salvo(parameter(parameter_in = Header))]
    pub authorization: Option<String>,

    #[serde(default = "default_true")]
    pub from_appservice: bool,
}

impl AuthArgs {
    pub fn require_access_token(&self) -> Result<&str, MatrixError> {
        if let Some(bearer) = &self.authorization {
            if bearer.starts_with("Bearer ") {
                Ok(&bearer["Bearer ".len()..])
            } else {
                Err(MatrixError::missing_token("Invalid Bearer token.").into())
            }
        } else if let Some(access_token) = self.access_token.as_deref() {
            Ok(access_token)
        } else {
            Err(MatrixError::missing_token("Token not found.").into())
        }
    }
}
