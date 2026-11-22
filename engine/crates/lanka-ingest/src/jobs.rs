//! The Postgres job queue (`ops.jobs`, 07 §3, ADR 0011).
//!
//! - **Enqueue is idempotent**: one live (queued/running) job per
//!   `dedupe_key` (partial unique index); a duplicate returns the live job.
//! - **Claim** takes the next queued job of a lane whose weight fits the free
//!   slots with `FOR UPDATE SKIP LOCKED`, ordered by priority, `run_after`, id.
//! - **Lease**: a running job is owned through a *session* advisory lock on
//!   its id, held by a dedicated connection for the job's life, plus a
//!   heartbeat (`heartbeat_at`, `lease_until`). A `running` row whose lock is
//!   free is provably abandoned (crash, OOM, kill -9) and the next tick
//!   re-queues it — or fails it once `max_attempts` is spent.
//! - **Retries** back off 15 min → 1 h → 6 h (03 §7), then the job fails;
//!   the scheduler enqueues the source again at its next check.
//! - **Cancellation**: a queued job is cancelled at once; a running one gets
//!   `cancel_requested`, which its heartbeat sees, and the worker aborts it.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::pool::PoolConnection;
use sqlx::{PgPool, Postgres};

use crate::run::{Event, Level, Stage, emit};

/// Job kinds the schema accepts (07 §3 + the analyst lane, 09 §2).
pub const KINDS: &[&str] = &[
    "ingest",
    "backfill",
    "restandardise",
    "recipe",
    "release",
    "export",
    "tiles",
    "check",
    "maintenance",
    "analysis",
    "upload",
    "sync",
];

/// Lease length: a job whose heartbeat is older than this is reported as
/// stale (the advisory lock decides abandonment).
pub const LEASE: Duration = Duration::from_secs(120);
/// Heartbeat interval of a running job.
pub const HEARTBEAT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub lane: String,
    pub dedupe_key: String,
    pub payload: serde_json::Value,
    pub state: String,
    pub priority: i16,
    pub weight: i16,
    pub run_after: DateTime<Utc>,
    pub attempts: i16,
    pub max_attempts: i16,
    pub locked_by: Option<String>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
    pub trigger: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

const JOB_COLS: &str = "id, kind, lane, dedupe_key, payload, state, priority, weight, run_after, attempts,
    max_attempts, locked_by, heartbeat_at, cancel_requested, trigger, created_at, started_at, finished_at,
    result, error";

/// A job to enqueue. [`NewJob::ingest`] and [`NewJob::release`] carry the
/// conventional priorities, weights and dedupe keys.
#[derive(Debug, Clone)]
pub struct NewJob {
    pub kind: String,
    pub lane: String,
    pub dedupe_key: String,
    pub payload: serde_json::Value,
    pub priority: i16,
    pub weight: i16,
    pub max_attempts: i16,
    pub trigger: String,
    pub run_after: Option<DateTime<Utc>>,
}

impl NewJob {
    /// An ingest of one source (priority 50, weight 2: two at a time on 4 slots).
    #[must_use]
    pub fn ingest(source_id: &str, trigger: &str, payload: serde_json::Value) -> Self {
        let mut p = payload;
        if let Some(o) = p.as_object_mut() {
            o.insert("source".into(), json!(source_id));
        } else {
            p = json!({ "source": source_id });
        }
        Self {
            kind: "ingest".into(),
            lane: "batch".into(),
            dedupe_key: format!("ingest:{source_id}"),
            payload: p,
            priority: 50,
            weight: 2,
            max_attempts: 4,
            trigger: trigger.into(),
            run_after: None,
        }
    }

    /// A release train (priority 10). `slot` makes the dedupe key per day
    /// (`daily:20261122`) or per hour (`live:2026112214`), so a train that
    /// already ran for that slot is never enqueued twice.
    #[must_use]
    pub fn release(slot: &str, trigger: &str, train_trigger: &str) -> Self {
        Self {
            kind: "release".into(),
            lane: "batch".into(),
            dedupe_key: format!("release:{slot}"),
            payload: json!({ "train_trigger": train_trigger }),
            priority: 10,
            weight: 1,
            max_attempts: 3,
            trigger: trigger.into(),
            run_after: None,
        }
    }
}

