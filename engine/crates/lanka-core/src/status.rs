//! Health and status reads (07 §4, ADR 0016). The platform *emits* state;
//! an external monitor polls these. Everything here is read-only SQL that the
//! `lanka_api` role may run.
//!
//! `overall` rule (a contract the monitor depends on):
//! - `failing` — the worker's last tick is older than 5 min (or it never
//!   ticked), the last release train failed, or a P0 source is `failing`;
//! - `degraded` — any source is `failing`/`overdue`/`expected_missing`, the
//!   oldest runnable queued job waits > 6 h, the store disk is > 85 % full,
//!   or (once backups are recorded) the last good pg/store backup is > 36 h old;
//! - `ok` otherwise. `reasons` lists every rule that fired.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::error::{CoreError, CoreResult};

/// Worker tick age beyond which the platform is `failing`.
pub const TICK_FAILING_S: f64 = 300.0;
/// Oldest runnable queued job beyond which the platform is `degraded`.
pub const QUEUE_DEGRADED_S: f64 = 6.0 * 3600.0;
pub const DISK_DEGRADED_PCT: f64 = 85.0;
pub const BACKUP_STALE_H: f64 = 36.0;

// ───────────────────────────── /v2/status ──────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Queue {
    pub queued: i64,
    /// Queued jobs whose `run_after` is still in the future (retry backoff, windows).
    pub waiting: i64,
    pub running: i64,
    /// Age of the oldest runnable queued job.
    pub oldest_queued_age_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkerStatus {
    /// Worker id of the batch leader (`host:pid:started`).
    pub leader: Option<String>,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub tick_age_s: Option<f64>,
    pub standby: i64,
    pub queue: Queue,
}

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct TrainBrief {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// running | released | no_change | failed
    pub outcome: String,
    pub data_version: Option<String>,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReleaseStatus {
    pub data_version: Option<String>,
    pub cut_at: Option<DateTime<Utc>>,
    pub age_h: Option<f64>,
    /// Approved dataset versions waiting for the next train.
    pub approved_pending: i64,
    pub last_train: Option<TrainBrief>,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct SourceCounts {
    pub fresh: i64,
    pub due: i64,
    pub overdue: i64,
    pub failing: i64,
    pub in_review: i64,
    pub expected_missing: i64,
    pub paused: i64,
    pub never_run: i64,
    pub retired: i64,
    /// Sources with an implemented connector (the ones the worker schedules).
    pub schedulable: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Disk {
    /// Blob store filesystem usage, as measured by the worker's last tick.
    pub store_pct: Option<f64>,
    /// Database size (the data volume's filesystem is not visible to the engine).
    pub pg_bytes: i64,
    pub pg_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Backups {
    pub pg_last_ok_at: Option<DateTime<Utc>>,
    pub store_last_ok_at: Option<DateTime<Utc>>,
    pub drill_last_ok_at: Option<DateTime<Utc>>,
    /// False until the first `lanka ops record-backup` (WP-25); staleness
    /// only counts once backups exist.
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StatusSummary {
    /// ok | degraded | failing
    pub overall: String,
    pub reasons: Vec<String>,
    pub generated_at: DateTime<Utc>,
    pub worker: WorkerStatus,
    pub release: ReleaseStatus,
    pub sources: SourceCounts,
    pub incidents_open: i64,
    pub disk: Disk,
    pub backups: Backups,
}

#[derive(FromRow)]
struct SummaryRow {
    leader: Option<String>,
    last_tick_at: Option<DateTime<Utc>>,
    standby: i64,
    store_pct: Option<f32>,
    queued: i64,
    waiting: i64,
    running: i64,
    oldest_queued_age_s: Option<f64>,
    data_version: Option<String>,
    cut_at: Option<DateTime<Utc>>,
    approved_pending: i64,
    incidents_open: i64,
    pg_bytes: i64,
    pg_last_ok_at: Option<DateTime<Utc>>,
    store_last_ok_at: Option<DateTime<Utc>>,
    drill_last_ok_at: Option<DateTime<Utc>>,
    backups_any: bool,
    p0_failing: i64,
}

const SUMMARY_SQL: &str = "
WITH w AS (   -- the leader, else the batch worker that ticked last (a stopped one ages out)
  SELECT CASE WHEN role = 'leader' THEN id END AS id, last_tick_at, store_pct FROM ops.workers
  WHERE lane = 'batch' AND last_tick_at IS NOT NULL
  ORDER BY (role = 'leader') DESC, last_tick_at DESC LIMIT 1
), q AS (
  SELECT count(*) FILTER (WHERE state = 'queued' AND run_after <= now()) AS queued,
         count(*) FILTER (WHERE state = 'queued' AND run_after > now()) AS waiting,
         count(*) FILTER (WHERE state = 'running') AS running,
         extract(epoch FROM now() - min(run_after) FILTER (WHERE state = 'queued' AND run_after <= now()))::float8
           AS oldest_queued_age_s
  FROM ops.jobs WHERE state IN ('queued', 'running')
), b AS (
  SELECT max(at) FILTER (WHERE kind = 'pg' AND ok) AS pg_last_ok_at,
         max(at) FILTER (WHERE kind = 'store' AND ok) AS store_last_ok_at,
         max(at) FILTER (WHERE kind = 'drill' AND ok) AS drill_last_ok_at,
         count(*) > 0 AS backups_any
  FROM ops.backups
)
SELECT (SELECT id FROM w) AS leader, (SELECT last_tick_at FROM w) AS last_tick_at,
       (SELECT count(*) FROM ops.workers WHERE role = 'standby' AND last_tick_at > now() - interval '5 minutes')
         AS standby,
       (SELECT store_pct FROM w) AS store_pct,
       q.queued, q.waiting, q.running, q.oldest_queued_age_s,
       (SELECT data_version FROM data.current_release) AS data_version,
       (SELECT created_at FROM data.current_release) AS cut_at,
       (SELECT count(*) FROM data.dataset_versions WHERE status = 'approved') AS approved_pending,
       (SELECT count(*) FROM ops.incidents WHERE closed_at IS NULL) AS incidents_open,
       pg_database_size(current_database()) AS pg_bytes,
       b.pg_last_ok_at, b.store_last_ok_at, b.drill_last_ok_at, b.backups_any,
       (SELECT count(*) FROM ops.source_freshness WHERE state = 'failing' AND priority = 'P0') AS p0_failing
FROM q, b";

/// `/v2/status`: the one-screen summary with the computed `overall` state.
pub async fn summary(pool: &PgPool) -> CoreResult<StatusSummary> {
    let (row, counts, train) = tokio::join!(
        sqlx::query_as::<_, SummaryRow>(SUMMARY_SQL).fetch_one(pool),
        sqlx::query_as::<_, (String, bool, i64)>(
            "SELECT state, schedulable, count(*) FROM ops.source_freshness GROUP BY 1, 2",
        )
        .fetch_all(pool),
        sqlx::query_as::<_, TrainBrief>(
            "SELECT id, started_at AS at, finished_at, outcome, data_version, trigger
             FROM ops.trains WHERE outcome <> 'running' ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(pool),
    );
    let (row, counts, train) = (row?, counts?, train?);
    let mut sources = SourceCounts::default();
    for (state, schedulable, n) in counts {
        if schedulable {
            sources.schedulable += n;
        }
        match state.as_str() {
            "fresh" => sources.fresh += n,
            "due" => sources.due += n,
            "overdue" => sources.overdue += n,
            "failing" => sources.failing += n,
            "in_review" => sources.in_review += n,
            "expected_missing" => sources.expected_missing += n,
            "paused" => sources.paused += n,
            "retired" => sources.retired += n,
            _ => sources.never_run += n,
        }
    }
    let now = Utc::now();
    #[allow(clippy::cast_precision_loss)]
    let age_s = |t: DateTime<Utc>| (now - t).num_milliseconds() as f64 / 1000.0;
    let tick_age_s = row.last_tick_at.map(age_s);
    let store_pct = row.store_pct.map(f64::from);

    let mut failing = Vec::new();
    let mut degraded = Vec::new();
    match tick_age_s {
        None => failing.push("worker has never ticked".to_owned()),
        Some(a) if a > TICK_FAILING_S => failing.push(format!("worker tick is {a:.0} s old")),
        Some(_) => {}
    }
    if let Some(t) = train.as_ref().filter(|t| t.outcome == "failed") {
        failing.push(format!("last release train ({}) failed", t.id));
    }
    if row.p0_failing > 0 {
        failing.push(format!("{} P0 source(s) failing", row.p0_failing));
    }
    for (n, what) in [
        (sources.failing, "failing"),
        (sources.overdue, "overdue"),
        (sources.expected_missing, "expected_missing"),
    ] {
        if n > 0 {
            degraded.push(format!("{n} source(s) {what}"));
        }
    }
    if let Some(a) = row.oldest_queued_age_s.filter(|a| *a > QUEUE_DEGRADED_S) {
        degraded.push(format!("oldest queued job waits {:.1} h", a / 3600.0));
    }
    if let Some(p) = store_pct.filter(|p| *p > DISK_DEGRADED_PCT) {
        degraded.push(format!("store disk {p:.0} % full"));
    }
    if row.backups_any {
        for (kind, at) in [("pg", row.pg_last_ok_at), ("store", row.store_last_ok_at)] {
            if at.is_none_or(|t| age_s(t) > BACKUP_STALE_H * 3600.0) {
                degraded.push(format!("{kind} backup older than {BACKUP_STALE_H} h"));
            }
        }
    }
    let overall = if !failing.is_empty() {
        "failing"
    } else if !degraded.is_empty() {
        "degraded"
    } else {
        "ok"
    };
    failing.extend(degraded);
    Ok(StatusSummary {
        overall: overall.to_owned(),
        reasons: failing,
        generated_at: now,
        worker: WorkerStatus {
            leader: row.leader,
            last_tick_at: row.last_tick_at,
            tick_age_s: tick_age_s.map(|a| (a * 10.0).round() / 10.0),
            standby: row.standby,
            queue: Queue {
                queued: row.queued,
                waiting: row.waiting,
                running: row.running,
                oldest_queued_age_s: row.oldest_queued_age_s.map(f64::round),
            },
        },
        release: ReleaseStatus {
            age_h: row.cut_at.map(|t| (age_s(t) / 360.0).round() / 10.0),
            data_version: row.data_version,
            cut_at: row.cut_at,
            approved_pending: row.approved_pending,
            last_train: train,
        },
        sources,
        incidents_open: row.incidents_open,
        disk: Disk {
            store_pct: store_pct.map(|p| (p * 10.0).round() / 10.0),
            pg_bytes: row.pg_bytes,
            pg_pct: None,
        },
        backups: Backups {
            pg_last_ok_at: row.pg_last_ok_at,
            store_last_ok_at: row.store_last_ok_at,
            drill_last_ok_at: row.drill_last_ok_at,
            configured: row.backups_any,
        },
    })
}

// ───────────────────────────── /v2/status/sources ──────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LastRun {
    pub id: i64,
    pub status: String,
    pub at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Expected {
    pub edition: String,
    pub date: chrono::NaiveDate,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SourceStatus {
    pub source_id: String,
    pub state: String,
    pub cadence: String,
    pub priority: String,
    pub schedulable: bool,
    pub paused: bool,
    pub check_every_s: Option<f64>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_run: Option<LastRun>,
    pub consecutive_failures: i64,
    pub next_check_at: Option<DateTime<Utc>>,
    pub expected: Option<Expected>,
    pub in_review: bool,
}

#[derive(FromRow)]
struct SourceRow {
    source_id: String,
    state: String,
    cadence: String,
    priority: String,
    schedulable: bool,
    paused: bool,
    check_every_s: Option<f64>,
    last_success_at: Option<DateTime<Utc>>,
    last_run_id: Option<i64>,
    last_status: Option<String>,
    last_run_at: Option<DateTime<Utc>>,
    last_finished_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    consecutive_failures: i64,
    next_check_at: Option<DateTime<Utc>>,
    expected_edition: Option<String>,
    expected_date: Option<chrono::NaiveDate>,
    in_review: bool,
}

/// Valid `state` filters.
pub const SOURCE_STATES: &[&str] = &[
    "fresh",
    "due",
    "overdue",
    "failing",
    "in_review",
    "expected_missing",
    "never_run",
    "paused",
    "retired",
];

/// `/v2/status/sources?state=&schedulable=`.
pub async fn sources(
    pool: &PgPool,
    state: Option<&str>,
    schedulable: Option<bool>,
) -> CoreResult<Vec<SourceStatus>> {
    if let Some(s) = state
        && !SOURCE_STATES.contains(&s)
    {
        return Err(CoreError::bad_request(format!(
            "state must be one of {}",
            SOURCE_STATES.join(", ")
        )));
    }
    let rows: Vec<SourceRow> = sqlx::query_as(
        "SELECT source_id, state, cadence, priority, schedulable, paused,
                extract(epoch FROM check_every)::float8 AS check_every_s, last_success_at, last_run_id,
                last_status, last_run_at, last_finished_at, last_error, consecutive_failures, next_check_at,
                expected_edition, expected_date, in_review
         FROM ops.source_freshness
         WHERE ($1::text IS NULL OR state = $1) AND ($2::bool IS NULL OR schedulable = $2)
         ORDER BY schedulable DESC, source_id",
    )
    .bind(state)
    .bind(schedulable)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SourceStatus {
            last_run: r.last_run_id.zip(r.last_run_at).map(|(id, at)| LastRun {
                id,
                status: r.last_status.clone().unwrap_or_default(),
                at,
                finished_at: r.last_finished_at,
                error: r.last_error.clone(),
            }),
            expected: r
                .expected_edition
                .zip(r.expected_date)
                .map(|(edition, date)| Expected { edition, date }),
            source_id: r.source_id,
            state: r.state,
            cadence: r.cadence,
            priority: r.priority,
            schedulable: r.schedulable,
            paused: r.paused,
            check_every_s: r.check_every_s,
            last_success_at: r.last_success_at,
            consecutive_failures: r.consecutive_failures,
            next_check_at: r.next_check_at,
            in_review: r.in_review,
        })
        .collect())
}

// ───────────────────────────── /v2/status/datasets ─────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct DatasetStatus {
    pub dataset_id: String,
    pub kind: String,
    /// published | approved | in_review | unpublished
    pub state: String,
    pub dsv: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub vintage: Option<String>,
    /// The open (unreleased) version, if any: approved (next train) or in review.
    pub pending_dsv: Option<String>,
    pub pending_status: Option<String>,
    pub review_open: bool,
    /// Derived inputs changed but the recipe kept its previous version (WP-06).
    pub stale_inputs: bool,
}

pub const DATASET_STATES: &[&str] = &["published", "approved", "in_review", "unpublished"];

/// `/v2/status/datasets?state=`.
pub async fn datasets(pool: &PgPool, state: Option<&str>) -> CoreResult<Vec<DatasetStatus>> {
    if let Some(s) = state
        && !DATASET_STATES.contains(&s)
    {
        return Err(CoreError::bad_request(format!(
            "state must be one of {}",
            DATASET_STATES.join(", ")
        )));
    }
    Ok(sqlx::query_as(
        "WITH d AS (
           SELECT ds.id AS dataset_id, ds.kind, cv.dsv, cv.published_at, cv.vintage,
                  o.dsv AS pending_dsv, o.status AS pending_status,
                  EXISTS (SELECT 1 FROM ops.reviews r WHERE r.version_id = o.id AND r.state = 'open') AS review_open
           FROM catalog.datasets ds
           LEFT JOIN data.current_versions cv ON cv.dataset_id = ds.id
           LEFT JOIN data.dataset_versions o ON o.dataset_id = ds.id AND o.status IN ('staged', 'in_review', 'approved'))
         SELECT dataset_id, kind,
                CASE WHEN pending_status = 'in_review' THEN 'in_review'
                     WHEN pending_status = 'approved' THEN 'approved'
                     WHEN dsv IS NOT NULL THEN 'published' ELSE 'unpublished' END AS state,
                dsv, published_at, vintage, pending_dsv, pending_status, review_open, false AS stale_inputs
         FROM d
         WHERE $1::text IS NULL OR (CASE WHEN pending_status = 'in_review' THEN 'in_review'
                     WHEN pending_status = 'approved' THEN 'approved'
                     WHEN dsv IS NOT NULL THEN 'published' ELSE 'unpublished' END) = $1
         ORDER BY (dsv IS NULL AND pending_dsv IS NULL), dataset_id",
    )
    .bind(state)
    .fetch_all(pool)
    .await?)
}

// ───────────────────────────── /v2/status/runs ─────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct RunRecord {
    pub id: i64,
    pub source_id: String,
    pub trigger: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_s: Option<f64>,
    pub artifacts: i32,
    pub rows_parsed: i64,
    pub rows_written: i64,
    pub message: Option<String>,
    pub error: Option<String>,
    pub job_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunPage {
    pub runs: Vec<RunRecord>,
    /// Pass as `cursor` for the next (older) page.
    pub next_cursor: Option<i64>,
}

pub const RUN_STATUSES: &[&str] = &[
    "running",
    "succeeded",
    "no_change",
    "needs_review",
    "failed",
];

/// `/v2/status/runs?since=&source=&status=&cursor=&limit=` — newest first.
pub async fn runs(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    source: Option<&str>,
    status: Option<&str>,
    cursor: Option<i64>,
    limit: i64,
) -> CoreResult<RunPage> {
    if let Some(s) = status
        && !RUN_STATUSES.contains(&s)
    {
        return Err(CoreError::bad_request(format!(
            "status must be one of {}",
            RUN_STATUSES.join(", ")
        )));
    }
    let limit = limit.clamp(1, 200);
    let runs: Vec<RunRecord> = sqlx::query_as(
        "SELECT id, source_id, trigger, status::text AS status, started_at, finished_at,
                extract(epoch FROM finished_at - started_at)::float8 AS duration_s,
                artifacts, rows_parsed, rows_written, message, error, job_id
         FROM ops.ingestion_runs
         WHERE ($1::timestamptz IS NULL OR started_at >= $1) AND ($2::text IS NULL OR source_id = $2)
           AND ($3::text IS NULL OR status::text = $3) AND ($4::int8 IS NULL OR id < $4)
         ORDER BY id DESC LIMIT $5",
    )
    .bind(since)
    .bind(source)
    .bind(status)
    .bind(cursor)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let full = runs.len() as i64 == limit;
    let next_cursor = runs.last().filter(|_| full).map(|r| r.id);
    Ok(RunPage { runs, next_cursor })
}

// ───────────────────────────── /v2/status/events ───────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct EventRecord {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub level: String,
    pub stage: String,
    pub kind: String,
    pub source_id: Option<String>,
    pub dataset_id: Option<String>,
    pub run_id: Option<i64>,
    pub job_id: Option<i64>,
    pub message: String,
    #[schema(value_type = Option<Object>)]
    pub data: Option<serde_json::Value>,
}

pub const LEVELS: &[&str] = &["debug", "info", "warn", "error"];

/// `/v2/status/events?after=&level=&limit=` — oldest first, strictly
/// increasing ids (`ops.emit` hands ids out in commit order), so a poller
/// keeps the last id it saw and never misses or repeats an event.
pub async fn events(
    pool: &PgPool,
    after: i64,
    level: Option<&str>,
    source: Option<&str>,
    limit: i64,
) -> CoreResult<Vec<EventRecord>> {
    let min_level = match level {
        None => 0,
        Some(l) => LEVELS.iter().position(|x| *x == l).ok_or_else(|| {
            CoreError::bad_request(format!("level must be one of {}", LEVELS.join(", ")))
        })?,
    };
    let levels: Vec<&str> = LEVELS[min_level..].to_vec();
    Ok(sqlx::query_as(
        "SELECT id, at, level::text AS level, stage, kind, source_id, dataset_id, run_id, job_id, message, data
         FROM ops.run_events
         WHERE id > $1 AND level::text = ANY($2) AND ($3::text IS NULL OR source_id = $3)
         ORDER BY id LIMIT $4",
    )
    .bind(after)
    .bind(&levels)
    .bind(source)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?)
}

// ───────────────────────────── /v2/status/incidents ────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema, FromRow)]
pub struct Incident {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub opened_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    #[schema(value_type = Option<Object>)]
    pub detail: Option<serde_json::Value>,
}

/// `/v2/status/incidents?open=1`.
pub async fn incidents(pool: &PgPool, open_only: bool, limit: i64) -> CoreResult<Vec<Incident>> {
    Ok(sqlx::query_as(
        "SELECT id, kind, subject, opened_at, last_seen_at, closed_at, detail FROM ops.incidents
         WHERE NOT $1 OR closed_at IS NULL ORDER BY id DESC LIMIT $2",
    )
    .bind(open_only)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?)
}

// ───────────────────────────── /v2/health ──────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DbHealth {
    pub ok: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StoreHealth {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeepHealth {
    pub postgis: Option<String>,
    pub migration_version: Option<i64>,
    /// Seconds behind the primary; null on a primary.
    pub replica_lag_s: Option<f64>,
    pub admin_units: i64,
    pub datasets_published: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Health {
    /// ok | unavailable
    pub status: String,
    pub data_version: Option<String>,
    pub db: DbHealth,
    pub store: StoreHealth,
    pub grids_mapped: u32,
    pub uptime_s: u64,
    pub engine_version: String,
    pub git_rev: String,
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep: Option<DeepHealth>,
}

/// `SELECT 1` round trip in milliseconds, or None when Postgres is unreachable.
pub async fn db_latency_ms(pool: &PgPool) -> Option<f64> {
    let t = std::time::Instant::now();
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await;
    matches!(r, Ok(Ok(_))).then(|| (t.elapsed().as_secs_f64() * 1e5).round() / 100.0)
}

/// The `deep=1` part of `/v2/health`.
pub async fn deep_health(
    pool: &PgPool,
    admin_scheme: &str,
) -> CoreResult<(Option<String>, Option<i64>, Option<f64>, i64)> {
    Ok(sqlx::query_as(
        "SELECT postgis_lib_version(),
                (SELECT max(version) FROM _sqlx_migrations WHERE success),
                CASE WHEN pg_is_in_recovery()
                     THEN extract(epoch FROM now() - pg_last_xact_replay_timestamp())::float8 END,
                (SELECT count(*) FROM geo.units WHERE scheme_id = $1 AND valid_to IS NULL)",
    )
    .bind(admin_scheme)
    .fetch_one(pool)
    .await?)
}
