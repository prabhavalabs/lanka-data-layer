//! The release train (03 §8, ADR 0012).
//!
//! Staging never releases: an approved dataset version (class A through the
//! gate, B/C signed off) waits for the next train, which pins every approved
//! version into **one** new release:
//!
//! ```text
//! 1. derived recompute hook (placeholder until recipes exist, WP-06)
//! 2. lock (pg_advisory_xact_lock 'lanka-release')
//! 3. collect approved versions                        → none: no_change
//! 4. data.releases + release_datasets (previous map, re-pointed) + changelog;
//!    close superseded rows (vto); statuses → published / superseded
//! 5. rebuild serve.area_latest for the changed datasets
//! 6. COMMIT → NOTIFY lanka_release (servers swap their catalogue)
//! ```
//!
//! Exports, tiles and checks per changed dataset (steps 7–8 of 03 §8) are
//! enqueued here once those job kinds exist (WP-09). Every train is recorded
//! in `ops.trains` and the event feed, including the ones with nothing to do.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::publish::Diff;
use crate::run::{Event, Level, Stage, emit};

/// Train triggers recorded in `ops.trains.trigger`.
pub const TRIGGERS: &[&str] = &["schedule", "manual", "live", "test"];

#[derive(Debug, Clone, Serialize)]
pub struct ReleasedVersion {
    pub dataset_id: String,
    pub dsv: String,
    pub version_id: i64,
    pub previous_version_id: Option<i64>,
    pub rows_closed: u64,
    pub area_latest_rows: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainReport {
    pub train_id: i64,
    /// released | no_change
    pub outcome: &'static str,
    pub data_version: Option<String>,
    pub versions: Vec<ReleasedVersion>,
    pub recipes: serde_json::Value,
    pub duration_ms: u128,
}

/// Recomputes stale derived datasets in topological order before the cut
/// (03 §8 step 3). No recipes exist before WP-06, so this reports nothing
/// to do; a failing recipe will keep its previous version and never block
/// official data.
async fn recompute_derived(_pool: &PgPool) -> serde_json::Value {
    json!({ "recipes": 0, "recomputed": [], "note": "no recipes registered (WP-06)" })
}

/// (version id, dataset id, dataset key, seq, dsv, diff, kind)
type ApprovedRow = (
    i64,
    String,
    i32,
    i32,
    String,
    Option<serde_json::Value>,
    String,
);

/// Runs one train. `trigger` is one of [`TRIGGERS`].
pub async fn run_train(
    pool: &PgPool,
    trigger: &str,
    job_id: Option<i64>,
    note: Option<&str>,
) -> Result<TrainReport> {
    anyhow::ensure!(
        TRIGGERS.contains(&trigger),
        "unknown train trigger {trigger:?}"
    );
    let started = std::time::Instant::now();
    let train_id: i64 =
        sqlx::query_scalar("INSERT INTO ops.trains (job_id, trigger) VALUES ($1, $2) RETURNING id")
            .bind(job_id)
            .bind(trigger)
            .fetch_one(pool)
            .await?;
    let ev = |kind: &'static str, data| Event {
        kind: Some(kind),
        job_id,
        data: Some(data),
        ..Event::default()
    };
    emit(
        pool,
        Level::Info,
        Stage::Release,
        &format!("release train {train_id} started ({trigger})"),
        ev("train_started", json!({ "train_id": train_id })),
    )
    .await;

