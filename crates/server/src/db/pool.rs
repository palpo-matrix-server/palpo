use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager, CustomizeConnection, State};
use std::ops::Deref;
use std::time::Duration;
use thiserror::Error;

use super::connection_url;
use crate::config::{self, DbConfig};

pub type PgPool = r2d2::Pool<ConnectionManager<PgConnection>>;
pub type PgPooledConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

#[derive(Clone, Debug)]
pub struct DieselPool {
    inner: PgPool,
}

impl DieselPool {
    pub(crate) fn new(
        url: &String,
        config: &DbConfig,
        r2d2_config: r2d2::Builder<ConnectionManager<PgConnection>>,
    ) -> Result<DieselPool, PoolError> {
        let manager = ConnectionManager::new(connection_url(config, url));

        // For crates.io we want the behavior of creating a database pool to be slightly different
        // than the defaults of R2D2: the library's build() method assumes its consumers always
        // need a database connection to operate, so it blocks creating a pool until a minimum
        // number of connections is available.
        //
        // crates.io can actually operate in a limited capacity without a database connections,
        // especially by serving download requests to our users. Because of that we don't want to
        // block indefinitely waiting for a connection: we instead need to wait for a bit (to avoid
        // serving errors for the first connections until the pool is initialized) and if we can't
        // establish any connection continue booting up the application. The database pool will
        // automatically be marked as unhealthy and the rest of the application will adapt.
        let pool = DieselPool {
            inner: r2d2_config.build_unchecked(manager),
        };
        match pool.wait_until_healthy(Duration::from_secs(5)) {
            Ok(()) => {}
            Err(PoolError::UnhealthyPool) => {}
            Err(err) => return Err(err),
        }

        Ok(pool)
    }

    pub fn new_background_worker(inner: r2d2::Pool<ConnectionManager<PgConnection>>) -> Self {
        Self { inner }
    }

    pub fn get(&self) -> Result<PgPooledConnection, PoolError> {
        Ok(self.inner.get()?)
    }

    pub fn state(&self) -> State {
        self.inner.state()
    }

    #[instrument(skip_all)]
    pub fn wait_until_healthy(&self, timeout: Duration) -> Result<(), PoolError> {
        match self.inner.get_timeout(timeout) {
            Ok(_) => Ok(()),
            Err(_) if !self.is_healthy() => Err(PoolError::UnhealthyPool),
            Err(err) => Err(PoolError::R2D2(err)),
        }
    }

    fn is_healthy(&self) -> bool {
        self.state().connections > 0
    }
}

impl Deref for DieselPool {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConnectionConfig {
    pub statement_timeout: Duration,
    pub read_only: bool,
}

impl CustomizeConnection<PgConnection, r2d2::Error> for ConnectionConfig {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        use diesel::sql_query;

        sql_query(format!(
            "SET statement_timeout = {}",
            self.statement_timeout.as_millis()
        ))
        .execute(conn)
        .map_err(r2d2::Error::QueryError)?;
        if self.read_only {
            sql_query("SET default_transaction_read_only = 't'")
                .execute(conn)
                .map_err(r2d2::Error::QueryError)?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum PoolError {
    #[error(transparent)]
    R2D2(#[from] r2d2::PoolError),
    #[error("unhealthy database pool")]
    UnhealthyPool,
    #[error("Failed to lock test database connection")]
    TestConnectionUnavailable,
}
