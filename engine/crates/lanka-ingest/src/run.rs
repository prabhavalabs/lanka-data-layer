//! Ingestion run tracking, structured run events and alerting.
//!
//! A run holds a session-level advisory lock on a dedicated connection for
//! its whole life, so two runs of one source can never overlap, and a
//! `running` row whose lock is free is provably abandoned (crash, OOM, kill)
//! and is closed as failed — the silent-stale-run failure mode that hid
//! developing-country.org's broken cron for weeks cannot happen here.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use sqlx::pool::PoolConnection;
use sqlx::postgres::Postgres;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Discover,
    Fetch,
    Archive,
    Extract,
    Standardise,
    Parse,
    Validate,
    Stage,
    Publish,
    Alert,
    Run,
}

impl Stage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Fetch => "fetch",
            Self::Archive => "archive",
            Self::Extract => "extract",
            Self::Standardise => "standardise",
            Self::Parse => "parse",
            Self::Validate => "validate",
            Self::Stage => "stage",
            Self::Publish => "publish",
            Self::Alert => "alert",
            Self::Run => "run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    NoChange,
    NeedsReview,
    Failed,
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::NoChange => "no_change",
            Self::NeedsReview => "needs_review",
            Self::Failed => "failed",
        }
    }
}

/// Alert delivery settings (from the environment).
#[derive(Debug, Clone, Default)]
pub struct AlertConfig {
    /// POSTed a JSON body on every failed run (ntfy, Slack/Matrix hooks …).
    pub webhook: Option<String>,
    /// GET after every successful run — an external dead-man's switch.
    pub heartbeat: Option<String>,
}

impl AlertConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Self {
            webhook: get("LANKA_ALERT_WEBHOOK"),
            heartbeat: get("LANKA_HEARTBEAT_URL"),
        }
    }
}

pub struct RunTracker {
    pool: PgPool,
    /// Session holding the per-source advisory lock.
    lock: Option<PoolConnection<Postgres>>,
    pub run_id: i64,
    pub source_id: String,
    finished: bool,
}

impl RunTracker {
    /// Starts a run: takes the per-source lock, closes abandoned runs, inserts
    /// the run row. Fails fast if another run of this source is active.
    pub async fn start(pool: &PgPool, source_id: &str, trigger: &str) -> Result<Self> {
        let mut lock = pool.acquire().await?;
        let got: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('lanka-ingest:' || $1))")
                .bind(source_id)
                .fetch_one(&mut *lock)
                .await?;
        if !got {
            bail!("another ingestion run of {source_id:?} is in progress");
        }
        let abandoned = sqlx::query(
            "UPDATE ops.ingestion_runs SET status = 'failed', finished_at = now(),
                    error = 'abandoned: process exited without closing the run'
             WHERE source_id = $1 AND status = 'running'",
        )
        .bind(source_id)
        .execute(pool)
        .await?
        .rows_affected();
        let host = std::env::var("HOSTNAME").ok();
        let run_id: i64 = sqlx::query_scalar(
            "INSERT INTO ops.ingestion_runs (source_id, trigger, host, engine_version)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(source_id)
        .bind(trigger)
        .bind(host)
        .bind(lanka_core::ENGINE_VERSION)
        .fetch_one(pool)
        .await
        .with_context(|| {
            format!("starting run for source {source_id:?} (is the registry synced?)")
        })?;
        let t = Self {
            pool: pool.clone(),
            lock: Some(lock),
            run_id,
            source_id: source_id.to_owned(),
            finished: false,
        };
        if abandoned > 0 {
            t.warn(
                Stage::Run,
                &format!("closed {abandoned} abandoned run(s) of this source"),
                None,
            )
            .await;
        }
        t.info(
            Stage::Run,
            "run started",
            Some(json!({ "trigger": trigger })),
        )
        .await;
        Ok(t)
    }

    /// Records an event; also mirrored to `tracing`. Never fails the run.
    pub async fn event(
        &self,
        level: Level,
        stage: Stage,
        message: &str,
        data: Option<serde_json::Value>,
    ) {
        match level {
            Level::Debug => {
                tracing::debug!(run = self.run_id, source = %self.source_id, stage = stage.as_str(), "{message}")
            }
            Level::Info => {
                tracing::info!(run = self.run_id, source = %self.source_id, stage = stage.as_str(), "{message}")
            }
            Level::Warn => {
                tracing::warn!(run = self.run_id, source = %self.source_id, stage = stage.as_str(), "{message}")
            }
            Level::Error => {
                tracing::error!(run = self.run_id, source = %self.source_id, stage = stage.as_str(), "{message}")
            }
        }
        let res = sqlx::query(
            "INSERT INTO ops.run_events (run_id, level, stage, message, data)
             VALUES ($1, $2::ops.event_level, $3, $4, $5)",
        )
        .bind(self.run_id)
        .bind(level.as_str())
        .bind(stage.as_str())
        .bind(message)
        .bind(data)
        .execute(&self.pool)
        .await;
        if let Err(e) = res {
            tracing::error!(run = self.run_id, error = %e, "failed to record run event");
        }
    }