    let recipes = recompute_derived(pool).await;
    match cut(pool, train_id, note, &recipes).await {
        Ok((data_version, versions)) => {
            let outcome = if data_version.is_some() {
                "released"
            } else {
                "no_change"
            };
            let report = TrainReport {
                train_id,
                outcome,
                data_version,
                versions,
                recipes,
                duration_ms: started.elapsed().as_millis(),
            };
            if outcome == "no_change" {
                sqlx::query(
                    "UPDATE ops.trains SET outcome = 'no_change', finished_at = now(), recipes = $2
                     WHERE id = $1",
                )
                .bind(train_id)
                .bind(&report.recipes)
                .execute(pool)
                .await?;
            } else {
                sqlx::query("ANALYZE serve.area_latest")
                    .execute(pool)
                    .await?;
            }
            sqlx::query(
                "UPDATE ops.incidents SET closed_at = now() WHERE kind = 'train_failed' AND closed_at IS NULL",
            )
            .execute(pool)
            .await?;
            let msg = match &report.data_version {
                Some(dv) => format!(
                    "release {dv} cut by train {train_id}: {}",
                    report
                        .versions
                        .iter()
                        .map(|v| format!("{} → {}", v.dataset_id, v.dsv))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => format!("release train {train_id}: no approved versions, no_change"),
            };
            emit(
                pool,
                Level::Info,
                Stage::Release,
                &msg,
                ev(
                    if outcome == "released" {
                        "release_cut"
                    } else {
                        "train_no_change"
                    },
                    json!({ "train_id": train_id, "data_version": report.data_version,
                            "versions": report.versions.len() }),
                ),
            )
            .await;
            Ok(report)
        }
        Err(e) => {
            let text = format!("{e:#}");
            sqlx::query(
                "UPDATE ops.trains SET outcome = 'failed', finished_at = now(), error = $2, recipes = $3
                 WHERE id = $1",
            )
            .bind(train_id)
            .bind(&text)
            .bind(&recipes)
            .execute(pool)
            .await?;
            sqlx::query(
                "INSERT INTO ops.incidents (kind, subject, detail) VALUES ('train_failed', 'release', $1)
                 ON CONFLICT (kind, subject) WHERE closed_at IS NULL
                 DO UPDATE SET last_seen_at = now(), detail = EXCLUDED.detail",
            )
            .bind(json!({ "train_id": train_id, "error": text }))
            .execute(pool)
            .await?;
            emit(
                pool,
                Level::Error,
                Stage::Release,
                &format!("release train {train_id} failed: {text}"),
                ev("train_failed", json!({ "train_id": train_id })),
            )
            .await;
            Err(e)
        }
    }
}

/// Steps 2–6 in one transaction. Returns the new data_version (None when
/// nothing was approved) and what it released.
async fn cut(
    pool: &PgPool,
    train_id: i64,
    note: Option<&str>,
    recipes: &serde_json::Value,
) -> Result<(Option<String>, Vec<ReleasedVersion>)> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('lanka-release'))")
        .execute(&mut *tx)
        .await?;
    let approved: Vec<ApprovedRow> = sqlx::query_as(
        "SELECT v.id, v.dataset_id, d.key, v.seq, v.dsv, v.diff, d.kind
         FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
         WHERE v.status = 'approved' ORDER BY v.dataset_id FOR UPDATE OF v",
    )
    .fetch_all(&mut *tx)
    .await?;
    if approved.is_empty() {
        tx.rollback().await?;
        return Ok((None, Vec::new()));
    }

    let data_version: String = sqlx::query_scalar(
        "SELECT d || '.' || (coalesce(max(split_part(data_version, '.', 2)::int), 0) + 1)
         FROM (SELECT to_char(now() AT TIME ZONE 'UTC', 'YYYYMMDD') AS d) t
         LEFT JOIN data.releases r ON r.data_version LIKE t.d || '.%'
         GROUP BY d",
    )
    .fetch_one(&mut *tx)
    .await?;
    let prev_release: Option<String> =
        sqlx::query_scalar("SELECT data_version FROM data.current_release")
            .fetch_optional(&mut *tx)
            .await?;
    let notes = note.map_or_else(
        || {
            format!(
                "train {train_id}: {}",
                approved
                    .iter()
                    .map(|a| format!("{} → {}", a.1, a.4))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        str::to_owned,
    );
    sqlx::query("INSERT INTO data.releases (data_version, notes, train_id) VALUES ($1, $2, $3)")
        .bind(&data_version)
        .bind(&notes)
        .bind(train_id)
        .execute(&mut *tx)
        .await?;
    let changed: Vec<&str> = approved.iter().map(|a| a.1.as_str()).collect();
    if let Some(prev) = &prev_release {
        sqlx::query(
            "INSERT INTO data.release_datasets (data_version, dataset_id, version_id)
             SELECT $1, dataset_id, version_id FROM data.release_datasets
             WHERE data_version = $2 AND dataset_id <> ALL($3)",
        )
        .bind(&data_version)
        .bind(prev)
        .bind(&changed)
        .execute(&mut *tx)
        .await?;
    }

    let mut released = Vec::with_capacity(approved.len());
    for (version_id, dataset_id, dataset_key, seq, dsv, diff, kind) in approved {
        let previous: Option<i64> = match &prev_release {
            Some(p) => {
                sqlx::query_scalar(
                    "SELECT version_id FROM data.release_datasets WHERE data_version = $1 AND dataset_id = $2",
                )
                .bind(p)
                .bind(&dataset_id)
                .fetch_optional(&mut *tx)
                .await?
            }
            None => None,
        };
        let diff: Option<Diff> = diff.and_then(|d| serde_json::from_value(d).ok());
        let rows_closed = release_one(
            &mut tx,
            &data_version,
            &dataset_id,
            dataset_key,
            version_id,
            seq,
            previous,
            diff.as_ref(),
        )
        .await
        .with_context(|| format!("releasing {dataset_id} {dsv}"))?;
        let area_latest_rows = if kind == "indicators" {
            sqlx::query_scalar("SELECT serve.refresh_area_latest($1, $2)")
                .bind(dataset_key)
                .bind(seq)
                .fetch_one(&mut *tx)
                .await?
        } else {
            0
        };
        released.push(ReleasedVersion {
            dataset_id,
            dsv,
            version_id,
            previous_version_id: previous,
            rows_closed,
            area_latest_rows,
        });
    }
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let n = released.len() as i32;
    sqlx::query(
        "UPDATE ops.trains SET outcome = 'released', finished_at = now(), data_version = $2, versions = $3,
            recipes = $4, detail = $5 WHERE id = $1",
    )
    .bind(train_id)
    .bind(&data_version)
    .bind(n)
    .bind(recipes)
    .bind(json!({ "released": released, "previous_release": prev_release }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((Some(data_version), released))
}

/// Pins one approved version into the release being written: applies its
/// closes (the only UPDATE observations ever see), re-points the dataset,
/// supersedes the previous version and writes the changelog row.
#[allow(clippy::too_many_arguments)]
async fn release_one(
    conn: &mut PgConnection,
    data_version: &str,
    dataset_id: &str,
    dataset_key: i32,
    version_id: i64,
    seq: i32,
    previous: Option<i64>,
    diff: Option<&Diff>,
) -> Result<u64> {
    let closed = sqlx::query(
        "UPDATE data.observations o SET vto = $3 - 1
         FROM data.version_closes c
         WHERE c.version_id = $1 AND o.dataset_key = $2 AND o.obs_id = c.obs_id AND o.vto = 2147483647",
    )
    .bind(version_id)
    .bind(dataset_key)
    .bind(seq)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        + crate::features::close_released(conn, version_id, dataset_key, seq).await?;
    sqlx::query("DELETE FROM data.version_closes WHERE version_id = $1")
        .bind(version_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO data.release_datasets (data_version, dataset_id, version_id) VALUES ($1, $2, $3)",
    )
    .bind(data_version)
    .bind(dataset_id)
    .bind(version_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE data.dataset_versions SET status = 'published', published_at = coalesce(published_at, now())
         WHERE id = $1",
    )
    .bind(version_id)
    .execute(&mut *conn)
    .await?;
    if let Some(prev) = previous.filter(|p| *p != version_id) {
        sqlx::query("UPDATE data.dataset_versions SET status = 'superseded' WHERE id = $1")
            .bind(prev)
            .execute(&mut *conn)
            .await?;
    }
    let row_count: i64 =
        sqlx::query_scalar("SELECT row_count FROM data.dataset_versions WHERE id = $1")
            .bind(version_id)
            .fetch_one(&mut *conn)
            .await?;
    let first = diff.is_none_or(|d| d.prev_rows == 0);
    let (added, removed, changed) = diff.map_or((0, 0, 0), |d| {
        (
            d.rows_added,
            d.rows_removed,
            d.rows_changed + d.rows_restated,
        )
    });
    let summary = match diff {
        Some(d) if d.prev_rows > 0 => format!(
            "+{} −{} ~{} rows (was {}; {} restated)",
            d.rows_added, d.rows_removed, d.rows_changed, d.prev_rows, d.rows_restated
        ),
        _ => format!("first version: {row_count} rows"),
    };
    sqlx::query(
        "INSERT INTO data.changelog (data_version, dataset_id, from_version_id, to_version_id,
            rows_added, rows_removed, rows_changed, summary)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(data_version)
    .bind(dataset_id)
    .bind(previous)
    .bind(version_id)
    .bind(if first { row_count } else { added })
    .bind(removed)
    .bind(changed)
    .bind(summary)
    .execute(&mut *conn)
    .await?;
    Ok(closed)
}

/// Rebuilds `serve.area_latest` for every dataset of the current release
/// (`lanka serve rebuild`): the projections are disposable.
pub async fn rebuild_serve(pool: &PgPool) -> Result<i64> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('lanka-release'))")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM serve.area_latest")
        .execute(&mut *tx)
        .await?;
    let n: Option<i64> = sqlx::query_scalar(
        "SELECT sum(serve.refresh_area_latest(dataset_key, seq))::int8 FROM data.current_versions",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    sqlx::query("ANALYZE serve.area_latest")
        .execute(pool)
        .await?;
    Ok(n.unwrap_or(0))
}
