use std::ops::Deref;
use std::sync::OnceLock;

use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager, CustomizeConnection, State};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::config::{self, DbConfig};

pub mod pool;
use crate::db;
pub use pool::{DieselPool, PgPooledConnection, PoolError};
// pub mod users;

pub static DIESEL_POOL: OnceLock<DieselPool> = OnceLock::new();
pub static REPLICA_POOL: OnceLock<Option<DieselPool>> = OnceLock::new();

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
pub fn migrate() {
    let conn = &mut db::connect().unwrap();
    println!(
        "Has pending migration: {}",
        conn.has_pending_migration(MIGRATIONS).unwrap()
    );
    conn.run_pending_migrations(MIGRATIONS)
        .expect("migrate db should worked");
}

pub fn connect() -> Result<PgPooledConnection, PoolError> {
    match DIESEL_POOL.get().expect("diesel pool should set").get() {
        Ok(conn) => Ok(conn),
        Err(e) => {
            println!("db connect error {e}");
            Err(e)
        }
    }
}
pub fn state() -> State {
    DIESEL_POOL.get().expect("diesel pool should set").state()
}

pub fn connection_url(config: &DbConfig, url: &str) -> String {
    let mut url = Url::parse(url).expect("Invalid database URL");

    if config.enforce_tls {
        maybe_append_url_param(&mut url, "sslmode", "require");
    }

    // Configure the time it takes for diesel to return an error when there is full packet loss
    // between the application and the database.
    maybe_append_url_param(&mut url, "tcp_user_timeout", &config.tcp_timeout.to_string());

    url.into()
}

fn maybe_append_url_param(url: &mut Url, key: &str, value: &str) {
    if !url.query_pairs().any(|(k, _)| k == key) {
        url.query_pairs_mut().append_pair(key, value);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConnectionConfig {
    pub statement_timeout: u64,
    // pub read_only: bool,
}

impl CustomizeConnection<PgConnection, r2d2::Error> for ConnectionConfig {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        use diesel::sql_query;

        sql_query(format!("SET statement_timeout = {}", self.statement_timeout))
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        // if self.read_only {
        //     sql_query("SET default_transaction_read_only = 't'")
        //         .execute(conn)
        //         .map_err(r2d2::Error::QueryError)?;
        // }
        Ok(())
    }
}