impl NewJob {
    /// A recipe run outside the train (priority 150, weight 4: the heavy
    /// slot is exclusive). `recipe = None` recomputes every stale recipe.
    #[must_use]
    pub fn recipe(recipe: Option<&str>, trigger: &str, force: bool) -> Self {
        Self {
            kind: "recipe".into(),
            lane: "batch".into(),
            dedupe_key: format!("recipe:{}", recipe.unwrap_or("*")),
            payload: json!({ "recipe": recipe, "force": force }),
            priority: 150,
            weight: 4,
            max_attempts: 2,
            trigger: trigger.into(),
            run_after: None,
        }
    }
}

/// Enqueues `job`; returns `(id, created)`. A live job with the same dedupe
/// key is returned instead of a new one.
pub async fn enqueue(pool: &PgPool, job: &NewJob) -> Result<(i64, bool)> {
    anyhow::ensure!(
        KINDS.contains(&job.kind.as_str()),
        "unknown job kind {:?}",
        job.kind
    );
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO ops.jobs (kind, lane, dedupe_key, payload, priority, weight, max_attempts, trigger, run_after)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, coalesce($9, now()))
         ON CONFLICT (dedupe_key) WHERE state IN ('queued', 'running') DO NOTHING
         RETURNING id",
    )
    .bind(&job.kind)
    .bind(&job.lane)
    .bind(&job.dedupe_key)
    .bind(&job.payload)
    .bind(job.priority)
    .bind(job.weight)
    .bind(job.max_attempts)
    .bind(&job.trigger)
    .bind(job.run_after)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = id {
        emit(
            pool,
            Level::Info,
            Stage::Job,
            &format!("job {id} queued: {} ({})", job.dedupe_key, job.trigger),
            Event {
                kind: Some("job_queued"),
                job_id: Some(id),
                source_id: job.payload.get("source").and_then(|s| s.as_str()),
                data: Some(
                    json!({ "kind": job.kind, "dedupe_key": job.dedupe_key, "trigger": job.trigger,
                                   "run_after": job.run_after }),
                ),
                ..Event::default()
            },
        )
        .await;
        return Ok((id, true));
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM ops.jobs WHERE dedupe_key = $1 AND state IN ('queued', 'running')",
    )
    .bind(&job.dedupe_key)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("job {} vanished while enqueuing", job.dedupe_key))?;
    Ok((id, false))
}

/// A claimed job with the connection that holds its advisory lock.
pub struct Claimed {
    pub job: Job,
    /// Session holding `pg_advisory_lock(hashtext('lanka-job'), id)`;
    /// dropping it (process death) frees the lock.
    pub lock: PoolConnection<Postgres>,
}

/// Claims the next runnable job of `lane` that fits `free_slots`.
pub async fn claim(
    pool: &PgPool,
    worker_id: &str,
    lane: &str,
    free_slots: i16,
    kinds: &[&str],
) -> Result<Option<Claimed>> {
    if free_slots <= 0 {
        return Ok(None);
    }
    let mut conn = pool.acquire().await?;
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    let cand: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM ops.jobs
         WHERE state = 'queued' AND lane = $1 AND run_after <= now() AND weight <= $2 AND kind = ANY($3)
         ORDER BY priority, run_after, id
         FOR UPDATE SKIP LOCKED LIMIT 1",
    )
    .bind(lane)
    .bind(free_slots)
    .bind(kinds)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(id) = cand else {
        tx.rollback().await?;
        return Ok(None);
    };
    // Session-level: survives this transaction, released with the session.
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('lanka-job'), $1::int4)")
            .bind(i32::try_from(id)?)
            .fetch_one(&mut *tx)
            .await?;
    if !locked {
        // A previous holder's session is still alive (being torn down).
        tx.rollback().await?;
        return Ok(None);
    }
    let job: Job = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE ops.jobs SET state = 'running', locked_by = $2, locked_at = now(), heartbeat_at = now(),
            lease_until = now() + make_interval(secs => $3), started_at = now(), attempts = attempts + 1,
            finished_at = NULL
         WHERE id = $1 RETURNING {JOB_COLS}"
    )))
    .bind(id)
    .bind(worker_id)
    .bind(LEASE.as_secs_f64())
    .fetch_one(&mut *tx)
    .await?;
    if let Err(e) = tx.commit().await {
        drop(conn.detach()); // never pool a session that may hold the lock
        return Err(e.into());
    }
    emit(
        pool,
        Level::Info,
        Stage::Job,
        &format!(
            "job {} started: {} (attempt {}/{})",
            job.id, job.dedupe_key, job.attempts, job.max_attempts
        ),
        Event {
            kind: Some("job_started"),
            job_id: Some(job.id),
            source_id: job.payload.get("source").and_then(|s| s.as_str()),
            data: Some(json!({ "worker": worker_id, "attempt": job.attempts })),
            ..Event::default()
        },
    )
    .await;
    Ok(Some(Claimed { job, lock: conn }))
}