    pub async fn info(&self, stage: Stage, message: &str, data: Option<serde_json::Value>) {
        self.event(Level::Info, stage, message, data).await;
    }
    pub async fn warn(&self, stage: Stage, message: &str, data: Option<serde_json::Value>) {
        self.event(Level::Warn, stage, message, data).await;
    }

    pub async fn add_counts(
        &self,
        artifacts: i32,
        rows_parsed: i64,
        rows_written: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE ops.ingestion_runs SET artifacts = artifacts + $2, rows_parsed = rows_parsed + $3,
                    rows_written = rows_written + $4 WHERE id = $1",
        )
        .bind(self.run_id)
        .bind(artifacts)
        .bind(rows_parsed)
        .bind(rows_written)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records the change-detection keys of the run (compared against the
    /// last successful run of the source by the next one).
    pub async fn set_change_keys(
        &self,
        connector_version: &str,
        fingerprint: Option<[u8; 32]>,
        input_hash: Option<[u8; 32]>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE ops.ingestion_runs SET connector_version = $2, fingerprint = $3, input_hash = $4
             WHERE id = $1",
        )
        .bind(self.run_id)
        .bind(connector_version)
        .bind(fingerprint.as_ref().map(<[u8; 32]>::as_slice))
        .bind(input_hash.as_ref().map(<[u8; 32]>::as_slice))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish(
        mut self,
        status: RunStatus,
        message: &str,
        alerts: &AlertConfig,
    ) -> Result<()> {
        self.info(
            Stage::Run,
            &format!("run finished: {}", status.as_str()),
            Some(json!({ "message": message })),
        )
        .await;
        sqlx::query(
            "UPDATE ops.ingestion_runs SET status = $2::ops.run_status, finished_at = now(), message = $3 WHERE id = $1",
        )
        .bind(self.run_id)
        .bind(status.as_str())
        .bind(message)
        .execute(&self.pool)
        .await?;
        self.release_lock().await;
        if let Some(url) = &alerts.heartbeat
            && let Err(e) = http_client()?
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
        {
            tracing::warn!(error = %e, "heartbeat ping failed");
        }
        Ok(())
    }

    /// Closes the run as failed, stores an alert and delivers it.
    pub async fn fail(mut self, error: &anyhow::Error, alerts: &AlertConfig) -> Result<()> {
        let text = format!("{error:#}");
        self.event(
            Level::Error,
            Stage::Run,
            &format!("run failed: {text}"),
            None,
        )
        .await;
        sqlx::query(
            "UPDATE ops.ingestion_runs SET status = 'failed', finished_at = now(), error = $2 WHERE id = $1",
        )
        .bind(self.run_id)
        .bind(&text)
        .execute(&self.pool)
        .await?;
        self.release_lock().await;
        raise_alert(
            &self.pool,
            Some(&self.source_id),
            Some(self.run_id),
            "run_failed",
            &text,
            alerts,
        )
        .await
    }
}

impl RunTracker {
    async fn release_lock(&mut self) {
        self.finished = true;
        if let Some(mut conn) = self.lock.take() {
            let res = sqlx::query("SELECT pg_advisory_unlock(hashtext('lanka-ingest:' || $1))")
                .bind(&self.source_id)
                .execute(&mut *conn)
                .await;
            if res.is_err() {
                // Never return a session that may still hold the lock to the pool.
                drop(conn.detach());
            }
        }
    }
}

impl Drop for RunTracker {
    fn drop(&mut self) {
        if let Some(conn) = self.lock.take() {
            // Closing the session releases the lock; the `running` row is
            // closed as abandoned by the next start of this source.
            drop(conn.detach());
            if !self.finished {
                tracing::warn!(
                    run = self.run_id,
                    "run dropped without finish; next run will close it as abandoned"
                );
            }
        }
    }
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?)
}

/// Persists an alert and tries to deliver it to the webhook. Delivery
/// failures are recorded on the alert row (and retried by the scheduler).
pub async fn raise_alert(
    pool: &PgPool,
    source_id: Option<&str>,
    run_id: Option<i64>,
    kind: &str,
    message: &str,
    cfg: &AlertConfig,
) -> Result<()> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.alerts (source_id, run_id, kind, message, channel) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(source_id)
    .bind(run_id)
    .bind(kind)
    .bind(message)
    .bind(cfg.webhook.as_ref().map(|_| "webhook"))
    .fetch_one(pool)
    .await?;
    tracing::error!(alert = id, kind, source = source_id, "{message}");
    let Some(url) = &cfg.webhook else {
        return Ok(());
    };
    let body = json!({
        "title": format!("lanka: {kind} {}", source_id.unwrap_or("-")),
        "kind": kind, "source": source_id, "run_id": run_id, "message": message,
    });
    let res = http_client()?
        .post(url)
        .json(&body)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    match res {
        Ok(_) => {
            sqlx::query("UPDATE ops.alerts SET delivered_at = now() WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?
        }
        Err(e) => {
            sqlx::query("UPDATE ops.alerts SET error = $2 WHERE id = $1")
                .bind(id)
                .bind(e.to_string())
                .execute(pool)
                .await?
        }
    };
    Ok(())
}
