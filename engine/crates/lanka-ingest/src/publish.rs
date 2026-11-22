//! Validate → stage → publish for observation outputs, plus releases and the
//! review gate, on the range-versioned model (05 §2–§3, ADR 0004).
//!
//! A version is a dataset-local sequence number `seq`. Staging version *n*
//! COPYs the connector's rows into an unlogged-per-session temp table, diffs
//! them set-wise against the open rows of the released version and inserts
//! only new or changed rows (`vfrom = n`, `vto = open`); the rows it will
//! close are recorded in `data.version_closes`. Unchanged rows are not
//! touched. Releasing *n* closes those rows (`vto = n - 1`) — the only
//! UPDATE the table ever sees. Rejecting *n* deletes its rows and closes.
//!
//! The dsv is `dsv(schema_hash, multiset_hash, rows)` over canonical rows
//! (lanka-core `dsv`), so it is order- and path-independent. It is computed
//! incrementally: the released version's multiset plus the hashes of new or
//! rewritten rows minus those of the rows they close, so only the delta is
//! hashed and an identical re-run hashes and writes nothing.
//!
//! Revisions: a row whose value differs from the released row of the same
//! key is stored with `status = revised` unless the source states another
//! status (02 §5). `revised` is derived from history, so it hashes as
//! `normal` and the dsv stays a function of the delivered content only.

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result, bail, ensure};
use lanka_core::dims::Dims;
use lanka_core::dsv::{CanonicalRow, MultisetHash, canonical_json, dsv, schema_hash};
use lanka_core::period::{Freq, Period};
use lanka_core::{
    Basis, Flags, GeoCode, IndicatorClass, Method, Profile, Review, Status, VTO_OPEN,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

use crate::connector::ObsRow;
use crate::copy_bin::BinCopy;
use crate::registry::Registry;

/// Class-A auto-publish gates (design doc §4.4). Exceeding one demotes the
/// version to human review instead of failing the run.
pub const MAX_CHANGED_SHARE: f64 = 0.01;
pub const MAX_REMOVED_SHARE: f64 = 0.01;

/// Bits of `obs_id` below the dataset key (`obs_id = dataset_key << 40 | seq`).
pub const OBS_KEY_SHIFT: u32 = 40;

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Diff {
    pub rows_added: i64,
    pub rows_removed: i64,
    /// Same key, different value/interval/basis/status/flags.
    pub rows_changed: i64,
    /// Same value, new locator or verbatim token (a restatement).
    pub rows_restated: i64,
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
    /// Rows physically written for this version (new or changed only).
    pub rows_written: i64,
    /// Canonical rows hashed to compute the dsv (the delta, not the dataset).
    pub rows_hashed: u64,
    pub diff: Option<Diff>,
    pub note: String,
}

// ───────────────────────────── identity ────────────────────────────────────

/// Descriptor lines of an indicator dataset's schema, from the synced
/// catalogue (so staging and the migration backfill agree).
async fn schema_lines(conn: &mut PgConnection, dataset_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String, Option<String>, i16, String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, unit_code, unit_mult, freq::text, dim_defs FROM catalog.indicators WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, unit, mult, freq, dims)| {
            format!(
                "indicator\u{1f}{id}\u{1f}{}\u{1f}{mult}\u{1f}{freq}\u{1f}{}",
                unit.unwrap_or_default(),
                canonical_json(&dims)
            )
        })
        .collect())
}

/// Canonical bytes of one admin unit of a boundary set.
#[must_use]
pub fn unit_row_bytes(
    code: &str,
    level: i16,
    names: [Option<&str>; 3],
    parent: Option<&str>,
    geom_sha256_hex: &str,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(128);
    for (i, part) in [
        code,
        &level.to_string(),
        names[0].unwrap_or_default(),
        names[1].unwrap_or_default(),
        names[2].unwrap_or_default(),
        parent.unwrap_or_default(),
        geom_sha256_hex,
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            b.push(0x1f);
        }
        b.extend_from_slice(part.as_bytes());
    }
    b
}

#[must_use]
pub fn boundary_schema(boundary_set: &str, scheme: &str) -> [u8; 32] {
    schema_hash(vec![format!(
        "boundaries\u{1f}{boundary_set}\u{1f}{scheme}"
    )])
}

// ───────────────────────────── validation ──────────────────────────────────

/// A row resolved to surrogate keys and canonical codes.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub indicator_key: i32,
    pub geo_key: i32,
    pub geo_level: i16,
    pub dims: Dims,
    pub period: Period,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct IndicatorRow {
    id: String,
    key: i32,
    freq: String,
    class: String,
    dim_defs: serde_json::Value,
}