impl Claimed {
    /// Releases the job lock; a session whose unlock failed is closed.
    pub async fn release(self) {
        let mut conn = self.lock;
        let res = sqlx::query("SELECT pg_advisory_unlock(hashtext('lanka-job'), $1::int4)")
            .bind(i32::try_from(self.job.id).unwrap_or_default())
            .execute(&mut *conn)
            .await;
        if res.is_err() {
            drop(conn.detach());
        }
    }
}

/// Refreshes the lease; returns whether cancellation was requested.
pub async fn heartbeat(pool: &PgPool, id: i64) -> Result<bool> {
    let cancel: Option<bool> = sqlx::query_scalar(
        "UPDATE ops.jobs SET heartbeat_at = now(), lease_until = now() + make_interval(secs => $2)
         WHERE id = $1 AND state = 'running' RETURNING cancel_requested",
    )
    .bind(id)
    .bind(LEASE.as_secs_f64())
    .fetch_optional(pool)
    .await?;
    Ok(cancel.unwrap_or(true))
}

/// Marks a job succeeded with its result.
pub async fn succeed(pool: &PgPool, job: &Job, result: &serde_json::Value) -> Result<()> {
    sqlx::query(
        "UPDATE ops.jobs SET state = 'succeeded', finished_at = now(), result = $2, error = NULL,
            locked_by = NULL, lease_until = NULL WHERE id = $1",
    )
    .bind(job.id)
    .bind(result)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE ops.incidents SET closed_at = now() WHERE kind = 'job_failed' AND subject = $1 AND closed_at IS NULL",
    )
    .bind(&job.dedupe_key)
    .execute(pool)
    .await?;
    emit(
        pool,
        Level::Info,
        Stage::Job,
        &format!("job {} succeeded: {}", job.id, job.dedupe_key),
        Event {
            kind: Some("job_succeeded"),
            job_id: Some(job.id),
            source_id: job.payload.get("source").and_then(|s| s.as_str()),
            data: Some(result.clone()),
            ..Event::default()
        },
    )
    .await;
    Ok(())
}

/// Backoff before retry `attempt` (1-based count of attempts made).
#[must_use]
pub fn backoff(attempt: i16) -> Duration {
    match attempt {
        ..=1 => Duration::from_secs(15 * 60),
        2 => Duration::from_secs(3600),
        _ => Duration::from_secs(6 * 3600),
    }
}

