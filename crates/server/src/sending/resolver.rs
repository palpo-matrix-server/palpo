use std::error::Error as StdError;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::{future, iter};

use futures_util::FutureExt;
use hyper_util::client::legacy::connect::dns::{GaiResolver, Name as HyperName};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tower_service::Service as TowerService;

use crate::TlsNameMap;

pub const MXC_LENGTH: usize = 32;
pub const DEVICE_ID_LENGTH: usize = 10;
pub const TOKEN_LENGTH: usize = 32;
pub const SESSION_ID_LENGTH: usize = 32;
pub const AUTO_GEN_PASSWORD_LENGTH: usize = 15;
pub const RANDOM_USER_ID_LENGTH: usize = 10;

pub struct Resolver {
    inner: GaiResolver,
    overrides: Arc<RwLock<TlsNameMap>>,
}

impl Resolver {
    pub fn new(overrides: Arc<RwLock<TlsNameMap>>) -> Self {
        Resolver {
            inner: GaiResolver::new(),
            overrides,
        }
    }
}

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        self.overrides
            .read()
            .unwrap()
            .get(name.as_str())
            .and_then(|(override_name, port)| {
                override_name.first().map(|first_name| {
                    let x: Box<dyn Iterator<Item = SocketAddr> + Send> =
                        Box::new(iter::once(SocketAddr::new(*first_name, *port)));
                    let x: Resolving = Box::pin(future::ready(Ok(x)));
                    x
                })
            })
            .unwrap_or_else(|| {
                let this = &mut self.inner.clone();
                Box::pin(
                    TowerService::<HyperName>::call(
                        this,
                        // Beautiful hack, please remove this in the future.
                        HyperName::from_str(name.as_str()).expect("reqwest Name is just wrapper for hyper-util Name"),
                    )
                    .map(|result| {
                        result
                            .map(|addrs| -> Addrs { Box::new(addrs) })
                            .map_err(|err| -> Box<dyn StdError + Send + Sync> { Box::new(err) })
                    }),
                )
            })
    }
}
