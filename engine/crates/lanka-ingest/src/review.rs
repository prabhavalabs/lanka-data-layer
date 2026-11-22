//! Evidence-based review (03 §6, WP-08): review subjects, per-cell
//! sign-off and the class-C extract gate.
//!
//! A review is about a dataset **version** (class B/C versions and gate
//! demotions; decided with `lanka review approve|reject <dsv>`, unchanged)
//! or about an **extract** (class C: an operator-run LLM-assisted reading).
//! A class-C extract needs a decision on every cell below confidence 1.0
//! plus a deterministic 10 % sample of the rest before it can be approved;
//! only an approved extract may feed `standardise`, and a version that
//! contains an unapproved class-C extract cannot be approved. A correction
//! writes a new extract (reader `manual`, `parent_id` = the original); the
//! original reading is never edited.

use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use lanka_store::{BlobRef, BlobStore, Ns};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::extract::{Cell, CellType, ExtractArtifact, ExtractOut, from_parquet, to_parquet};

/// Readers whose extracts are class C (per-cell sign-off).
pub const CLASS_C_READERS: &[&str] = &["llm_dual", "ocr"];

/// What a review is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A dataset version, by dsv.
    Version(String),
    /// An extract, by id (`x123` / `extract:123` on the command line).
    Extract(i64),
}

impl Target {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if let Some(id) = s.strip_prefix("extract:").or_else(|| s.strip_prefix('x')) {
            return Ok(Self::Extract(
                id.parse()
                    .with_context(|| format!("bad extract id in {s:?}"))?,
            ));
        }
        ensure!(
            s.starts_with("dsv_"),
            "expected a dsv (dsv_…) or an extract (x<id>), got {s:?}"
        );
        Ok(Self::Version(s.to_owned()))
    }

    /// Directory name under `store/evidence/`.
    #[must_use]
    pub fn subject(&self) -> String {
        match self {
            Self::Version(d) => d.clone(),
            Self::Extract(id) => format!("extract-{id}"),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version(d) => f.write_str(d),
            Self::Extract(id) => write!(f, "x{id}"),
        }
    }
}

/// Reviewer handles: non-empty, and in `LANKA_REVIEWERS` (comma-separated)
/// when that allow-list is configured (03 §6: there are no accounts).
pub fn check_reviewer(name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty(), "reviewer name is required");
    if let Ok(list) = std::env::var("LANKA_REVIEWERS")
        && !list.trim().is_empty()
    {
        ensure!(
            list.split(',').any(|r| r.trim() == name.trim()),
            "{name:?} is not in LANKA_REVIEWERS"
        );
    }
    Ok(())
}

/// An `ops.extracts` row.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ExtractRow {
    pub id: i64,
    #[serde(skip)]
    pub sha256: Vec<u8>,
    pub source_id: String,
    pub shape: String,
    pub extractor: String,
    pub extractor_ver: String,
    pub reader: Option<String>,
    pub model: Option<String>,
    pub parent_id: Option<i64>,
    pub approved_by: Option<String>,
    pub approved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub rows: i64,
    pub store_key: String,
    #[serde(skip)]
    pub template_sha256: Option<Vec<u8>>,
    #[serde(skip)]
    pub prompt_sha256: Option<Vec<u8>>,
}

impl ExtractRow {
    #[must_use]
    pub fn is_class_c(&self) -> bool {
        self.reader
            .as_deref()
            .is_some_and(|r| CLASS_C_READERS.contains(&r))
    }

    pub fn sha(&self) -> Result<[u8; 32]> {
        self.sha256.as_slice().try_into().context("extract sha256")
    }
}

const EXTRACT_COLS: &str =
    "id, sha256, source_id, shape, extractor, extractor_ver, reader, model, parent_id,
    approved_by, approved_at, rows, store_key, template_sha256, prompt_sha256";

pub async fn extract_row(pool: &PgPool, id: i64) -> Result<ExtractRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EXTRACT_COLS} FROM ops.extracts WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no extract {id}"))
}

