use salvo::prelude::*;

use crate::core::identifiers::*;
use crate::{AppResult, AuthedInfo};

mod url;
pub use url::*;

pub trait DepotExt {
    fn authed_info(&self) -> AppResult<&AuthedInfo>;
    fn set_origin(&mut self, origin: OwnedServerName);
    fn origin(&mut self) -> AppResult<&OwnedServerName>;
    fn take_authed_info(&mut self) -> AppResult<AuthedInfo>;
}
impl DepotExt for Depot {
    fn authed_info(&self) -> AppResult<&AuthedInfo> {
        self.obtain::<AuthedInfo>()
            .map_err(|_| StatusError::unauthorized().into())
    }
    fn set_origin(&mut self, origin: OwnedServerName) {
        self.insert("origin", origin);
    }
    fn origin(&mut self) -> AppResult<&OwnedServerName> {
        self.get::<OwnedServerName>("origin")
            .map_err(|_| StatusError::unauthorized().into())
    }
    fn take_authed_info(&mut self) -> AppResult<AuthedInfo> {
        self.scrape::<AuthedInfo>()
            .map_err(|_| StatusError::unauthorized().into())
    }
}

pub trait IsRemote {
    fn is_remote(&self) -> bool;
}
impl IsRemote for UserId {
    fn is_remote(&self) -> bool {
        self.server_name() != crate::server_name()
    }
}
impl IsRemote for OwnedUserId {
    fn is_remote(&self) -> bool {
        println!(
            "server_name: {:?}   crate server name: {:?}",
            self.server_name(),
            crate::server_name()
        );
        self.server_name() != crate::server_name()
    }
}

impl IsRemote for RoomId {
    fn is_remote(&self) -> bool {
        if let Ok(server_name) = self.server_name() {
            server_name != crate::server_name()
        } else {
            false
        }
    }
}
impl IsRemote for OwnedRoomId {
    fn is_remote(&self) -> bool {
        if let Ok(server_name) = self.server_name() {
            server_name != crate::server_name()
        } else {
            false
        }
    }
}

impl IsRemote for RoomAliasId {
    fn is_remote(&self) -> bool {
        self.server_name() != crate::server_name()
    }
}

impl IsRemote for OwnedRoomAliasId {
    fn is_remote(&self) -> bool {
        self.server_name() != crate::server_name()
    }
}

impl IsRemote for ServerName {
    fn is_remote(&self) -> bool {
        self != crate::server_name()
    }
}
