//! Validate → stage → publish for observation outputs, plus releases and the
//! review gate.
//!
//! Staging is not a separate table: rows are COPY'd straight into their
//! dataset's `data.observations` partition under a new, `staged`
//! `dataset_versions` row. A version is invisible to readers until a release
//! pins it, so staging costs one write and publishing is a pointer move.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

use crate::connector::{Fetched, ObsRow};
use crate::registry::Registry;

/// Class-A auto-publish gates (design doc §4.4). Exceeding one demotes the
/// version to human review instead of failing the run.
pub const MAX_CHANGED_SHARE: f64 = 0.01;
pub const MAX_REMOVED_SHARE: f64 = 0.01;

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub rows_added: i64,
    pub rows_removed: i64,
    pub rows_changed: i64,
    pub prev_rows: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    pub dataset_id: String,
    pub dsv: String,
    pub version_id: i64,
    /// published | in_review | unchanged
    pub outcome: &'static str,
    pub data_version: Option<String>,
    pub rows: i64,
    pub diff: Option<Diff>,
    pub note: String,
}

/// `dsv_` + 16 hex of the content digest.
#[must_use]
pub fn dsv_from_digest(d: &[u8; 32]) -> String {
    format!("dsv_{}", &hex::encode(d)[..16])
}

/// Deterministic content digest over the semantic row content (values,
/// statuses, locators, verbatim tokens). Artifact hashes are excluded on
/// purpose: a re-fetch with new bytes but identical values is no change.
#[must_use]
pub fn content_digest(rows: &[ObsRow]) -> [u8; 32] {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    idx.sort_unstable_by(|&a, &b| {
        let (x, y) = (&rows[a], &rows[b]);
        (&x.indicator_id, &x.geo_code, &x.period, &x.dims).cmp(&(
            &y.indicator_id,
            &y.geo_code,
            &y.period,
            &y.dims,
        ))
    });
    let mut h = Sha256::new();
    let mut line = String::with_capacity(256);
    for i in idx {
        let r = &rows[i];
        line.clear();
        let _ = writeln!(
            line,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.indicator_id,
            r.geo_code,
            r.period,
            r.dims,
            r.value.map(|v| v.to_string()).unwrap_or_default(),
            r.status,
            r.verbatim.as_deref().unwrap_or_default(),
            r.locator,
            r.method,
        );
        h.update(line.as_bytes());
    }
    h.finalize().into()
}

/// Structural validation before anything is written. Returns the admin level
/// of every geo code (looked up, never trusted from the connector).
pub async fn validate(
    pool: &PgPool,
    registry: &Registry,
    dataset_id: &str,
    rows: &[ObsRow],
) -> Result<HashMap<String, i16>> {
    let ds = registry
        .dataset(dataset_id)
        .with_context(|| format!("dataset {dataset_id:?} not in registry"))?;
    ensure!(!rows.is_empty(), "parser produced no rows for {dataset_id}");
    let indicators: HashMap<&str, &crate::registry::IndicatorSpec> =
        ds.indicators.iter().map(|i| (i.id.as_str(), i)).collect();

    let mut problems: Vec<String> = Vec::new();
    let mut keys: BTreeSet<(&str, &str, &str, &str)> = BTreeSet::new();
    for r in rows {
        let Some(ind) = indicators.get(r.indicator_id.as_str()) else {
            problems.push(format!(
                "indicator {:?} is not declared by dataset {dataset_id}",
                r.indicator_id
            ));
            continue;
        };
        if lanka_core::period::period_start(&r.period).is_err() {
            problems.push(format!("bad period {:?} for {}", r.period, r.indicator_id));
        }
        if let Some(v) = r.value
            && !v.is_finite()
        {
            problems.push(format!(
                "non-finite value for {} {} {}",
                r.indicator_id, r.geo_code, r.period
            ));
        }
        let dim_keys: Vec<&str> = r
            .dims
            .split(';')
            .filter(|s| !s.is_empty())
            .filter_map(|kv| kv.split('=').next())
            .collect();
        if dim_keys.iter().any(|k| !ind.dims.iter().any(|d| d == k)) {
            problems.push(format!(
                "dims {:?} not declared on {}",
                r.dims, r.indicator_id
            ));
        }
        if !keys.insert((&r.indicator_id, &r.geo_code, &r.period, &r.dims)) {
            problems.push(format!(
                "duplicate key {} {} {} {:?}",
                r.indicator_id, r.geo_code, r.period, r.dims
            ));
        }
        if problems.len() > 50 {
            break;
        }
    }

    let codes: Vec<&str> = rows
        .iter()
        .map(|r| r.geo_code.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let levels: Vec<(String, i16)> =
        sqlx::query_as("SELECT pcode, level FROM geo.admin_units WHERE pcode = ANY($1)")
            .bind(&codes)
            .fetch_all(pool)
            .await?;
    let levels: HashMap<String, i16> = levels.into_iter().collect();
    let missing: Vec<&str> = codes
        .iter()
        .copied()
        .filter(|c| !levels.contains_key(*c))
        .take(20)
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "{} geo code(s) not in geo.admin_units (load cod-ab-lka first?): {}",
            missing.len(),
            missing.join(", ")
        ));
    }
    if !problems.is_empty() {
        bail!(
            "validation failed for {dataset_id}:\n  - {}",
            problems.join("\n  - ")
        );
    }
    Ok(levels)
}