/// Structural and semantic validation before anything is written (02 §3–§5):
/// declared indicator, canonical period of its frequency, finite values and
/// intervals, codelist-backed dims, `basis` allowed by the indicator class,
/// intervals for estimates, null values for missing/suppressed, unique keys,
/// geographies that exist. Returns the resolved rows, in input order.
pub async fn validate(
    pool: &PgPool,
    registry: &Registry,
    profile: &Profile,
    dataset_id: &str,
    rows: &[ObsRow],
) -> Result<Vec<Prepared>> {
    registry
        .dataset(dataset_id)
        .with_context(|| format!("dataset {dataset_id:?} not in registry"))?;
    ensure!(!rows.is_empty(), "parser produced no rows for {dataset_id}");
    let inds: Vec<IndicatorRow> = sqlx::query_as(
        "SELECT id, key, freq::text, class, dim_defs FROM catalog.indicators WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(pool)
    .await?;
    let inds: HashMap<&str, &IndicatorRow> = inds.iter().map(|i| (i.id.as_str(), i)).collect();
    // dimension id → codelist, per indicator
    let mut dim_codelists: HashMap<&str, HashMap<String, String>> = HashMap::new();
    for i in inds.values() {
        let defs = i.dim_defs.as_array().map(Vec::as_slice).unwrap_or_default();
        dim_codelists.insert(
            i.id.as_str(),
            defs.iter()
                .filter_map(|d| {
                    Some((
                        d["id"].as_str()?.to_owned(),
                        d["codelist"].as_str()?.to_owned(),
                    ))
                })
                .collect(),
        );
    }
    let codes: Vec<(String, String)> =
        sqlx::query_as("SELECT codelist_id, code FROM catalog.codes")
            .fetch_all(pool)
            .await?;
    let codes: BTreeSet<(String, String)> = codes.into_iter().collect();
    let codelists: BTreeSet<&str> = codes.iter().map(|(c, _)| c.as_str()).collect();

    let mut problems: Vec<String> = Vec::new();
    let mut keys: BTreeSet<(&str, &str, &str, &str)> = BTreeSet::new();
    let mut partial: Vec<Option<(i32, Dims, Period, String)>> = Vec::with_capacity(rows.len());
    for r in rows {
        if problems.len() > 50 {
            break;
        }
        let at = || {
            format!(
                "{} {} {} {:?}",
                r.indicator_id, r.geo_code, r.period, r.dims
            )
        };
        let Some(ind) = inds.get(r.indicator_id.as_str()) else {
            problems.push(format!(
                "indicator {:?} is not declared by dataset {dataset_id}",
                r.indicator_id
            ));
            partial.push(None);
            continue;
        };
        let Some(freq) = ind.freq.chars().next().and_then(Freq::from_char) else {
            problems.push(format!("indicator {} has no frequency", ind.id));
            partial.push(None);
            continue;
        };
        let period = match Period::parse(freq, &r.period, &profile.calendars) {
            Ok(p) if p.render(&profile.calendars) == r.period => Some(p),
            Ok(p) => {
                problems.push(format!(
                    "{}: period {:?} is not canonical (use {:?})",
                    at(),
                    r.period,
                    p.render(&profile.calendars)
                ));
                None
            }
            Err(e) => {
                problems.push(format!("{}: {e}", at()));
                None
            }
        };
        for (name, v) in [("value", r.value), ("lo", r.lo), ("hi", r.hi)] {
            if v.is_some_and(|v| !v.is_finite()) {
                problems.push(format!("{}: non-finite {name}", at()));
            }
        }
        if let (Some(lo), Some(hi)) = (r.lo, r.hi)
            && (lo > hi || r.value.is_some_and(|v| v < lo || v > hi))
        {
            problems.push(format!(
                "{}: interval [{lo}, {hi}] does not contain the value",
                at()
            ));
        }
        if r.status == Status::Revised {
            problems.push(format!(
                "{}: status revised is assigned by the pipeline from version history; \
                 deliver the source status",
                at()
            ));
        }
        if r.status.requires_null() && r.value.is_some() {
            problems.push(format!(
                "{}: status {} requires a null value",
                at(),
                r.status
            ));
        }
        let class = IndicatorClass::parse(&ind.class);
        if let Some(c) = class
            && !c.allows(r.basis)
        {
            problems.push(format!(
                "{}: basis {} is not allowed for a {} indicator",
                at(),
                r.basis,
                c.as_str()
            ));
        }
        if class == Some(IndicatorClass::Estimated) && (r.lo.is_none() || r.hi.is_none()) {
            problems.push(format!("{}: estimated values need lo/hi", at()));
        }
        let dims = match Dims::parse(&r.dims) {
            Ok(d) => {
                let decl = &dim_codelists[r.indicator_id.as_str()];
                for (k, v) in d.iter() {
                    match decl.get(k) {
                        None => problems.push(format!("{}: dimension {k:?} not declared", at())),
                        Some(cl) if !codelists.contains(cl.as_str()) => {
                            problems.push(format!(
                                "{}: codelist {cl:?} is not in registry/codelists",
                                at()
                            ));
                        }
                        Some(cl) if !codes.contains(&(cl.clone(), v.to_owned())) => {
                            problems
                                .push(format!("{}: code {v:?} is not in codelist {cl:?}", at()));
                        }
                        Some(_) => {}
                    }
                }
                if d.canonical() != r.dims {
                    problems.push(format!(
                        "{}: dims not canonical (use {:?})",
                        at(),
                        d.canonical()
                    ));
                }
                Some(d)
            }
            Err(e) => {
                problems.push(format!("{}: {e}", at()));
                None
            }
        };
        if !keys.insert((&r.indicator_id, &r.geo_code, &r.period, &r.dims)) {
            problems.push(format!("duplicate key {}", at()));
        }
        partial.push(match (period, dims) {
            (Some(p), Some(d)) => Some((ind.key, d, p, r.geo_code.clone())),
            _ => None,
        });
    }

    let mut wanted: Vec<String> = Vec::new();
    for r in rows {
        match profile.admin.parse_geo(&r.geo_code) {
            Ok(g) => wanted.push(g.to_code()),
            Err(e) => {
                if problems.len() <= 50 {
                    problems.push(e.to_string());
                }
            }
        }
    }
    wanted.sort_unstable();
    wanted.dedup();
    let units: Vec<(String, i32, i16)> = sqlx::query_as(
        "SELECT code, key, level FROM geo.units WHERE code = ANY($1) AND valid_to IS NULL",
    )
    .bind(&wanted)
    .fetch_all(pool)
    .await?;
    let units: HashMap<String, (i32, i16)> =
        units.into_iter().map(|(c, k, l)| (c, (k, l))).collect();
    let missing: Vec<&str> = wanted
        .iter()
        .map(String::as_str)
        .filter(|c| !units.contains_key(*c))
        .take(20)
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "{} geo code(s) not in geo.units (load the boundaries first?): {}",
            wanted.iter().filter(|c| !units.contains_key(*c)).count(),
            missing.join(", ")
        ));
    }
    if !problems.is_empty() {
        bail!(
            "validation failed for {dataset_id}:\n  - {}",
            problems.join("\n  - ")
        );
    }
    Ok(partial
        .into_iter()
        .map(|p| {
            let (indicator_key, dims, period, geo) = p.expect("validated");
            let code = match profile.admin.parse_geo(&geo) {
                Ok(GeoCode::Admin(p)) => p.to_string(),
                Ok(g) => g.to_code(),
                Err(_) => geo,
            };
            let (geo_key, geo_level) = units[&code];
            Prepared {
                indicator_key,
                geo_key,
                geo_level,
                dims,
                period,
            }
        })
        .collect())
}