/// Records a failure: re-queued with backoff while attempts remain
/// (`retryable`), else failed for good (and a `job_failed` incident).
pub async fn fail(pool: &PgPool, job: &Job, error: &str, retryable: bool) -> Result<&'static str> {
    let retry = retryable && job.attempts < job.max_attempts;
    if retry {
        let wait = backoff(job.attempts);
        sqlx::query(
            "UPDATE ops.jobs SET state = 'queued', run_after = now() + make_interval(secs => $3), error = $2,
                locked_by = NULL, lease_until = NULL, heartbeat_at = NULL, finished_at = now()
             WHERE id = $1",
        )
        .bind(job.id)
        .bind(error)
        .bind(wait.as_secs_f64())
        .execute(pool)
        .await?;
        emit(
            pool,
            Level::Warn,
            Stage::Job,
            &format!(
                "job {} failed (attempt {}/{}), retry in {} min: {error}",
                job.id,
                job.attempts,
                job.max_attempts,
                wait.as_secs() / 60
            ),
            Event {
                kind: Some("job_retry"),
                job_id: Some(job.id),
                source_id: job.payload.get("source").and_then(|s| s.as_str()),
                data: Some(json!({ "attempt": job.attempts, "retry_in_s": wait.as_secs() })),
                ..Event::default()
            },
        )
        .await;
        return Ok("queued");
    }
    sqlx::query(
        "UPDATE ops.jobs SET state = 'failed', finished_at = now(), error = $2, locked_by = NULL,
            lease_until = NULL WHERE id = $1",
    )
    .bind(job.id)
    .bind(error)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO ops.incidents (kind, subject, detail) VALUES ('job_failed', $1, $2)
         ON CONFLICT (kind, subject) WHERE closed_at IS NULL
         DO UPDATE SET last_seen_at = now(), detail = EXCLUDED.detail",
    )
    .bind(&job.dedupe_key)
    .bind(json!({ "job_id": job.id, "kind": job.kind, "attempts": job.attempts, "error": error }))
    .execute(pool)
    .await?;
    emit(
        pool,
        Level::Error,
        Stage::Job,
        &format!(
            "job {} failed after {} attempt(s): {error}",
            job.id, job.attempts
        ),
        Event {
            kind: Some("job_failed"),
            job_id: Some(job.id),
            source_id: job.payload.get("source").and_then(|s| s.as_str()),
            data: Some(json!({ "attempts": job.attempts })),
            ..Event::default()
        },
    )
    .await;
    Ok("failed")
}

/// Marks a running job cancelled (after the worker aborted it) and closes
/// the ingestion runs it left open.
pub async fn mark_cancelled(pool: &PgPool, job: &Job, why: &str) -> Result<()> {
    sqlx::query(
        "UPDATE ops.jobs SET state = 'cancelled', finished_at = now(), error = $2, locked_by = NULL,
            lease_until = NULL WHERE id = $1",
    )
    .bind(job.id)
    .bind(why)
    .execute(pool)
    .await?;
    close_job_runs(pool, job.id, why).await?;
    emit(
        pool,
        Level::Warn,
        Stage::Job,
        &format!("job {} cancelled: {why}", job.id),
        Event {
            kind: Some("job_cancelled"),
            job_id: Some(job.id),
            source_id: job.payload.get("source").and_then(|s| s.as_str()),
            ..Event::default()
        },
    )
    .await;
    Ok(())
}

/// Puts a running job back in the queue without spending an attempt
/// (graceful shutdown: SIGTERM between deploys).
pub async fn requeue(pool: &PgPool, job: &Job, why: &str) -> Result<()> {
    sqlx::query(
        "UPDATE ops.jobs SET state = 'queued', attempts = greatest(attempts - 1, 0), run_after = now(),
            locked_by = NULL, lease_until = NULL, heartbeat_at = NULL, error = $2 WHERE id = $1 AND state = 'running'",
    )
    .bind(job.id)
    .bind(why)
    .execute(pool)
    .await?;
    close_job_runs(pool, job.id, why).await?;
    emit(
        pool,
        Level::Warn,
        Stage::Job,
        &format!("job {} re-queued: {why}", job.id),
        Event {
            kind: Some("job_requeued"),
            job_id: Some(job.id),
            source_id: job.payload.get("source").and_then(|s| s.as_str()),
            ..Event::default()
        },
    )
    .await;
    Ok(())
}