/// An extract with its decoded rows and artifact list.
pub async fn load_extract(
    pool: &PgPool,
    store: &dyn BlobStore,
    id: i64,
) -> Result<(ExtractRow, ExtractOut, Vec<ExtractArtifact>)> {
    let row = extract_row(pool, id).await?;
    let blob = BlobRef {
        ns: Ns::Extract,
        key: row.store_key.clone(),
        sha256: row.sha()?,
        bytes: 0,
    };
    let bytes = store.get(&blob).await?;
    let data = tokio::task::spawn_blocking(move || from_parquet(bytes)).await??;
    let arts: Vec<(i64, String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT x.artifact_id, x.key, x.url, a.sha256
         FROM ops.extract_artifacts x JOIN ops.artifacts a ON a.id = x.artifact_id
         WHERE x.extract_id = $1 ORDER BY x.idx",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let artifacts = arts
        .into_iter()
        .map(|(artifact_id, key, url, sha)| {
            Ok(ExtractArtifact {
                artifact_id,
                key,
                url,
                sha256: sha.as_slice().try_into().context("artifact sha256")?,
            })
        })
        .collect::<Result<_>>()?;
    Ok((row, data, artifacts))
}

/// Value cells (not headers, not labels) of a cells extract, by row index.
fn value_cells(cells: &[Cell]) -> impl Iterator<Item = (u32, &Cell)> {
    cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.r > 0 && c.c > 0)
        .map(|(i, c)| (u32::try_from(i).unwrap_or(u32::MAX), c))
}

/// Is a cell in the deterministic 10 % sample of an extract?
#[must_use]
pub fn sampled(extract_sha: &[u8; 32], row_idx: u32) -> bool {
    let mut h = Sha256::new();
    h.update(b"lanka-review-sample-v1");
    h.update(extract_sha);
    h.update(row_idx.to_le_bytes());
    h.finalize()[0] < 26 // 26/256 ≈ 10 %
}

/// Cells of a class-C extract that need a reviewer decision: every value
/// cell below confidence 1.0, plus a 10 % sample of the rest (at least one).
#[must_use]
pub fn required_cells(extract_sha: &[u8; 32], cells: &[Cell]) -> Vec<u32> {
    let mut low = Vec::new();
    let mut rest = Vec::new();
    for (i, c) in value_cells(cells) {
        if c.conf.is_none_or(|v| v < 1.0) {
            low.push(i);
        } else {
            rest.push(i);
        }
    }
    let mut sample: Vec<u32> = rest
        .iter()
        .copied()
        .filter(|i| sampled(extract_sha, *i))
        .collect();
    if sample.is_empty()
        && let Some(first) = rest.first()
    {
        sample.push(*first);
    }
    low.extend(sample);
    low.sort_unstable();
    low
}

/// The open review of a subject, opened if needed (versions must be in
/// review; extracts any time before approval).
pub async fn open_review(pool: &PgPool, target: &Target, reason: &str) -> Result<i64> {
    match target {
        Target::Version(dsv) => {
            let (vid, status): (i64, String) = sqlx::query_as(
                "SELECT id, status FROM data.dataset_versions WHERE dsv = $1
                 ORDER BY status = 'in_review' DESC, seq DESC LIMIT 1",
            )
            .bind(dsv)
            .fetch_optional(pool)
            .await?
            .with_context(|| format!("no dataset version {dsv}"))?;
            if let Some(id) = sqlx::query_scalar::<_, i64>(
                "SELECT id FROM ops.reviews WHERE version_id = $1 AND state = 'open'",
            )
            .bind(vid)
            .fetch_optional(pool)
            .await?
            {
                return Ok(id);
            }
            ensure!(status == "in_review", "{dsv} is {status}, not in review");
            Ok(sqlx::query_scalar(
                "INSERT INTO ops.reviews (version_id, reason) VALUES ($1, $2) RETURNING id",
            )
            .bind(vid)
            .bind(reason)
            .fetch_one(pool)
            .await?)
        }
        Target::Extract(id) => {
            if let Some(rid) = sqlx::query_scalar::<_, i64>(
                "SELECT id FROM ops.reviews WHERE extract_id = $1 AND state = 'open'",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                return Ok(rid);
            }
            let e = extract_row(pool, *id).await?;
            ensure!(e.approved_at.is_none(), "extract {id} is already approved");
            Ok(sqlx::query_scalar(
                "INSERT INTO ops.reviews (extract_id, reason) VALUES ($1, $2)
                 ON CONFLICT (extract_id) WHERE state = 'open' AND extract_id IS NOT NULL
                 DO UPDATE SET reason = EXCLUDED.reason RETURNING id",
            )
            .bind(id)
            .bind(reason)
            .fetch_one(pool)
            .await?)
        }
    }
}

/// Records the evidence sheet location on the open review.
pub async fn set_evidence_key(pool: &PgPool, review_id: i64, key: &str) -> Result<()> {
    sqlx::query("UPDATE ops.reviews SET evidence_key = $2 WHERE id = $1")
        .bind(review_id)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accept,
    Correct,
    Reject,
}