fn canonical(r: &ObsRow) -> CanonicalRow<'_> {
    CanonicalRow {
        indicator_id: &r.indicator_id,
        geo_code: &r.geo_code,
        period: &r.period,
        dims: &r.dims,
        value: r.value,
        lo: r.lo,
        hi: r.hi,
        basis: r.basis,
        status: r.status,
        flags: r.flags,
        verbatim: r.verbatim.as_deref(),
        locator: Some(&r.locator),
    }
}

// ───────────────────────────── staging ─────────────────────────────────────

/// Bytes per binary-COPY chunk sent to the server.
const COPY_CHUNK: usize = 4 << 20;

/// Binary-COPYs the prepared rows into a session temp table (`obs_stage`).
/// Temp tables are never WAL-logged and vanish at commit (05 §2.1's
/// `stage.*` unlogged tables, per session).
#[allow(clippy::too_many_arguments)]
async fn copy_stage(
    conn: &mut PgConnection,
    rows: &[ObsRow],
    prep: &[Prepared],
    dims_keys: &HashMap<String, i32>,
    artifact_ids: &[i64],
    extract_ids: &[i64],
    review: Review,
) -> Result<u64> {
    sqlx::query(
        "CREATE TEMP TABLE obs_stage (
            rn int8, indicator_key int4, geo_key int4, dims_key int4, freq \"char\", time_start date,
            geo_level int2, value float8, lo float8, hi float8, basis int2, status int2, flags int4,
            artifact_id int8, extract_id int8, extract_row int4, locator jsonb, verbatim text,
            method int2, confidence real, review int2
         ) ON COMMIT DROP",
    )
    .execute(&mut *conn)
    .await?;
    let mut copy = conn
        .copy_in_raw("COPY obs_stage FROM STDIN (FORMAT binary)")
        .await?;
    let mut b = BinCopy::new();
    for (n, (r, p)) in rows.iter().zip(prep).enumerate() {
        b.row(21);
        b.i64(i64::try_from(n)?);
        b.i32(p.indicator_key);
        b.i32(p.geo_key);
        b.i32(dims_keys[&p.dims.canonical()]);
        b.char1(p.period.freq.as_char() as u8);
        b.date(p.period.start);
        b.i16(p.geo_level);
        b.f64(r.value);
        b.f64(r.lo);
        b.f64(r.hi);
        b.i16(r.basis.code());
        b.i16(r.status.code());
        b.i32(r.flags.to_db());
        b.opt_i64(artifact_ids.get(r.artifact).copied());
        b.opt_i64(r.prov.and_then(|p| extract_ids.get(p.extract).copied()));
        b.opt_i32(r.prov.map(|p| i32::try_from(p.row).unwrap_or(i32::MAX)));
        b.jsonb(&r.locator.to_string());
        b.text(r.verbatim.as_deref());
        b.i16(r.method.code());
        b.f32(r.confidence);
        b.i16(review.code());
        if b.len() >= COPY_CHUNK {
            copy.send(b.take()).await?;
        }
    }
    b.finish();
    copy.send(b.take()).await?;
    let n = copy.finish().await?;
    sqlx::query("ANALYZE obs_stage").execute(&mut *conn).await?;
    Ok(n)
}

/// Interns dims combinations; returns canonical text → key.
async fn intern_dims(conn: &mut PgConnection, prep: &[Prepared]) -> Result<HashMap<String, i32>> {
    let mut uniq: Vec<&Dims> = prep.iter().map(|p| &p.dims).collect();
    uniq.sort_unstable();
    uniq.dedup();
    let texts: Vec<String> = uniq.iter().map(|d| d.canonical()).collect();
    let pairs: Vec<serde_json::Value> = uniq.iter().map(|d| d.to_json()).collect();
    sqlx::query(
        "INSERT INTO catalog.dim_sets (dims, pairs) SELECT * FROM unnest($1::text[], $2::jsonb[])
         ON CONFLICT (dims) DO NOTHING",
    )
    .bind(&texts)
    .bind(&pairs)
    .execute(&mut *conn)
    .await?;
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT dims, key FROM catalog.dim_sets WHERE dims = ANY($1)")
            .bind(&texts)
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows.into_iter().collect())
}

/// Classifies every staged row against the rows open at `cur_seq` (the
/// released version) — `a`dded, `c`hanged (value/interval/basis/status/
/// flags), `r`estated (same values, new locator or verbatim), `s`ame — and
/// lists the open rows that are no longer delivered (`obs_removed`) and the
/// rows this version would close (`obs_closing`). Nothing is written to
/// `data.*` here. Stored `revised` compares as `normal` (it is derived).
async fn classify(conn: &mut PgConnection, dataset_key: i32, cur_seq: i32) -> Result<Diff> {
    sqlx::query(
        "CREATE TEMP TABLE obs_diff ON COMMIT DROP AS
         SELECT s.*, c.obs_id AS cur_obs_id, c.status AS cur_status,
           CASE WHEN c.obs_id IS NULL THEN 'a'
                WHEN (s.value, s.lo, s.hi, s.basis, s.status, s.flags)
                     IS DISTINCT FROM (c.value, c.lo, c.hi, c.basis,
                                       CASE WHEN c.status = 3 THEN 1::int2 ELSE c.status END, c.flags)
                  THEN 'c'
                WHEN (s.locator::text, s.verbatim) IS DISTINCT FROM (p.locator::text, p.verbatim) THEN 'r'
                ELSE 's' END AS change,
           c.obs_id IS NOT NULL AND (s.value, s.lo, s.hi) IS DISTINCT FROM (c.value, c.lo, c.hi) AS value_changed
         FROM obs_stage s
         LEFT JOIN data.observations c
           ON c.dataset_key = $1 AND c.vto = 2147483647 AND c.vfrom <= $2
          AND c.indicator_key = s.indicator_key AND c.geo_key = s.geo_key
          AND c.time_start = s.time_start AND c.freq = s.freq AND c.dims_key = s.dims_key
         LEFT JOIN data.obs_provenance p ON p.dataset_key = $1 AND p.obs_id = c.obs_id",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE TEMP TABLE obs_removed ON COMMIT DROP AS
         SELECT o.obs_id FROM data.observations o
         WHERE o.dataset_key = $1 AND o.vto = 2147483647 AND o.vfrom <= $2
           AND NOT EXISTS (SELECT 1 FROM obs_stage s
                           WHERE s.indicator_key = o.indicator_key AND s.geo_key = o.geo_key
                             AND s.time_start = o.time_start AND s.freq = o.freq AND s.dims_key = o.dims_key)",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE TEMP TABLE obs_closing ON COMMIT DROP AS
         SELECT cur_obs_id AS obs_id FROM obs_diff WHERE change IN ('c', 'r')
         UNION ALL SELECT obs_id FROM obs_removed",
    )
    .execute(&mut *conn)
    .await?;
    let (added, changed, restated, removed, prev): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE change = 'a'), count(*) FILTER (WHERE change = 'c'),
                count(*) FILTER (WHERE change = 'r'), (SELECT count(*) FROM obs_removed),
                (SELECT count(*) FROM data.observations
                  WHERE dataset_key = $1 AND vto = 2147483647 AND vfrom <= $2)
         FROM obs_diff",
    )
    .bind(dataset_key)
    .bind(cur_seq)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Diff {
        rows_added: added,
        rows_removed: removed,
        rows_changed: changed,
        rows_restated: restated,
        prev_rows: prev,
    })
}

