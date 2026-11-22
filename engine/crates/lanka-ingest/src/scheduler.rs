//! The scheduler tick of `lanka worker` (03 §4, ADR 0011): everything about
//! *when* is data — registry cadence, `schedule` overrides and release
//! calendars (synced into `catalog.sources` / `ops.release_calendar`) plus
//! the run history, combined by the `ops.source_freshness` view.
//!
//! One tick (every 60 s, leader only):
//! 1. re-queue jobs abandoned by a dead worker;
//! 2. enqueue an ingest for every schedulable source that is due (respecting
//!    its politeness window), deduplicated against live jobs;
//! 3. enqueue the daily release train once its slot (default 20:30 UTC) has
//!    passed, and an intraday train (≤ 1/h) when a `live` dataset has an
//!    approved version;
//! 4. open and close incidents for persistent conditions.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, NaiveTime, Timelike, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;

use crate::jobs::{self, NewJob};
use crate::run::{Event, Level, Stage, emit};

/// Scheduler settings (from the environment, see `TickConfig::from_env`).
#[derive(Debug, Clone)]
pub struct TickConfig {
    /// Daily train time, UTC (`LANKA_TRAIN_AT`, default 20:30 = 02:00 Colombo).
    pub train_at: NaiveTime,
    /// Intraday trains for `live` datasets (`LANKA_LIVE_TRAINS`, default on).
    pub live_trains: bool,
    /// Local input directories by source (`LANKA_LOCAL_INPUTS=src=/dir,…`),
    /// passed to scheduled runs as `--from` (sources that cannot fetch yet).
    pub local_inputs: HashMap<String, PathBuf>,
    /// Blob store disk usage (percent) measured by the worker this tick.
    pub store_pct: Option<f32>,
}

impl Default for TickConfig {
    fn default() -> Self {
        Self {
            train_at: NaiveTime::from_hms_opt(20, 30, 0).unwrap_or_default(),
            live_trains: true,
            local_inputs: HashMap::new(),
            store_pct: None,
        }
    }
}

impl TickConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if let Some(t) = std::env::var("LANKA_TRAIN_AT")
            .ok()
            .and_then(|v| NaiveTime::parse_from_str(v.trim(), "%H:%M").ok())
        {
            c.train_at = t;
        }
        if let Ok(v) = std::env::var("LANKA_LIVE_TRAINS") {
            c.live_trains = !matches!(v.trim(), "0" | "false" | "off" | "no");
        }
        c.local_inputs =
            parse_local_inputs(&std::env::var("LANKA_LOCAL_INPUTS").unwrap_or_default());
        c
    }
}

/// `src=/dir,other=/dir2` → map.
#[must_use]
pub fn parse_local_inputs(v: &str) -> HashMap<String, PathBuf> {
    v.split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, d)| (k.trim().to_owned(), PathBuf::from(d.trim())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TickReport {
    pub reaped: Vec<i64>,
    /// (source, job id) of ingests enqueued this tick.
    pub enqueued: Vec<(String, i64)>,
    pub train_job: Option<i64>,
    pub incidents_opened: usize,
    pub incidents_closed: usize,
}

/// (source, run_after, calendar-triggered)
type DueRow = (String, DateTime<Utc>, bool);

/// Runs one scheduler tick.
pub async fn tick(pool: &PgPool, cfg: &TickConfig) -> Result<TickReport> {
    let mut rep = TickReport {
        reaped: jobs::reap_abandoned(pool).await?,
        ..TickReport::default()
    };

    // ── due ingests ──
    let due: Vec<DueRow> = sqlx::query_as(
        "SELECT f.source_id,
                ops.window_start(f.check_window,
                    coalesce((SELECT spec->>'timezone' FROM catalog.profile), 'UTC'), now()),
                (f.expected_date IS NOT NULL AND f.expected_date <= current_date
                 AND (f.last_run_at IS NULL OR f.last_run_at < f.expected_date))
         FROM ops.source_freshness f
         WHERE f.schedulable AND NOT f.paused AND f.registry_status NOT IN ('retired', 'blocked')
           AND f.next_check_at IS NOT NULL AND f.next_check_at <= now()
           AND NOT EXISTS (SELECT 1 FROM ops.jobs j
                           WHERE j.dedupe_key = 'ingest:' || f.source_id AND j.state IN ('queued', 'running'))
         ORDER BY f.next_check_at, f.source_id",
    )
    .fetch_all(pool)
    .await?;
    for (source, run_after, calendar) in due {
        let payload = cfg
            .local_inputs
            .get(&source)
            .map_or_else(|| json!({}), |d| json!({ "from": d }));
        let mut job = NewJob::ingest(
            &source,
            if calendar {
                "upstream_change"
            } else {
                "schedule"
            },
            payload,
        );
        job.run_after = Some(run_after);
        let (id, created) = jobs::enqueue(pool, &job).await?;
        if created {
            rep.enqueued.push((source, id));
        }
    }

    // ── release trains ──
    let now = Utc::now();
    let slot_today = now.date_naive().and_time(cfg.train_at).and_utc();
    if now >= slot_today {
        let slot = format!("daily:{}", now.format("%Y%m%d"));
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM ops.jobs WHERE dedupe_key = $1)")
                .bind(format!("release:{slot}"))
                .fetch_one(pool)
                .await?;
        if !exists {
            let (id, _) =
                jobs::enqueue(pool, &NewJob::release(&slot, "schedule", "schedule")).await?;
            rep.train_job = Some(id);
        }
    }
    if cfg.live_trains && rep.train_job.is_none() {
        let live_pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
                            WHERE v.status = 'approved' AND d.live)
                AND NOT EXISTS (SELECT 1 FROM ops.trains WHERE started_at > now() - interval '1 hour')
                AND NOT EXISTS (SELECT 1 FROM ops.jobs WHERE kind = 'release' AND state IN ('queued', 'running'))",
        )
        .fetch_one(pool)
        .await?;
        if live_pending {
            let slot = format!("live:{}{:02}", now.format("%Y%m%d"), now.hour());
            let (id, _) = jobs::enqueue(pool, &NewJob::release(&slot, "schedule", "live")).await?;
            rep.train_job = Some(id);
        }
    }

    // ── incidents ──
    let (o, c) = sync_incidents(pool, cfg).await?;
    rep.incidents_opened = o;
    rep.incidents_closed = c;
    Ok(rep)
}