impl Decision {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "accept" => Self::Accept,
            "correct" => Self::Correct,
            "reject" => Self::Reject,
            _ => bail!("decision must be accept, correct or reject"),
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Correct => "correct",
            Self::Reject => "reject",
        }
    }
}

/// One per-cell decision.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CellDecision {
    pub extract_id: i64,
    pub row_idx: i32,
    pub decision: String,
    pub corrected_text: Option<String>,
    pub reviewer: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

/// Records (or replaces) a reviewer's decision on one extract cell.
pub async fn sign(
    pool: &PgPool,
    target: &Target,
    extract_id: i64,
    row_idx: u32,
    decision: Decision,
    corrected_text: Option<&str>,
    reviewer: &str,
) -> Result<()> {
    check_reviewer(reviewer)?;
    ensure!(
        (decision == Decision::Correct) == corrected_text.is_some_and(|t| !t.trim().is_empty()),
        "`correct` needs the text as printed (--text); other decisions take none"
    );
    let review_id = open_review(pool, target, "per-cell sign-off").await?;
    // The cell must belong to the subject.
    let belongs: bool = match target {
        Target::Extract(id) => *id == extract_id,
        Target::Version(dsv) => {
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM data.dataset_version_extracts ve
             JOIN data.dataset_versions v ON v.id = ve.version_id
             WHERE v.dsv = $1 AND ve.extract_id = $2)",
            )
            .bind(dsv)
            .bind(extract_id)
            .fetch_one(pool)
            .await?
        }
    };
    ensure!(belongs, "extract {extract_id} is not part of {target}");
    let rows: i64 = sqlx::query_scalar("SELECT rows FROM ops.extracts WHERE id = $1")
        .bind(extract_id)
        .fetch_one(pool)
        .await?;
    ensure!(
        i64::from(row_idx) < rows,
        "extract {extract_id} has {rows} rows"
    );
    sqlx::query(
        "INSERT INTO ops.review_cells (review_id, extract_id, row_idx, decision, corrected_text, reviewer)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (review_id, extract_id, row_idx) DO UPDATE
           SET decision = EXCLUDED.decision, corrected_text = EXCLUDED.corrected_text,
               reviewer = EXCLUDED.reviewer, at = now()",
    )
    .bind(review_id)
    .bind(extract_id)
    .bind(i32::try_from(row_idx)?)
    .bind(decision.as_str())
    .bind(corrected_text.map(str::trim))
    .bind(reviewer.trim())
    .execute(pool)
    .await?;
    Ok(())
}

/// Decisions recorded on a review.
pub async fn decisions(pool: &PgPool, review_id: i64) -> Result<Vec<CellDecision>> {
    Ok(sqlx::query_as(
        "SELECT extract_id, row_idx, decision, corrected_text, reviewer, at
         FROM ops.review_cells WHERE review_id = $1 ORDER BY extract_id, row_idx",
    )
    .bind(review_id)
    .fetch_all(pool)
    .await?)
}

/// Sign-off state of a class-C extract.
#[derive(Debug, Clone, Serialize)]
pub struct SignoffState {
    pub extract_id: i64,
    pub review_id: Option<i64>,
    pub required: Vec<u32>,
    pub missing: Vec<u32>,
    pub decisions: usize,
}

pub async fn signoff_state(
    pool: &PgPool,
    store: &dyn BlobStore,
    extract_id: i64,
) -> Result<SignoffState> {
    let (row, data, _) = load_extract(pool, store, extract_id).await?;
    let required = required_cells(&row.sha()?, data.cells());
    let review_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM ops.reviews WHERE extract_id = $1 AND state = 'open'")
            .bind(extract_id)
            .fetch_optional(pool)
            .await?;
    let done = match review_id {
        Some(r) => decisions(pool, r).await?,
        None => Vec::new(),
    };
    let missing = required
        .iter()
        .copied()
        .filter(|i| {
            !done
                .iter()
                .any(|d| d.extract_id == extract_id && d.row_idx == i32::try_from(*i).unwrap_or(-1))
        })
        .collect();
    Ok(SignoffState {
        extract_id,
        review_id,
        required,
        missing,
        decisions: done.len(),
    })
}