/// Incremental multiset: `prev + Σ h(new or rewritten rows) − Σ h(closed
/// rows)`. Only the delta is hashed — staging an unchanged dataset hashes
/// nothing, appending 1,000 rows to 1 M hashes 1,000 (05 §3).
async fn delta_multiset(
    conn: &mut PgConnection,
    profile: &Profile,
    dataset_key: i32,
    prev: MultisetHash,
    rows: &[ObsRow],
) -> Result<(MultisetHash, u64)> {
    let mut m = prev;
    let rns: Vec<i64> = sqlx::query_scalar("SELECT rn FROM obs_diff WHERE change <> 's'")
        .fetch_all(&mut *conn)
        .await?;
    for rn in &rns {
        let r = rows
            .get(usize::try_from(*rn)?)
            .context("staged row number out of range")?;
        m.add(&canonical(r).hash());
    }
    // CANON_SELECT is a constant; nothing user-supplied is formatted in.
    let closed: Vec<CanonRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CANON_SELECT} JOIN obs_closing x ON x.obs_id = o.obs_id WHERE o.dataset_key = $1"
    )))
    .bind(dataset_key)
    .fetch_all(&mut *conn)
    .await?;
    for r in &closed {
        m.remove(&r.hash(profile)?);
    }
    Ok((m, u64::try_from(rns.len() + closed.len())?))
}

/// Writes version `seq`: new and rewritten rows (`vfrom = seq`), their
/// provenance and the close list. `status` becomes `revised` when the
/// source says `normal` and the value differs from the released one (or the
/// released row was already `revised` — a restatement keeps the mark).
async fn write_version_rows(
    conn: &mut PgConnection,
    dataset_key: i32,
    version_id: i64,
    seq: i32,
) -> Result<i64> {
    let n_new: i64 = sqlx::query_scalar("SELECT count(*) FROM obs_diff WHERE change <> 's'")
        .fetch_one(&mut *conn)
        .await?;
    let next: i64 = sqlx::query_scalar(
        "UPDATE catalog.datasets SET obs_seq = obs_seq + $2 WHERE key = $1 RETURNING obs_seq - $2",
    )
    .bind(dataset_key)
    .bind(n_new)
    .fetch_one(&mut *conn)
    .await?;
    let base = i64::from(dataset_key) << OBS_KEY_SHIFT;
    sqlx::query(
        "CREATE TEMP TABLE obs_new ON COMMIT DROP AS
         SELECT d.*, $1::int8 + $2::int8 + row_number() OVER (ORDER BY d.rn) AS obs_id_new,
                CASE WHEN d.status = 1 AND (d.value_changed OR d.cur_status = 3) THEN 3::int2
                     ELSE d.status END AS status_out
         FROM obs_diff d WHERE d.change <> 's'",
    )
    .bind(base)
    .bind(next)
    .execute(&mut *conn)
    .await?;
    let written = sqlx::query(
        "INSERT INTO data.observations (obs_id, value, lo, hi, dataset_key, indicator_key, geo_key, dims_key,
            vfrom, vto, flags, time_start, geo_level, basis, status, freq)
         SELECT obs_id_new, value, lo, hi, $1, indicator_key, geo_key, dims_key, $2, 2147483647, flags,
                time_start, geo_level, basis, status_out, freq
         FROM obs_new ORDER BY obs_id_new",
    )
    .bind(dataset_key)
    .bind(seq)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    sqlx::query(
        "INSERT INTO data.obs_provenance (obs_id, dataset_key, artifact_id, extract_id, extract_row, locator,
            verbatim, method, confidence, review)
         SELECT obs_id_new, $1, artifact_id, extract_id, extract_row, locator, verbatim, method, confidence, review
         FROM obs_new WHERE artifact_id IS NOT NULL",
    )
    .bind(dataset_key)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO data.version_closes (version_id, obs_id) SELECT $1, obs_id FROM obs_closing",
    )
    .bind(version_id)
    .execute(&mut *conn)
    .await?;
    Ok(i64::try_from(written)?)
}

// ───────────────────────────── versions & releases ─────────────────────────

/// Current published version of a dataset: (id, dsv, seq), if any.
pub async fn current_version(
    conn: &mut PgConnection,
    dataset_id: &str,
) -> Result<Option<(i64, String, i32)>> {
    Ok(sqlx::query_as(
        "SELECT version_id, dsv, seq FROM data.current_versions WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_optional(&mut *conn)
    .await?)
}

/// Serialises publishing per dataset for the rest of the transaction.
async fn lock_dataset(conn: &mut PgConnection, dataset_key: i32) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('lanka-dataset'), $1)")
        .bind(dataset_key)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Deletes a staged/in-review version's rows and closes; marks it `status`.