fn csv_field(out: &mut String, s: Option<&str>) {
    match s {
        None => {}
        Some(v) => {
            out.push('"');
            for c in v.chars() {
                if c == '"' {
                    out.push('"');
                }
                out.push(c);
            }
            out.push('"');
        }
    }
}

/// COPYs rows into the dataset's partition under `version_id`.
async fn copy_rows(
    conn: &mut PgConnection,
    dataset_id: &str,
    version_id: i64,
    rows: &[ObsRow],
    levels: &HashMap<String, i16>,
    fetched: &[Fetched],
    review: &str,
) -> Result<u64> {
    let mut copy = conn
        .copy_in_raw(
            "COPY data.observations (dataset_id, version_id, indicator_id, geo_code, geo_level, period,
               period_start, dims, value, status, artifact_id, locator, verbatim, method, confidence, review)
             FROM STDIN (FORMAT csv)",
        )
        .await?;
    let mut buf = String::with_capacity(1 << 20);
    let vid = version_id.to_string();
    for r in rows {
        let start = lanka_core::period::period_start(&r.period)?;
        let level = levels.get(&r.geo_code).copied().unwrap_or(9);
        csv_field(&mut buf, Some(dataset_id));
        buf.push(',');
        buf.push_str(&vid);
        buf.push(',');
        csv_field(&mut buf, Some(&r.indicator_id));
        buf.push(',');
        csv_field(&mut buf, Some(&r.geo_code));
        let _ = write!(buf, ",{level},");
        csv_field(&mut buf, Some(&r.period));
        let _ = write!(buf, ",{start},");
        csv_field(&mut buf, Some(&r.dims));
        buf.push(',');
        if let Some(v) = r.value {
            let _ = write!(buf, "{v}");
        }
        let _ = write!(buf, ",{},", r.status);
        if let Some(f) = fetched.get(r.artifact) {
            let _ = write!(buf, "{}", f.artifact_id);
        }
        buf.push(',');
        csv_field(&mut buf, Some(&r.locator.to_string()));
        buf.push(',');
        csv_field(&mut buf, r.verbatim.as_deref());
        let _ = write!(buf, ",{},", r.method);
        if let Some(c) = r.confidence {
            let _ = write!(buf, "{c}");
        }
        let _ = writeln!(buf, ",{review}");
        if buf.len() > (1 << 20) - 4096 {
            copy.send(buf.as_bytes()).await?;
            buf.clear();
        }
    }
    if !buf.is_empty() {
        copy.send(buf.as_bytes()).await?;
    }
    Ok(copy.finish().await?)
}