/// Content-addresses a cells extract and indexes it (reader, parent and
/// class-C model metadata included).
#[allow(clippy::too_many_arguments)]
pub async fn persist_cells(
    pool: &PgPool,
    store: &dyn BlobStore,
    source_id: &str,
    extractor: &str,
    extractor_ver: &str,
    meta: &ExtractMeta<'_>,
    cells: Vec<Cell>,
    artifacts: &[ExtractArtifact],
) -> Result<i64> {
    let out = ExtractOut::Cells(cells);
    let rows = i64::try_from(out.len())?;
    let reader = crate::pipeline::extract_reader(&out);
    let enc = to_parquet(
        &out,
        &[
            ("extractor", extractor),
            ("extractor_ver", extractor_ver),
            ("source", source_id),
        ],
    )?;
    let (blob, _) = store.put_cas(Ns::Extract, Bytes::from(enc)).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO ops.extracts (sha256, source_id, shape, extractor, extractor_ver, template_sha256,
            model, prompt_sha256, rows, bytes, store_key, reader, parent_id)
         VALUES ($1, $2, 'cells', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT (sha256) DO UPDATE SET reader = coalesce(ops.extracts.reader, EXCLUDED.reader)
         RETURNING id",
    )
    .bind(blob.sha256.as_slice())
    .bind(source_id)
    .bind(extractor)
    .bind(extractor_ver)
    .bind(meta.template_sha256.as_ref().map(<[u8; 32]>::as_slice))
    .bind(meta.model)
    .bind(meta.prompt_sha256.as_ref().map(<[u8; 32]>::as_slice))
    .bind(rows)
    .bind(i64::try_from(blob.bytes)?)
    .bind(&blob.key)
    .bind(reader)
    .bind(meta.parent_id)
    .fetch_one(pool)
    .await?;
    let idx: Vec<i32> = (0..artifacts.len())
        .map(i32::try_from)
        .collect::<std::result::Result<_, _>>()?;
    sqlx::query(
        "INSERT INTO ops.extract_artifacts (extract_id, idx, artifact_id, key, url)
         SELECT $1, * FROM unnest($2::int[], $3::bigint[], $4::text[], $5::text[])
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&idx)
    .bind(artifacts.iter().map(|a| a.artifact_id).collect::<Vec<_>>())
    .bind(artifacts.iter().map(|a| a.key.clone()).collect::<Vec<_>>())
    .bind(artifacts.iter().map(|a| a.url.clone()).collect::<Vec<_>>())
    .execute(pool)
    .await?;
    Ok(id)
}

/// Optional metadata of a persisted extract.
#[derive(Debug, Clone, Default)]
pub struct ExtractMeta<'a> {
    pub template_sha256: Option<[u8; 32]>,
    pub model: Option<&'a str>,
    pub prompt_sha256: Option<[u8; 32]>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractDecision {
    pub extract_id: i64,
    /// The extract standardise may now use (a manual child when there were
    /// corrections or rejections, else the reviewed extract itself).
    pub approved_extract_id: i64,
    pub accepted: usize,
    pub corrected: usize,
    pub rejected: usize,
}