async fn discard_open(
    conn: &mut PgConnection,
    dataset_key: i32,
    version_id: i64,
    seq: i32,
    status: &str,
    reviewer: Option<&str>,
    note: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM data.obs_provenance p USING data.observations o
         WHERE p.dataset_key = $1 AND o.dataset_key = $1 AND p.obs_id = o.obs_id AND o.vfrom = $2",
    )
    .bind(dataset_key)
    .bind(seq)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM data.observations WHERE dataset_key = $1 AND vfrom = $2")
        .bind(dataset_key)
        .bind(seq)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM data.version_closes WHERE version_id = $1")
        .bind(version_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE data.dataset_versions SET status = $2, reviewed_by = coalesce($3, reviewed_by),
            reviewed_at = CASE WHEN $3 IS NULL THEN reviewed_at ELSE now() END, review_note = $4
         WHERE id = $1",
    )
    .bind(version_id)
    .bind(status)
    .bind(reviewer)
    .bind(note)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Creates the release (`YYYYMMDD.N`): the current release's map with
/// `dataset_id` re-pointed to `version_id`. Applies the version's closes
/// (the only UPDATE on observations), supersedes the previous version and
/// writes the changelog row. Serialised by a transaction advisory lock.
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
    let (dataset_key, seq): (i32, i32) = sqlx::query_as(
        "SELECT d.key, v.seq FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
         WHERE v.id = $1",
    )
    .bind(version_id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE data.observations o SET vto = $3 - 1
         FROM data.version_closes c
         WHERE c.version_id = $1 AND o.dataset_key = $2 AND o.obs_id = c.obs_id AND o.vto = 2147483647",
    )
    .bind(version_id)
    .bind(dataset_key)
    .bind(seq)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM data.version_closes WHERE version_id = $1")
        .bind(version_id)
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
        (
            d.rows_added,
            d.rows_removed,
            d.rows_changed + d.rows_restated,
        )
    });
    let row_count: i64 =
        sqlx::query_scalar("SELECT row_count FROM data.dataset_versions WHERE id = $1")
            .bind(version_id)
            .fetch_one(&mut **tx)
            .await?;
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
    .bind(&data_version)
    .bind(dataset_id)
    .bind(prev_version)
    .bind(version_id)
    .bind(if diff.is_none_or(|d| d.prev_rows == 0) {
        row_count
    } else {
        added
    })
    .bind(removed)
    .bind(changed)
    .bind(summary)
    .execute(&mut **tx)
    .await?;
    Ok(data_version)
}

/// One observation dataset to stage, as produced by `standardise`.
#[derive(Debug, Clone, Copy)]
pub struct StageInput<'a> {
    pub run_id: i64,
    pub dataset_id: &'a str,
    pub vintage: Option<&'a str>,
    pub rows: &'a [ObsRow],
    /// `ops.artifacts.id` by `ObsRow::artifact`.
    pub artifact_ids: &'a [i64],
    /// `ops.extracts.id` by `CellProv::extract`.
    pub extract_ids: &'a [i64],
}

/// The published version a staging is diffed against.
struct Current {
    version_id: i64,
    dsv: String,
    seq: i32,
    multiset: Option<MultisetHash>,
    schema: Option<Vec<u8>>,
}

/// (version id, dsv, seq, multiset hash, schema hash)
type CurrentRow = (i64, String, i32, Option<Vec<u8>>, Option<Vec<u8>>);

