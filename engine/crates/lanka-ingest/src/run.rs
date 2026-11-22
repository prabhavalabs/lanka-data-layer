//! Ingestion run tracking and the event feed.
//!
//! The platform emits state and notifies no one (ADR 0016): every run stage,
//! job, train, review and incident change is an `ops.run_events` row
//! written through `ops.emit`, which hands out ids in commit order, so the
//! `/v2/status/events?after=` feed is strictly ordered and loss-free.
//!
//! A run holds a session-level advisory lock on a dedicated connection for
//! its whole life, so two runs of one source can never overlap, and a
//! `running` row whose lock is free is provably abandoned (crash, OOM, kill)
//! and is closed as failed — the silent-stale-run failure mode that hid
//! developing-country.org's broken cron for weeks cannot happen here.

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
    Run,
    Recipe,
    Release,
    Export,
    Job,
    Review,
    Incident,
    Worker,
    Schedule,
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
            Self::Run => "run",
            Self::Recipe => "recipe",
            Self::Release => "release",
            Self::Export => "export",
            Self::Job => "job",
            Self::Review => "review",
            Self::Incident => "incident",
            Self::Worker => "worker",
            Self::Schedule => "schedule",
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
    #[must_use]
    pub const fn as_str(self) -> &'static str {
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

/// One event of the feed. `kind` defaults to the stage name.
#[derive(Debug, Clone, Default)]
pub struct Event<'a> {
    pub kind: Option<&'a str>,
    pub run_id: Option<i64>,
    pub job_id: Option<i64>,
    pub source_id: Option<&'a str>,
    pub dataset_id: Option<&'a str>,
    pub data: Option<serde_json::Value>,
}

/// Appends an event to the feed (autocommit, through `ops.emit`) and mirrors
/// it to `tracing`. Never fails the caller: a lost event is logged.
pub async fn emit(pool: &PgPool, level: Level, stage: Stage, message: &str, ev: Event<'_>) {
    match level {
        Level::Debug => tracing::debug!(
            stage = stage.as_str(),
            kind = ev.kind,
            run = ev.run_id,
            job = ev.job_id,
            source = ev.source_id,
            "{message}"
        ),
        Level::Info => tracing::info!(
            stage = stage.as_str(),
            kind = ev.kind,
            run = ev.run_id,
            job = ev.job_id,
            source = ev.source_id,
            "{message}"
        ),
        Level::Warn => tracing::warn!(
            stage = stage.as_str(),
            kind = ev.kind,
            run = ev.run_id,
            job = ev.job_id,
            source = ev.source_id,
            "{message}"
        ),
        Level::Error => tracing::error!(
            stage = stage.as_str(),
            kind = ev.kind,
            run = ev.run_id,
            job = ev.job_id,
            source = ev.source_id,
            "{message}"
        ),
    }
    let res = sqlx::query("SELECT ops.emit($1::ops.event_level, $2, $3, $4, $5, $6, $7, $8, $9)")
        .bind(level.as_str())
        .bind(stage.as_str())
        .bind(ev.kind)
        .bind(message)
        .bind(ev.data)
        .bind(ev.run_id)
        .bind(ev.job_id)
        .bind(ev.source_id)
        .bind(ev.dataset_id)
        .execute(pool)
        .await;
    if let Err(e) = res {
        tracing::error!(error = %e, "failed to record event");
    }
}

pub struct RunTracker {
    pool: PgPool,
    /// Session holding the per-source advisory lock.
    lock: Option<PoolConnection<Postgres>>,
    pub run_id: i64,
    pub source_id: String,
    pub job_id: Option<i64>,
    finished: bool,
}

impl RunTracker {
    /// Starts a run: takes the per-source lock, closes abandoned runs, inserts
    /// the run row. Fails fast if another run of this source is active.
    pub async fn start(pool: &PgPool, source_id: &str, trigger: &str) -> Result<Self> {
        Self::start_for_job(pool, source_id, trigger, None).await
    }

    /// [`Self::start`] for a run executed by a worker job.
    pub async fn start_for_job(
        pool: &PgPool,
        source_id: &str,
        trigger: &str,
        job_id: Option<i64>,
    ) -> Result<Self> {
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
            "INSERT INTO ops.ingestion_runs (source_id, trigger, host, engine_version, job_id)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(source_id)
        .bind(trigger)
        .bind(host)
        .bind(lanka_core::ENGINE_VERSION)
        .bind(job_id)
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
            job_id,
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

    /// Records an event of this run (see [`emit`]). Never fails the run.
    pub async fn event(
        &self,
        level: Level,
        stage: Stage,
        message: &str,
        data: Option<serde_json::Value>,
    ) {
        emit(
            &self.pool,
            level,
            stage,
            message,
            Event {
                run_id: Some(self.run_id),
                job_id: self.job_id,
                source_id: Some(&self.source_id),
                data,
                ..Event::default()
            },
        )
        .await;
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

    pub async fn finish(mut self, status: RunStatus, message: &str) -> Result<()> {
        self.event(
            if status == RunStatus::NeedsReview {
                Level::Warn
            } else {
                Level::Info
            },
            Stage::Run,
            &format!("run finished: {}", status.as_str()),
            Some(json!({ "status": status.as_str(), "message": message })),
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
        Ok(())
    }

    /// Closes the run as failed. The failure is an `error` event of the feed
    /// and, after repeated failures, a `source_failing` incident (opened by
    /// the worker's tick) — nothing is delivered anywhere.
    pub async fn fail(mut self, error: &anyhow::Error) -> Result<()> {
        let text = format!("{error:#}");
        self.event(
            Level::Error,
            Stage::Run,
            &format!("run failed: {text}"),
            Some(json!({ "status": "failed" })),
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
        Ok(())
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
