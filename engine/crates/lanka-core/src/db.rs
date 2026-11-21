//! Postgres pool construction and embedded migrations.

use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};

/// Migrations from `engine/migrations`, embedded at compile time.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, Clone, Copy)]
pub enum PoolRole {
    /// Read path (server, CLI queries): bounded statements, larger pool.
    Api,
    /// Ingestion / maintenance: no statement timeout, small pool.
    Ingest,
}

/// Connects a pool tuned for `role`. Prepared statements are cached per
/// connection by sqlx (every query in this crate uses bind parameters).
pub async fn connect(url: &str, role: PoolRole) -> Result<PgPool, sqlx::Error> {
    let (app, max, timeout, slow) = match role {
        PoolRole::Api => ("lanka-api", 32, "5s", Duration::from_millis(250)),
        PoolRole::Ingest => ("lanka-ingest", 8, "0", Duration::from_secs(300)),
    };
    let opts: PgConnectOptions = url
        .parse::<PgConnectOptions>()?
        .application_name(app)
        .statement_cache_capacity(256)
        .options([("statement_timeout", timeout), ("timezone", "UTC")])
        .log_slow_statements(tracing::log::LevelFilter::Warn, slow);
    PgPoolOptions::new()
        .max_connections(max)
        .min_connections(if matches!(role, PoolRole::Api) { 2 } else { 0 })
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .connect_with(opts)
        .await
}

/// Applies pending migrations.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}