async fn current_full(conn: &mut PgConnection, dataset_id: &str) -> Result<Option<Current>> {
    let r: Option<CurrentRow> = sqlx::query_as(
        "SELECT c.version_id, c.dsv, c.seq, v.multiset_hash, v.schema_hash
         FROM data.current_versions c JOIN data.dataset_versions v ON v.id = c.version_id
         WHERE c.dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(r.map(|(version_id, dsv, seq, m, schema)| Current {
        version_id,
        dsv,
        seq,
        multiset: m.as_deref().and_then(MultisetHash::from_slice),
        schema,
    }))
}

/// Validate → stage → diff → gate → publish one observation dataset.
///
/// Staging COPYs the rows into a temp table and classifies them against the
/// released version without writing anything; an unchanged dataset (no
/// added/changed/restated/removed row, same schema) ends here with nothing
/// hashed or written. Otherwise the dsv is computed incrementally from the
/// released version's multiset hash, and only new or rewritten rows are
/// inserted (range versioning, 05 §2.1).
#[allow(clippy::too_many_lines)]
pub async fn publish_observations(
    pool: &PgPool,
    registry: &Registry,
    profile: &Profile,
    input: StageInput<'_>,
) -> Result<PublishResult> {
    let StageInput {
        run_id,
        dataset_id,
        vintage,
        rows,
        artifact_ids,
        extract_ids,
    } = input;
    let class = registry
        .dataset(dataset_id)
        .map(|d| d.publication_class.clone())
        .with_context(|| format!("dataset {dataset_id:?} not in registry"))?;
    let prep = validate(pool, registry, profile, dataset_id, rows).await?;
    let n_rows = i64::try_from(rows.len())?;
    let n = u64::try_from(rows.len())?;

    let mut tx = pool.begin().await?;
    let (dataset_key, boundary_set): (i32, Option<String>) =
        sqlx::query_as("SELECT key, boundary_set FROM catalog.datasets WHERE id = $1")
            .bind(dataset_id)
            .fetch_one(&mut *tx)
            .await?;
    lock_dataset(&mut tx, dataset_key).await?;
    sqlx::query("SELECT data.ensure_obs_partition($1)")
        .bind(dataset_id)
        .execute(&mut *tx)
        .await?;
    let schema = schema_hash(schema_lines(&mut tx, dataset_id).await?);
    let current = current_full(&mut tx, dataset_id).await?;
    let review = if class == "A" {
        Review::AutoAccepted
    } else {
        Review::Pending
    };

    let dims_keys = intern_dims(&mut tx, &prep).await?;
    let staged = copy_stage(
        &mut tx,
        rows,
        &prep,
        &dims_keys,
        artifact_ids,
        extract_ids,
        review,
    )
    .await?;
    ensure!(staged == n, "COPY staged {staged} of {n} rows");
    let cur_seq = current.as_ref().map_or(0, |c| c.seq);
    let diff = classify(&mut tx, dataset_key, cur_seq).await?;
    let untouched = diff.rows_added == 0
        && diff.rows_changed == 0
        && diff.rows_restated == 0
        && diff.rows_removed == 0;

    let unchanged = |version_id, dsv: String, note: String| PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome: "unchanged",
        data_version: None,
        rows: n_rows,
        rows_written: 0,
        rows_hashed: 0,
        diff: None,
        note,
    };
    if let Some(c) = &current
        && untouched
        && c.schema.as_deref() == Some(schema.as_slice())
    {
        let r = unchanged(
            c.version_id,
            c.dsv.clone(),
            "content identical to the published version".to_owned(),
        );
        tx.rollback().await?;
        return Ok(r);
    }
    // Incremental from the released version when its hash is known, else a
    // full sum over the delivered rows (first version).
    let (multiset, hashed) = match current.as_ref().and_then(|c| c.multiset) {
        Some(prev) => delta_multiset(&mut tx, profile, dataset_key, prev, rows).await?,
        None => {
            let mut m = MultisetHash::empty();
            for r in rows {
                m.add(&canonical(r).hash());
            }
            (m, n)
        }
    };
    let dsv = dsv(&schema, &multiset, n);
    if let Some(c) = &current
        && c.dsv == dsv
    {
        let r = unchanged(
            c.version_id,
            dsv,
            "content identical to the published version".to_owned(),
        );
        tx.rollback().await?;
        return Ok(r);
    }
    let existing: Option<(i64, String)> =
        sqlx::query_as("SELECT id, status FROM data.dataset_versions WHERE dsv = $1")
            .bind(&dsv)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((vid, status)) = existing {
        tx.rollback().await?;
        let mut r = unchanged(
            vid,
            dsv,
            format!("identical content already exists as a {status} version"),
        );
        r.rows_hashed = hashed;
        if status == "in_review" {
            r.outcome = "in_review";
        }
        return Ok(r);
    }
    // One open version per dataset: a newer staging supersedes an older one.
    let open: Vec<(i64, i32)> = sqlx::query_as(
        "SELECT id, seq FROM data.dataset_versions
         WHERE dataset_id = $1 AND status IN ('staged', 'in_review', 'approved')",
    )
    .bind(dataset_id)
    .fetch_all(&mut *tx)
    .await?;
    for (vid, seq) in open {
        discard_open(
            &mut tx,
            dataset_key,
            vid,
            seq,
            "rejected",
            None,
            &format!("superseded by a newer staging of {dsv}"),
        )
        .await?;
    }

    let seq: i32 = sqlx::query_scalar(
        "SELECT coalesce(max(seq), 0) + 1 FROM data.dataset_versions WHERE dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_one(&mut *tx)
    .await?;
    let version_id: i64 = sqlx::query_scalar(
        "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status,
            publication_class, vintage, run_id, supersedes_id, seq, multiset_hash, schema_hash, boundary_set,
            git_rev, reproducible)
         VALUES ($1, $2, $3, $4, 'staged', $5::catalog.publication_class, $6, $7, $8, $9, $3, $10, $11, $12, true)
         RETURNING id",
    )
    .bind(dataset_id)
    .bind(&dsv)
    .bind(multiset.0.as_slice())
    .bind(n_rows)
    .bind(&class)
    .bind(vintage)
    .bind(run_id)
    .bind(current.as_ref().map(|c| c.version_id))
    .bind(seq)
    .bind(schema.as_slice())
    .bind(&boundary_set)
    .bind(lanka_core::git_rev())
    .fetch_one(&mut *tx)
    .await?;
    let written = write_version_rows(&mut tx, dataset_key, version_id, seq).await?;

    let used: BTreeSet<i64> = rows
        .iter()
        .filter_map(|r| artifact_ids.get(r.artifact).copied())
        .collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_artifacts (version_id, artifact_id) SELECT $1, unnest($2::bigint[])",
    )
    .bind(version_id)
    .bind(used.into_iter().collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;
    let used_extracts: BTreeSet<i64> = rows
        .iter()
        .filter_map(|r| r.prov.and_then(|p| extract_ids.get(p.extract).copied()))
        .collect();
    sqlx::query(
        "INSERT INTO data.dataset_version_extracts (version_id, extract_id) SELECT $1, unnest($2::bigint[])",
    )
    .bind(version_id)
    .bind(used_extracts.into_iter().collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;

    // Gate: class A publishes unless history moved more than the thresholds.
    let has_prev = current.is_some() && diff.prev_rows > 0;
    #[allow(clippy::cast_precision_loss)]
    let gate_failure = has_prev
        .then(|| {
            let prev = diff.prev_rows.max(1) as f64;
            if diff.rows_changed as f64 / prev > MAX_CHANGED_SHARE {
                Some(format!(
                    "{} historical values changed (> {:.0}%)",
                    diff.rows_changed,
                    MAX_CHANGED_SHARE * 100.0
                ))
            } else if diff.rows_removed as f64 / prev > MAX_REMOVED_SHARE {
                Some(format!(
                    "{} rows removed (> {:.0}%)",
                    diff.rows_removed,
                    MAX_REMOVED_SHARE * 100.0
                ))
            } else {
                None
            }
        })
        .flatten();
    let validation = json!({
        "rows": n_rows, "rows_written": written, "rows_hashed": hashed,
        "geo_codes": prep.iter().map(|p| p.geo_key).collect::<BTreeSet<_>>().len(),
        "gate_failure": gate_failure,
    });
    let diff_out = has_prev.then_some(diff);
    sqlx::query("UPDATE data.dataset_versions SET validation = $2, diff = $3 WHERE id = $1")
        .bind(version_id)
        .bind(&validation)
        .bind(diff_out.as_ref().map(|d| json!(d)))
        .execute(&mut *tx)
        .await?;

    let (outcome, data_version, note) = if class == "A" && gate_failure.is_none() {
        let dv = create_release(
            &mut tx,
            dataset_id,
            version_id,
            current.as_ref().map(|c| c.version_id),
            diff_out.as_ref(),
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
    // dataset_key is an integer from the catalogue, not user input.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ANALYZE data.observations_{dataset_key}"
    )))
    .execute(pool)
    .await?;
    Ok(PublishResult {
        dataset_id: dataset_id.to_owned(),
        dsv,
        version_id,
        outcome,
        data_version,
        rows: n_rows,
        rows_written: written,
        rows_hashed: hashed,
        diff: diff_out,
        note,
    })
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingVersion {
    pub dsv: String,
    pub dataset_id: String,
    pub seq: i32,
    pub publication_class: String,
    pub row_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub run_id: Option<i64>,
    pub diff: Option<serde_json::Value>,
    pub validation: Option<serde_json::Value>,
}

pub async fn pending_reviews(pool: &PgPool) -> Result<Vec<PendingVersion>> {
    Ok(sqlx::query_as(
        "SELECT dsv, dataset_id, seq, publication_class::text AS publication_class, row_count, created_at,
                run_id, diff, validation
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
    let (version_id, dataset_id, status, rows, diff, seq, dataset_key): (
        i64,
        String,
        String,
        i64,
        Option<serde_json::Value>,
        i32,
        i32,
    ) = sqlx::query_as(
        "SELECT v.id, v.dataset_id, v.status, v.row_count, v.diff, v.seq, d.key
         FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
         WHERE v.dsv = $1 FOR UPDATE OF v",
    )
    .bind(dsv)
    .fetch_optional(&mut *tx)
    .await?
    .with_context(|| format!("no dataset version {dsv}"))?;
    ensure!(status == "in_review", "{dsv} is {status}, not in_review");
    lock_dataset(&mut tx, dataset_key).await?;
    let reviewed = sqlx::query(
        "UPDATE data.obs_provenance p SET review = $3
         FROM data.observations o
         WHERE p.dataset_key = $1 AND o.dataset_key = $1 AND o.obs_id = p.obs_id AND o.vfrom = $2
           AND p.review = $4",
    )
    .bind(dataset_key)
    .bind(seq)
    .bind(Review::Approved.code())
    .bind(Review::Pending.code())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE data.dataset_versions SET status = 'approved', reviewed_by = $2, reviewed_at = now(),
            review_note = $3 WHERE id = $1",
    )
    .bind(version_id)
    .bind(reviewer)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    let current = current_version(&mut tx, &dataset_id).await?;
    let diff: Option<Diff> = diff.and_then(|d| serde_json::from_value(d).ok());
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
        rows_written: i64::try_from(reviewed)?,
        rows_hashed: 0,
        diff,
        note: format!("{reviewed} rows approved by {reviewer}"),
    })
}