/// Approves a class-C extract once every required cell has a decision.
pub async fn approve_extract(
    pool: &PgPool,
    store: &dyn BlobStore,
    extract_id: i64,
    reviewer: &str,
    note: Option<&str>,
) -> Result<ExtractDecision> {
    check_reviewer(reviewer)?;
    let state = signoff_state(pool, store, extract_id).await?;
    let review_id = state
        .review_id
        .with_context(|| format!("extract {extract_id} has no open review"))?;
    ensure!(
        state.missing.is_empty(),
        "extract {extract_id}: {} of {} required cell(s) have no decision yet (first: rows {:?}); \
         sign them with `lanka review sign x{extract_id} --row N --decision accept|correct|reject --by NAME`",
        state.missing.len(),
        state.required.len(),
        state.missing.iter().take(8).collect::<Vec<_>>()
    );
    let (row, data, artifacts) = load_extract(pool, store, extract_id).await?;
    let ds = decisions(pool, review_id).await?;
    let (mut accepted, mut corrected, mut rejected) = (0, 0, 0);
    let mut cells: Vec<Cell> = data.cells().to_vec();
    let mut drop = vec![false; cells.len()];
    for d in &ds {
        let i = usize::try_from(d.row_idx)?;
        ensure!(i < cells.len(), "decision on missing row {i}");
        match d.decision.as_str() {
            "accept" => accepted += 1,
            "correct" => {
                corrected += 1;
                let text = d.corrected_text.clone().unwrap_or_default();
                let c = &mut cells[i];
                c.vtype = if crate::template::NumberSpec::default().parse(&text)
                    == crate::template::Value::Text
                {
                    CellType::String
                } else {
                    CellType::Number
                };
                c.text = text;
                c.reader = "manual".to_owned();
                c.conf = Some(1.0);
            }
            _ => {
                rejected += 1;
                drop[i] = true;
            }
        }
    }
    let mut tx_target = extract_id;
    if corrected + rejected > 0 {
        let kept: Vec<Cell> = cells
            .into_iter()
            .zip(drop)
            .filter(|(_, d)| !d)
            .map(|(c, _)| c)
            .collect();
        tx_target = persist_cells(
            pool,
            store,
            &row.source_id,
            "manual",
            &row.extractor_ver,
            &ExtractMeta {
                template_sha256: row
                    .template_sha256
                    .as_deref()
                    .and_then(|s| s.try_into().ok()),
                model: row.model.as_deref(),
                prompt_sha256: row.prompt_sha256.as_deref().and_then(|s| s.try_into().ok()),
                parent_id: Some(extract_id),
            },
            kept,
            &artifacts,
        )
        .await?;
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE ops.extracts SET approved_by = $2, approved_at = now() WHERE id = ANY($1)")
        .bind(vec![extract_id, tx_target])
        .bind(reviewer.trim())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE ops.reviews SET state = 'approved', reviewer = $2, decided_at = now(), note = $3
         WHERE id = $1",
    )
    .bind(review_id)
    .bind(reviewer.trim())
    .bind(note)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ExtractDecision {
        extract_id,
        approved_extract_id: tx_target,
        accepted,
        corrected,
        rejected,
    })
}

/// Rejects a class-C extract: it can never feed `standardise`.
pub async fn reject_extract(
    pool: &PgPool,
    extract_id: i64,
    reviewer: &str,
    note: &str,
) -> Result<()> {
    check_reviewer(reviewer)?;
    let n = sqlx::query(
        "UPDATE ops.reviews SET state = 'rejected', reviewer = $2, decided_at = now(), note = $3
         WHERE extract_id = $1 AND state = 'open'",
    )
    .bind(extract_id)
    .bind(reviewer.trim())
    .bind(note)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(n == 1, "extract {extract_id} has no open review");
    Ok(())
}

/// Class-C extracts of a version that are not approved (the version
/// cannot be approved until they are).
pub async fn unapproved_class_c(pool: &PgPool, version_id: i64) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT e.id FROM data.dataset_version_extracts ve JOIN ops.extracts e ON e.id = ve.extract_id
         WHERE ve.version_id = $1 AND e.reader = ANY($2) AND e.approved_at IS NULL ORDER BY e.id",
    )
    .bind(version_id)
    .bind(CLASS_C_READERS.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(r: u32, c: u32, conf: f32) -> Cell {
        Cell {
            table_id: "t".into(),
            r,
            c,
            text: "1".into(),
            vtype: CellType::Number,
            page: Some(1),
            bbox: None,
            sheet: None,
            a1: None,
            row_header: None,
            col_header: None,
            artifact_idx: 0,
            conf: Some(conf),
            reader: "llm_dual".into(),
        }
    }

    #[test]
    fn targets_parse() {
        assert_eq!(Target::parse("x12").expect("x"), Target::Extract(12));
        assert_eq!(Target::parse("extract:7").expect("x"), Target::Extract(7));
        assert_eq!(
            Target::parse("dsv_ab").expect("dsv"),
            Target::Version("dsv_ab".into())
        );
        assert!(Target::parse("hello").is_err());
        assert_eq!(Target::Extract(3).subject(), "extract-3");
    }

    #[test]
    fn every_low_confidence_cell_and_a_sample_need_sign_off() {
        let mut cells = vec![cell(0, 1, 1.0), cell(1, 0, 1.0)]; // header + label: never required
        for r in 1..=60 {
            cells.push(cell(r, 1, if r % 7 == 0 { 0.8 } else { 1.0 }));
        }
        let req = required_cells(&[9; 32], &cells);
        let low: Vec<u32> = cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.conf < Some(1.0))
            .map(|(i, _)| u32::try_from(i).expect("i"))
            .collect();
        assert!(low.iter().all(|i| req.contains(i)), "all < 1.0 required");
        assert!(!req.contains(&0) && !req.contains(&1));
        let sample = req.len() - low.len();
        assert!((1..=15).contains(&sample), "~10 % sample, got {sample}");
        assert_eq!(req, required_cells(&[9; 32], &cells), "deterministic");
    }
}