async fn diff_versions(
    conn: &mut PgConnection,
    dataset_id: &str,
    new_id: i64,
    old_id: i64,
) -> Result<Diff> {
    let (rows_added, rows_removed, rows_changed, prev_rows): (i64, i64, i64, i64) = sqlx::query_as(
        "WITH n AS (SELECT indicator_id, geo_code, period, dims, value, status FROM data.observations
                    WHERE dataset_id = $1 AND version_id = $2),
              o AS (SELECT indicator_id, geo_code, period, dims, value, status FROM data.observations
                    WHERE dataset_id = $1 AND version_id = $3)
         SELECT count(*) FILTER (WHERE o.indicator_id IS NULL),
                count(*) FILTER (WHERE n.indicator_id IS NULL),
                count(*) FILTER (WHERE n.indicator_id IS NOT NULL AND o.indicator_id IS NOT NULL
                                   AND (n.value IS DISTINCT FROM o.value OR n.status <> o.status)),
                count(o.indicator_id)
         FROM n FULL JOIN o ON n.indicator_id = o.indicator_id AND n.geo_code = o.geo_code
                           AND n.period = o.period AND n.dims = o.dims",
    )
    .bind(dataset_id)
    .bind(new_id)
    .bind(old_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Diff {
        rows_added,
        rows_removed,
        rows_changed,
        prev_rows,
    })
}

/// Current published version of a dataset (id, dsv), if any.
pub async fn current_version(
    conn: &mut PgConnection,
    dataset_id: &str,
) -> Result<Option<(i64, String)>> {
    Ok(
        sqlx::query_as("SELECT version_id, dsv FROM data.current_versions WHERE dataset_id = $1")
            .bind(dataset_id)
            .fetch_optional(&mut *conn)
            .await?,
    )
}

/// Creates the next release (`YYYYMMDD.N`): the current release's map with
/// `dataset_id` re-pointed to `version_id`. Supersedes the previous version
/// and writes the changelog row. Serialised by a transaction advisory lock.
pub async fn create_release(
    tx: &mut Transaction<'_, Postgres>,
    dataset_id: &str,
    version_id: i64,
    prev_version: Option<i64>,
    diff: Option<&Diff>,
    notes: &str,
    run_id: Option<i64>,
) -> Result<String> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('lanka-release'))")
        .execute(&mut **tx)
        .await?;
    let data_version: String = sqlx::query_scalar(
        "SELECT d || '.' || (coalesce(max(split_part(data_version, '.', 2)::int), 0) + 1)
         FROM (SELECT to_char(now() AT TIME ZONE 'UTC', 'YYYYMMDD') AS d) t
         LEFT JOIN data.releases r ON r.data_version LIKE t.d || '.%'
         GROUP BY d",
    )
    .fetch_one(&mut **tx)
    .await?;
    let prev_release: Option<String> =
        sqlx::query_scalar("SELECT data_version FROM data.current_release")
            .fetch_optional(&mut **tx)
            .await?;

    sqlx::query("INSERT INTO data.releases (data_version, notes, run_id) VALUES ($1, $2, $3)")
        .bind(&data_version)
        .bind(notes)
        .bind(run_id)
        .execute(&mut **tx)
        .await?;
    if let Some(prev) = &prev_release {
        sqlx::query(
            "INSERT INTO data.release_datasets (data_version, dataset_id, version_id)
             SELECT $1, dataset_id, version_id FROM data.release_datasets
             WHERE data_version = $2 AND dataset_id <> $3",
        )
        .bind(&data_version)
        .bind(prev)
        .bind(dataset_id)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query("INSERT INTO data.release_datasets (data_version, dataset_id, version_id) VALUES ($1, $2, $3)")
        .bind(&data_version)
        .bind(dataset_id)
        .bind(version_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE data.dataset_versions SET status = 'published', published_at = coalesce(published_at, now())
         WHERE id = $1",
    )
    .bind(version_id)
    .execute(&mut **tx)
    .await?;
    if let Some(prev) = prev_version.filter(|p| *p != version_id) {
        sqlx::query("UPDATE data.dataset_versions SET status = 'superseded' WHERE id = $1")
            .bind(prev)
            .execute(&mut **tx)
            .await?;
    }
    let (added, removed, changed) = diff.map_or((0, 0, 0), |d| {
        (d.rows_added, d.rows_removed, d.rows_changed)
    });
    let row_count: i64 =
        sqlx::query_scalar("SELECT row_count FROM data.dataset_versions WHERE id = $1")
            .bind(version_id)
            .fetch_one(&mut **tx)
            .await?;
    let summary = match diff {
        None => format!("first version: {row_count} rows"),
        Some(d) => format!(
            "+{} −{} ~{} rows (was {})",
            d.rows_added, d.rows_removed, d.rows_changed, d.prev_rows
        ),
    };
    sqlx::query(
        "INSERT INTO data.changelog (data_version, dataset_id, from_version_id, to_version_id,
            rows_added, rows_removed, rows_changed, summary)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&data_version)
    .bind(dataset_id)
    .bind(prev_version)
    .bind(version_id)
    .bind(if diff.is_none() { row_count } else { added })
    .bind(removed)
    .bind(changed)
    .bind(summary)
    .execute(&mut **tx)
    .await?;
    Ok(data_version)
}