async fn close_job_runs(pool: &PgPool, job_id: i64, why: &str) -> Result<()> {
    sqlx::query(
        "UPDATE ops.ingestion_runs SET status = 'failed', finished_at = now(), error = $2
         WHERE job_id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .bind(format!("aborted: {why}"))
    .execute(pool)
    .await?;
    Ok(())
}

/// Cancels a job: a queued one at once, a running one on its next heartbeat.
pub async fn cancel(pool: &PgPool, id: i64) -> Result<&'static str> {
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM ops.jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    match state.as_deref() {
        None => bail!("no job {id}"),
        Some("queued") => {
            sqlx::query(
                "UPDATE ops.jobs SET state = 'cancelled', finished_at = now(), error = 'cancelled while queued'
                 WHERE id = $1 AND state = 'queued'",
            )
            .bind(id)
            .execute(pool)
            .await?;
            emit(
                pool,
                Level::Info,
                Stage::Job,
                &format!("job {id} cancelled while queued"),
                Event {
                    kind: Some("job_cancelled"),
                    job_id: Some(id),
                    ..Event::default()
                },
            )
            .await;
            Ok("cancelled")
        }
        Some("running") => {
            sqlx::query("UPDATE ops.jobs SET cancel_requested = true WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            Ok("cancel_requested")
        }
        Some(other) => bail!("job {id} is already {other}"),
    }
}

/// Re-queues (or fails) `running` jobs whose advisory lock is free: their
/// worker died. Returns the ids handled.
pub async fn reap_abandoned(pool: &PgPool) -> Result<Vec<i64>> {
    let running: Vec<Job> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {JOB_COLS} FROM ops.jobs WHERE state = 'running'"
    )))
    .fetch_all(pool)
    .await?;
    let mut handled = Vec::new();
    if running.is_empty() {
        return Ok(handled);
    }
    let mut probe = pool.acquire().await?;
    for job in running {
        let key = i32::try_from(job.id)?;
        let free: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext('lanka-job'), $1::int4)")
                .bind(key)
                .fetch_one(&mut *probe)
                .await?;
        if !free {
            continue;
        }
        sqlx::query("SELECT pg_advisory_unlock(hashtext('lanka-job'), $1::int4)")
            .bind(key)
            .execute(&mut *probe)
            .await?;
        let why = format!(
            "abandoned: worker {} exited without finishing the job",
            job.locked_by.as_deref().unwrap_or("?")
        );
        close_job_runs(pool, job.id, &why).await?;
        if job.attempts < job.max_attempts {
            let n = sqlx::query(
                "UPDATE ops.jobs SET state = 'queued', run_after = now(), locked_by = NULL,
                    lease_until = NULL, heartbeat_at = NULL, error = $2 WHERE id = $1 AND state = 'running'",
            )
            .bind(job.id)
            .bind(&why)
            .execute(pool)
            .await?
            .rows_affected();
            if n > 0 {
                emit(
                    pool,
                    Level::Warn,
                    Stage::Job,
                    &format!("job {} re-queued: {why}", job.id),
                    Event {
                        kind: Some("job_abandoned"),
                        job_id: Some(job.id),
                        source_id: job.payload.get("source").and_then(|s| s.as_str()),
                        data: Some(json!({ "attempts": job.attempts, "worker": job.locked_by })),
                        ..Event::default()
                    },
                )
                .await;
            }
        } else {
            fail(pool, &job, &why, false).await?;
        }
        handled.push(job.id);
    }
    Ok(handled)
}

/// Recent jobs, newest first.
pub async fn list(pool: &PgPool, state: Option<&str>, limit: i64) -> Result<Vec<Job>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {JOB_COLS} FROM ops.jobs WHERE $1::text IS NULL OR state = $1 ORDER BY id DESC LIMIT $2"
    )))
    .bind(state)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// One job by id.
pub async fn get(pool: &PgPool, id: i64) -> Result<Option<Job>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {JOB_COLS} FROM ops.jobs WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_the_documented_ladder() {
        assert_eq!(backoff(1), Duration::from_secs(900));
        assert_eq!(backoff(2), Duration::from_secs(3600));
        assert_eq!(backoff(3), Duration::from_secs(21_600));
        assert_eq!(backoff(9), Duration::from_secs(21_600));
    }

    #[test]
    fn conventional_jobs() {
        let j = NewJob::ingest("worldbank-wdi", "schedule", json!({}));
        assert_eq!(
            (j.dedupe_key.as_str(), j.priority, j.weight),
            ("ingest:worldbank-wdi", 50, 2)
        );
        assert_eq!(j.payload["source"], "worldbank-wdi");
        let r = NewJob::release("daily:20261122", "schedule", "schedule");
        assert_eq!(
            (r.dedupe_key.as_str(), r.priority),
            ("release:daily:20261122", 10)
        );
    }
}