/// (kind, subject, detail) of a condition that holds right now.
type Condition = (String, String, serde_json::Value);

/// Opens incidents for conditions that hold and closes the ones that no
/// longer do. Kinds managed here: source_failing, expected_missing,
/// default_partition_rows, disk_pressure, backup_stale.
pub async fn sync_incidents(pool: &PgPool, cfg: &TickConfig) -> Result<(usize, usize)> {
    let mut holding: Vec<Condition> = sqlx::query_as(
        "SELECT CASE state WHEN 'failing' THEN 'source_failing' ELSE 'expected_missing' END, source_id,
                jsonb_build_object('state', state, 'consecutive_failures', consecutive_failures,
                                   'last_error', last_error, 'expected_edition', expected_edition,
                                   'expected_date', expected_date, 'priority', priority)
         FROM ops.source_freshness WHERE state IN ('failing', 'expected_missing')",
    )
    .fetch_all(pool)
    .await?;
    let default_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT 1 FROM data.observations_default LIMIT 1000) t",
    )
    .fetch_one(pool)
    .await?;
    if default_rows > 0 {
        holding.push((
            "default_partition_rows".into(),
            "data.observations_default".into(),
            json!({ "rows_at_least": default_rows }),
        ));
    }
    if let Some(p) = cfg.store_pct
        && p > 85.0
    {
        holding.push((
            "disk_pressure".into(),
            "store".into(),
            json!({ "store_pct": p }),
        ));
    }
    let stale: Vec<(String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT k, (SELECT max(at) FROM ops.backups b WHERE b.kind = k AND b.ok)
         FROM unnest(ARRAY['pg', 'store']) AS k
         WHERE EXISTS (SELECT 1 FROM ops.backups)",
    )
    .fetch_all(pool)
    .await?;
    for (kind, last) in stale {
        if last.is_none_or(|t| Utc::now() - t > chrono::Duration::hours(36)) {
            holding.push(("backup_stale".into(), kind, json!({ "last_ok_at": last })));
        }
    }

    let managed = [
        "source_failing",
        "expected_missing",
        "default_partition_rows",
        "disk_pressure",
        "backup_stale",
    ];
    let mut opened = 0;
    for (kind, subject, detail) in &holding {
        let new: Option<bool> = sqlx::query_scalar(
            "INSERT INTO ops.incidents (kind, subject, detail) VALUES ($1, $2, $3)
             ON CONFLICT (kind, subject) WHERE closed_at IS NULL
             DO UPDATE SET last_seen_at = now(), detail = EXCLUDED.detail
             RETURNING (xmax = 0)",
        )
        .bind(kind)
        .bind(subject)
        .bind(detail)
        .fetch_optional(pool)
        .await?;
        if new == Some(true) {
            opened += 1;
            emit(
                pool,
                Level::Error,
                Stage::Incident,
                &format!("incident opened: {kind} {subject}"),
                Event {
                    kind: Some("incident_opened"),
                    source_id: matches!(kind.as_str(), "source_failing" | "expected_missing")
                        .then_some(subject.as_str()),
                    data: Some(detail.clone()),
                    ..Event::default()
                },
            )
            .await;
        }
    }
    let kinds: Vec<&str> = holding.iter().map(|h| h.0.as_str()).collect();
    let subjects: Vec<&str> = holding.iter().map(|h| h.1.as_str()).collect();
    let closed: Vec<(i64, String, String)> = sqlx::query_as(
        "UPDATE ops.incidents i SET closed_at = now()
         WHERE closed_at IS NULL AND kind = ANY($1)
           AND NOT EXISTS (SELECT 1 FROM unnest($2::text[], $3::text[]) AS h(k, s)
                           WHERE h.k = i.kind AND h.s = i.subject)
         RETURNING id, kind, subject",
    )
    .bind(&managed[..])
    .bind(&kinds)
    .bind(&subjects)
    .fetch_all(pool)
    .await?;
    for (id, kind, subject) in &closed {
        emit(
            pool,
            Level::Info,
            Stage::Incident,
            &format!("incident {id} closed: {kind} {subject}"),
            Event {
                kind: Some("incident_closed"),
                data: Some(json!({ "incident_id": id, "kind": kind, "subject": subject })),
                ..Event::default()
            },
        )
        .await;
    }
    Ok((opened, closed.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_inputs_parse() {
        let m = parse_local_inputs("hdx-cod-ab-lka=/data/layers, x = /y ,bad");
        assert_eq!(m.len(), 2);
        assert_eq!(m["hdx-cod-ab-lka"], PathBuf::from("/data/layers"));
        assert_eq!(m["x"], PathBuf::from("/y"));
        assert!(parse_local_inputs("").is_empty());
    }
}