/// Rejects a version in review: its rows and closes are deleted, the version
/// row stays (status `rejected`) as the record of the decision.
pub async fn reject(pool: &PgPool, dsv: &str, reviewer: &str, note: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let (version_id, seq, dataset_key): (i64, i32, i32) = sqlx::query_as(
        "SELECT v.id, v.seq, d.key FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
         WHERE v.dsv = $1 AND v.status = 'in_review' FOR UPDATE OF v",
    )
    .bind(dsv)
    .fetch_optional(&mut *tx)
    .await?
    .with_context(|| format!("{dsv} is not in review"))?;
    lock_dataset(&mut tx, dataset_key).await?;
    discard_open(
        &mut tx,
        dataset_key,
        version_id,
        seq,
        "rejected",
        Some(reviewer),
        note,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ───────────────────────────── hash backfill ───────────────────────────────

/// (code, level, name_en, name_si, name_ta, parent code, geom sha256 hex)
type UnitCanon = (
    String,
    i16,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

#[derive(Debug, sqlx::FromRow)]
struct VersionRow {
    id: i64,
    dataset_id: String,
    key: i32,
    kind: String,
    seq: i32,
    boundary_set: Option<String>,
}

/// Canonical columns of stored observations (codes, not keys), for hashing.
/// Callers append `JOIN …`/`WHERE …` on alias `o`.
const CANON_SELECT: &str =
    "SELECT i.id AS indicator_id, u.code AS geo_code, o.freq::text AS freq, o.time_start,
        ds.dims, o.value, o.lo, o.hi, o.basis, o.status, o.flags, p.verbatim, p.locator
 FROM data.observations o
 JOIN catalog.indicators i ON i.key = o.indicator_key
 JOIN geo.units u ON u.key = o.geo_key
 JOIN catalog.dim_sets ds ON ds.key = o.dims_key
 LEFT JOIN data.obs_provenance p ON p.dataset_key = o.dataset_key AND p.obs_id = o.obs_id";

#[derive(Debug, sqlx::FromRow)]
struct CanonRow {
    indicator_id: String,
    geo_code: String,
    freq: String,
    time_start: chrono::NaiveDate,
    dims: String,
    value: Option<f64>,
    lo: Option<f64>,
    hi: Option<f64>,
    basis: i16,
    status: i16,
    flags: i32,
    verbatim: Option<String>,
    locator: Option<serde_json::Value>,
}

impl CanonRow {
    /// The row's canonical hash — identical to the hash of the `ObsRow` the
    /// connector produced (stored `revised` hashes as `normal`).
    fn hash(&self, profile: &Profile) -> Result<[u8; 32]> {
        let freq = self
            .freq
            .chars()
            .next()
            .and_then(Freq::from_char)
            .context("frequency")?;
        let period = Period {
            freq,
            start: self.time_start,
        }
        .render(&profile.calendars);
        let null = serde_json::Value::Null;
        Ok(CanonicalRow {
            indicator_id: &self.indicator_id,
            geo_code: &self.geo_code,
            period: &period,
            dims: &self.dims,
            value: self.value,
            lo: self.lo,
            hi: self.hi,
            basis: Basis::from_code(self.basis).context("basis")?,
            status: Status::from_code(self.status).context("status")?,
            flags: Flags::from_db(self.flags)?,
            verbatim: self.verbatim.as_deref(),
            locator: Some(self.locator.as_ref().unwrap_or(&null)),
        }
        .hash())
    }
}

/// Multiset hash of the rows of a dataset visible at version `seq`,
/// recomputed from the database (backfill and verification).
pub async fn multiset_at(
    conn: &mut PgConnection,
    profile: &Profile,
    dataset_key: i32,
    seq: i32,
) -> Result<(MultisetHash, u64)> {
    let rows: Vec<CanonRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{CANON_SELECT} WHERE o.dataset_key = $1 AND o.vfrom <= $2 AND o.vto >= $2"
    )))
    .bind(dataset_key)
    .bind(seq)
    .fetch_all(&mut *conn)
    .await?;
    let mut m = MultisetHash::empty();
    for r in &rows {
        m.add(&r.hash(profile)?);
    }
    Ok((m, u64::try_from(rows.len())?))
}

