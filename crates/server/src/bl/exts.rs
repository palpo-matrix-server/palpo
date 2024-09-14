use salvo::prelude::*;

use crate::core::identifiers::*;
use crate::{AppResult, AuthedInfo};

pub trait DepotExt {
    fn authed_info(&self) -> AppResult<&AuthedInfo>;
    fn take_authed_info(&mut self) -> AppResult<AuthedInfo>;
}
impl DepotExt for Depot {
    fn authed_info(&self) -> AppResult<&AuthedInfo> {
        self.obtain::<AuthedInfo>()
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
        println!(
            "UserId.is_romte() {:?} == {:?}",
            self.server_name(),
            crate::server_name()
        );
        self.server_name() != crate::server_name()
    }
}
impl IsRemote for OwnedUserId {
    fn is_remote(&self) -> bool {
        println!(
            "OwnedUserId.is_romte() {:?} == {:?}",
            self.server_name(),
            crate::server_name()
        );
        self.server_name() != crate::server_name()
    }
}

impl IsRemote for RoomId {
    fn is_remote(&self) -> bool {
        if let Ok(server_name) = self.server_name() {
            println!(
                "RoomId.is_romte() {:?} == {:?}",
                self.server_name(),
                crate::server_name()
            );
            server_name != crate::server_name()
        } else {
            false
        }
    }
}
impl IsRemote for OwnedRoomId {
    fn is_remote(&self) -> bool {
        if let Ok(server_name) = self.server_name() {
            println!(
                "OwnedRoomId.is_romte() {:?} == {:?}",
                self.server_name(),
                crate::server_name()
            );
            server_name != crate::server_name()
        } else {
            false
        }
    }
}

impl IsRemote for RoomAliasId {
    fn is_remote(&self) -> bool {
        println!(
            "RoomAliasId.is_romte() {:?} == {:?}",
            self.server_name(),
            crate::server_name()
        );
        self.server_name() != crate::server_name()
    }
}

impl IsRemote for OwnedRoomAliasId {
    fn is_remote(&self) -> bool {
        println!(
            "OwnedRoomAliasId.is_romte() {:?} == {:?}",
            self.server_name(),
            crate::server_name()
        );
        self.server_name() != crate::server_name()
    }
}