/// Validate → stage → gate → publish one observation dataset.
#[allow(clippy::too_many_arguments)]
pub async fn publish_observations(
    pool: &PgPool,
    registry: &Registry,
    run_id: i64,
    dataset_id: &str,
    vintage: Option<&str>,
    rows: &[ObsRow],
    fetched: &[Fetched],
) -> Result<PublishResult> {
    let class = registry
        .dataset(dataset_id)
        .map(|d| d.publication_class.clone())
        .with_context(|| format!("dataset {dataset_id:?} not in registry"))?;
    let levels = validate(pool, registry, dataset_id, rows).await?;
    let digest = content_digest(rows);
    let dsv = dsv_from_digest(&digest);
    let n_rows = i64::try_from(rows.len())?;

    let mut tx = pool.begin().await?;
    sqlx::query("SELECT data.ensure_obs_partition($1)")
        .bind(dataset_id)
        .execute(&mut *tx)
        .await?;
    let current = current_version(&mut tx, dataset_id).await?;
    if let Some((vid, cur)) = &current
        && *cur == dsv
    {
        tx.rollback().await?;
        return Ok(PublishResult {
            dataset_id: dataset_id.to_owned(),
            dsv,
            version_id: *vid,
            outcome: "unchanged",
            data_version: None,
            rows: n_rows,
            diff: None,
            note: "content identical to the published version".to_owned(),
        });
    }
    let existing: Option<(i64, String)> =
        sqlx::query_as("SELECT id, status::text FROM data.dataset_versions WHERE dsv = $1")
            .bind(&dsv)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((vid, status)) = existing {
        tx.rollback().await?;
        return Ok(PublishResult {
            dataset_id: dataset_id.to_owned(),
            dsv,
            version_id: vid,
            outcome: if status == "in_review" {
                "in_review"
            } else {
                "unchanged"
            },
            data_version: None,
            rows: n_rows,
            diff: None,
            note: format!("identical content already exists as a {status} version"),
        });
    }

    let review = if class == "A" {
        "auto_accepted"
    } else {
        "pending"
    };
    let version_id: i64 = sqlx::query_scalar(
        "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status,
            publication_class, vintage, run_id, supersedes_id)
         VALUES ($1, $2, $3, $4, 'staged', $5::catalog.publication_class, $6, $7, $8) RETURNING id",
    )
    .bind(dataset_id)
    .bind(&dsv)
    .bind(digest.as_slice())
    .bind(n_rows)
    .bind(&class)
    .bind(vintage)
    .bind(run_id)
    .bind(current.as_ref().map(|c| c.0))
    .fetch_one(&mut *tx)
    .await?;

    let written = copy_rows(
        &mut tx, dataset_id, version_id, rows, &levels, fetched, review,
    )
    .await?;
    ensure!(
        written == rows.len() as u64,
        "COPY wrote {written} of {} rows",
        rows.len()
    );
    let used: BTreeSet<i64> = rows
        .iter()
        .filter_map(|r| fetched.get(r.artifact).map(|f| f.artifact_id))
        .collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_artifacts (version_id, artifact_id) SELECT $1, unnest($2::bigint[])",
    )
    .bind(version_id)
    .bind(used.into_iter().collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;

    let diff = match &current {
        Some((old, _)) => Some(diff_versions(&mut tx, dataset_id, version_id, *old).await?),
        None => None,
    };
    // Gate: class A publishes unless history moved more than the thresholds.
    #[allow(clippy::cast_precision_loss)]
    let gate_failure = diff.as_ref().and_then(|d| {
        let prev = d.prev_rows.max(1) as f64;
        if d.rows_changed as f64 / prev > MAX_CHANGED_SHARE {
            Some(format!(
                "{} historical values changed (> {:.0}%)",
                d.rows_changed,
                MAX_CHANGED_SHARE * 100.0
            ))
        } else if d.rows_removed as f64 / prev > MAX_REMOVED_SHARE {
            Some(format!(
                "{} rows removed (> {:.0}%)",
                d.rows_removed,
                MAX_REMOVED_SHARE * 100.0
            ))
        } else {
            None
        }
    });
    let validation =
        json!({ "rows": n_rows, "geo_codes": levels.len(), "gate_failure": gate_failure });
    sqlx::query("UPDATE data.dataset_versions SET validation = $2, diff = $3 WHERE id = $1")
        .bind(version_id)
        .bind(&validation)
        .bind(diff.as_ref().map(|d| json!(d)))
        .execute(&mut *tx)
        .await?;

    let (outcome, data_version, note) = if class == "A" && gate_failure.is_none() {
        let dv = create_release(
            &mut tx,
            dataset_id,
            version_id,
            current.as_ref().map(|c| c.0),
            diff.as_ref(),
            &format!("{dataset_id} → {dsv} (auto, class A)"),
            Some(run_id),
        )
        .await?;
        ("published", Some(dv), "auto-published (class A)".to_owned())
    } else {
        sqlx::query("UPDATE data.dataset_versions SET status = 'in_review' WHERE id = $1")
            .bind(version_id)
            .execute(&mut *tx)
            .await?;
        let why = gate_failure.unwrap_or_else(|| format!("class {class} requires human review"));
        (
            "in_review",
            None,
            format!("{why}; approve with `lanka review approve {dsv} --by <name>`"),
        )
    };
    tx.commit().await?;
    Ok(PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome,
        data_version,
        rows: n_rows,
        diff,
        note,
    })
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PendingVersion {
    pub dsv: String,
    pub dataset_id: String,
    pub publication_class: String,
    pub row_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub run_id: Option<i64>,
    pub diff: Option<serde_json::Value>,
    pub validation: Option<serde_json::Value>,
}