/// Gives every version migrated from 0001 (no multiset hash yet) its
/// multiset/schema hashes and recomputes its dsv; the 0001 value stays in
/// `legacy_dsv`. Indicator versions hash the rows visible at their seq;
/// a boundary version hashes the units of its set (only the current one is
/// reconstructible — older ones are marked `reproducible = false`).
pub async fn backfill_hashes(pool: &PgPool, profile: &Profile) -> Result<usize> {
    let pending: Vec<VersionRow> = sqlx::query_as(
        "SELECT v.id, v.dataset_id, d.key, d.kind, v.seq, v.boundary_set
         FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
         WHERE v.multiset_hash IS NULL AND v.status NOT IN ('rejected', 'withdrawn')
         ORDER BY v.id",
    )
    .fetch_all(pool)
    .await?;
    let mut done = 0;
    for v in pending {
        let mut tx = pool.begin().await?;
        let (schema, multiset, n) = if v.kind == "boundaries" {
            let current = current_version(&mut tx, &v.dataset_id).await?;
            if current.as_ref().map(|c| c.0) != Some(v.id) {
                sqlx::query("UPDATE data.dataset_versions SET reproducible = false WHERE id = $1")
                    .bind(v.id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                continue;
            }
            let set = v
                .boundary_set
                .clone()
                .unwrap_or_else(|| profile.reference_boundary_set().to_owned());
            let units: Vec<UnitCanon> =
                sqlx::query_as(
                    "SELECT u.code, u.level, u.name_en, u.name_si, u.name_ta, p.code, encode(g.geom_sha256, 'hex')
                     FROM geo.unit_geoms g JOIN geo.units u ON u.key = g.unit_key
                     LEFT JOIN geo.units p ON p.key = u.parent_key
                     WHERE g.boundary_set = $1",
                )
                .bind(&set)
                .fetch_all(&mut *tx)
                .await?;
            let m = MultisetHash::of_bytes(units.iter().map(|u| {
                unit_row_bytes(
                    &u.0,
                    u.1,
                    [Some(u.2.as_str()), u.3.as_deref(), u.4.as_deref()],
                    u.5.as_deref(),
                    &u.6,
                )
            }));
            (
                boundary_schema(&set, profile.admin_scheme()),
                m,
                units.len(),
            )
        } else {
            let (m, n) = multiset_at(&mut tx, profile, v.key, v.seq).await?;
            (
                schema_hash(schema_lines(&mut tx, &v.dataset_id).await?),
                m,
                usize::try_from(n)?,
            )
        };
        let new_dsv = dsv(&schema, &multiset, u64::try_from(n)?);
        sqlx::query(
            "UPDATE data.dataset_versions SET multiset_hash = $2, schema_hash = $3, dsv = $4,
                legacy_dsv = coalesce(legacy_dsv, dsv)
             WHERE id = $1",
        )
        .bind(v.id)
        .bind(multiset.0.as_slice())
        .bind(schema.as_slice())
        .bind(&new_dsv)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        done += 1;
    }
    if done > 0 {
        sqlx::query("SELECT pg_notify('lanka_release', 'hash-backfill')")
            .execute(pool)
            .await?;
    }
    Ok(done)
}

/// Method code for display (`data.code_labels`).
#[must_use]
pub fn method_label(m: Method) -> &'static str {
    m.as_str()
}

/// Upper bound of the version range of a current row.
pub const VTO: i32 = VTO_OPEN;

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
            lo: None,
            hi: None,
            basis: Basis::Reported,
            status: Status::Normal,
            flags: Flags::EMPTY,
            artifact: 0,
            prov: None,
            locator: json!({"path": "$[1][0]"}),
            verbatim: Some(v.to_string()),
            method: Method::Api,
            confidence: Some(1.0),
        }
    }

    fn multiset(rows: &[ObsRow]) -> MultisetHash {
        let mut m = MultisetHash::empty();
        rows.iter().for_each(|r| m.add(&canonical(r).hash()));
        m
    }

    #[test]
    fn dsv_is_order_independent_and_value_sensitive() {
        let s = schema_hash(vec!["x".into()]);
        let a = vec![row("x", "LK", "2020", 1.0), row("x", "LK", "2021", 2.0)];
        let b = vec![row("x", "LK", "2021", 2.0), row("x", "LK", "2020", 1.0)];
        assert_eq!(dsv(&s, &multiset(&a), 2), dsv(&s, &multiset(&b), 2));
        let c = vec![row("x", "LK", "2020", 1.0), row("x", "LK", "2021", 2.5)];
        assert_ne!(dsv(&s, &multiset(&a), 2), dsv(&s, &multiset(&c), 2));
        let d = dsv(&s, &multiset(&a), 2);
        assert!(d.starts_with("dsv_") && d.len() == 20);
        // Appending a period changes the hash by exactly the new row.
        let mut inc = multiset(&a);
        inc.add(&canonical(&row("x", "LK", "2022", 3.0)).hash());
        let mut all = a.clone();
        all.push(row("x", "LK", "2022", 3.0));
        assert_eq!(inc, multiset(&all));
    }

    #[test]
    fn unit_rows_separate_fields() {
        let a = unit_row_bytes("LK1", 1, [Some("A"), None, None], Some("LK"), "ab");
        let b = unit_row_bytes("LK1", 1, [None, Some("A"), None], Some("LK"), "ab");
        assert_ne!(a, b);
    }
}
