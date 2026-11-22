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
    /// Ingestion / maintenance (CLI): no statement timeout, small pool.
    Ingest,
    /// `lanka worker`: like `Ingest`, sized for the leader session, the job
    /// listener and two sessions (job lock + run lock) per running job.
    Worker,
}

/// Connects a pool tuned for `role`. Prepared statements are cached per
/// connection by sqlx (every query in this crate uses bind parameters).
pub async fn connect(url: &str, role: PoolRole) -> Result<PgPool, sqlx::Error> {
    // The API binds dataset keys as constants; a custom plan per execution
    // keeps partition pruning at plan time (05 §7).
    let (app, max, timeout, plans, slow) = match role {
        PoolRole::Api => (
            "lanka-api",
            32,
            "5s",
            "force_custom_plan",
            Duration::from_millis(250),
        ),
        PoolRole::Ingest => ("lanka-ingest", 8, "0", "auto", Duration::from_secs(300)),
        PoolRole::Worker => ("lanka-worker", 16, "0", "auto", Duration::from_secs(300)),
    };
    let opts: PgConnectOptions = url
        .parse::<PgConnectOptions>()?
        .application_name(app)
        .statement_cache_capacity(256)
        .options([
            ("statement_timeout", timeout),
            ("timezone", "UTC"),
            ("plan_cache_mode", plans),
        ])
        .log_slow_statements(tracing::log::LevelFilter::Warn, slow);
    PgPoolOptions::new()
        .max_connections(max)
        .min_connections(if matches!(role, PoolRole::Api) { 2 } else { 0 })
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .connect_with(opts)
        .await
}

/// Applies pending migrations, then re-applies ownership and grants
/// (`ops.apply_grants`, migration 0004) so objects created by any migration
/// belong to `lanka_owner` and are visible to the application roles.
/// Run by the migration (admin) role, never by `lanka_api`/`lanka_ingest`.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await?;
    let has: bool = sqlx::query_scalar("SELECT to_regprocedure('ops.apply_grants()') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    if has {
        sqlx::query("SELECT ops.apply_grants()")
            .execute(pool)
            .await?;
    }
    Ok(())
}