pub async fn pending_reviews(pool: &PgPool) -> Result<Vec<PendingVersion>> {
    Ok(sqlx::query_as(
        "SELECT dsv, dataset_id, publication_class::text AS publication_class, row_count, created_at, run_id,
                diff, validation
         FROM data.dataset_versions WHERE status = 'in_review' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?)
}

/// Human sign-off: approves every pending row of the version and publishes it.
pub async fn approve(
    pool: &PgPool,
    dsv: &str,
    reviewer: &str,
    note: Option<&str>,
) -> Result<PublishResult> {
    ensure!(!reviewer.trim().is_empty(), "reviewer name is required");
    let mut tx = pool.begin().await?;
    let (version_id, dataset_id, status, rows, diff): (i64, String, String, i64, Option<serde_json::Value>) =
        sqlx::query_as(
            "SELECT id, dataset_id, status::text, row_count, diff FROM data.dataset_versions WHERE dsv = $1 FOR UPDATE",
        )
        .bind(dsv)
        .fetch_optional(&mut *tx)
        .await?
        .with_context(|| format!("no dataset version {dsv}"))?;
    ensure!(status == "in_review", "{dsv} is {status}, not in_review");
    let reviewed = sqlx::query(
        "UPDATE data.observations SET review = 'approved'
         WHERE dataset_id = $1 AND version_id = $2 AND review = 'pending'",
    )
    .bind(&dataset_id)
    .bind(version_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE data.dataset_versions SET reviewed_by = $2, reviewed_at = now(), review_note = $3 WHERE id = $1",
    )
    .bind(version_id)
    .bind(reviewer)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    let current = current_version(&mut tx, &dataset_id).await?;
    let diff: Option<Diff> = diff.and_then(|d| {
        Some(Diff {
            rows_added: d.get("rows_added")?.as_i64()?,
            rows_removed: d.get("rows_removed")?.as_i64()?,
            rows_changed: d.get("rows_changed")?.as_i64()?,
            prev_rows: d.get("prev_rows")?.as_i64()?,
        })
    });
    let dv = create_release(
        &mut tx,
        &dataset_id,
        version_id,
        current.map(|c| c.0),
        diff.as_ref(),
        &format!("{dataset_id} → {dsv} (approved by {reviewer})"),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(PublishResult {
        dataset_id,
        dsv: dsv.to_owned(),
        version_id,
        outcome: "published",
        data_version: Some(dv),
        rows,
        diff,
        note: format!("{reviewed} rows approved by {reviewer}"),
    })
}

pub async fn reject(pool: &PgPool, dsv: &str, reviewer: &str, note: &str) -> Result<()> {
    let n = sqlx::query(
        "UPDATE data.dataset_versions SET status = 'rejected', reviewed_by = $2, reviewed_at = now(), review_note = $3
         WHERE dsv = $1 AND status = 'in_review'",
    )
    .bind(dsv)
    .bind(reviewer)
    .bind(note)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n == 1, "{dsv} is not in review");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ind: &str, geo: &str, period: &str, v: f64) -> ObsRow {
        ObsRow {
            indicator_id: ind.into(),
            geo_code: geo.into(),
            period: period.into(),
            dims: String::new(),
            value: Some(v),
            status: "reported",
            artifact: 0,
            locator: json!({"path": "$[1][0]"}),
            verbatim: Some(v.to_string()),
            method: "api",
            confidence: Some(1.0),
        }
    }

    #[test]
    fn digest_is_order_independent_and_value_sensitive() {
        let a = vec![row("x", "LK", "2020", 1.0), row("x", "LK", "2021", 2.0)];
        let b = vec![row("x", "LK", "2021", 2.0), row("x", "LK", "2020", 1.0)];
        assert_eq!(content_digest(&a), content_digest(&b));
        let c = vec![row("x", "LK", "2020", 1.0), row("x", "LK", "2021", 2.5)];
        assert_ne!(content_digest(&a), content_digest(&c));
        assert!(dsv_from_digest(&content_digest(&a)).starts_with("dsv_"));
        assert_eq!(dsv_from_digest(&content_digest(&a)).len(), 20);
    }

    #[test]
    fn csv_quoting() {
        let mut s = String::new();
        csv_field(&mut s, Some(r#"a "b", c"#));
        assert_eq!(s, r#""a ""b"", c""#);
        s.clear();
        csv_field(&mut s, Some(""));
        assert_eq!(s, r#""""#);
        s.clear();
        csv_field(&mut s, None);
        assert_eq!(s, "");
    }
}
