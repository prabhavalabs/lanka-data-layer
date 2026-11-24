//! `lanka civic extract` (13 §2.1): read an archived English gazette, decide
//! whether it is a baseline or an amendment, and store the instrument.
//!
//! A baseline's PDF is read through the document sandbox (`pdf_words`),
//! parsed by [`super::schedule`], checked by [`super::validate`] and stored
//! with its portfolios and items. An amendment is recorded as a timeline
//! event only (kind, effective date, the baseline it amends); applying its
//! operations is C2.

use anyhow::Result;
use chrono::NaiveDate;
use lanka_extractd::Sandbox;
use lanka_store::BlobStore;
use serde::Serialize;
use sqlx::PgPool;

use super::discover::{lead_pages, read_text};
use super::entity::{Aliases, portfolio_key};
use super::exceptions::Exceptions;
use super::instrument::{self, Kind};
use super::validate::{self, Check};
use super::{PARSER, schedule, schedule_b};
use crate::docs::files::blob_ref;

/// Why a document was not extracted (the batch skips these).
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The English file is not archived, or has no text yet.
    #[error("not ready: {0}")]
    NotReady(String),
    /// The notification does not cite Article 44 or 43, or says nothing
    /// about a schedule.
    #[error("not an Article 44 instrument: {0}")]
    NotInstrument(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct Extracted {
    pub doc_id: String,
    pub number: Option<String>,
    pub kind: &'static str,
    pub effective_from: NaiveDate,
    pub amends: Option<String>,
    pub head_of_state: Option<String>,
    pub status: &'static str,
    pub checks: Vec<Check>,
    pub portfolios: usize,
    pub institutions: usize,
    pub functions: usize,
    pub laws: usize,
}

#[derive(sqlx::FromRow)]
struct DocRow {
    issued_on: NaiveDate,
    number: Option<String>,
    sha256: Option<String>,
    bytes: Option<i64>,
    status: String,
    text_sha256: Option<String>,
}

/// Resolves the gazette an amendment names to the baseline it amends: the
/// gazette itself when it is a baseline, the baseline of the amendment when
/// the notification names an earlier amendment.
async fn resolve_baseline(pool: &PgPool, named: &str) -> Result<Option<String>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT kind, amends FROM civic.instruments WHERE doc_id = $1")
            .bind(named)
            .fetch_optional(pool)
            .await?;
    Ok(match row {
        Some((k, _)) if k == "baseline" || k == "supplement" => Some(named.to_owned()),
        Some((_, amends)) => amends,
        None => None,
    })
}

/// A supplement covers only the ministers just appointed, so it is judged
/// with the run it belongs to: the supplements since the last baseline,
/// together with this one, should add up to a cabinet (not less than
/// [`MIN_COVERAGE_PERCENT`] of the baseline before the run).
async fn run_check(pool: &PgPool, doc_id: &str, on: NaiveDate, keys: &[String]) -> Result<Check> {
    let base: Option<(String, NaiveDate, i64)> = sqlx::query_as(
        "SELECT i.doc_id, i.effective_from, (SELECT count(*) FROM civic.portfolios p WHERE p.doc_id = i.doc_id)
         FROM civic.instruments i
         WHERE i.kind = 'baseline' AND i.status IN ('validated', 'approved') AND i.effective_from < $1
         ORDER BY i.effective_from DESC LIMIT 1",
    )
    .bind(on)
    .fetch_optional(pool)
    .await?;
    let Some((base_id, base_date, base_n)) = base else {
        return Ok(Check {
            check: "run_coverage".to_owned(),
            ok: true,
            detail: "no earlier baseline to compare with".to_owned(),
        });
    };
    let earlier: Vec<(String, String)> = sqlx::query_as(
        "SELECT i.doc_id, p.portfolio_key
         FROM civic.instruments i JOIN civic.portfolios p ON p.doc_id = i.doc_id
         WHERE i.kind = 'supplement' AND i.status <> 'rejected'
           AND i.effective_from > $1 AND i.effective_from < $2 AND i.doc_id <> $3",
    )
    .bind(base_date)
    .bind(on)
    .bind(doc_id)
    .fetch_all(pool)
    .await?;
    let mut distinct: std::collections::BTreeSet<&str> =
        earlier.iter().map(|e| e.1.as_str()).collect();
    distinct.extend(keys.iter().map(String::as_str));
    let gazettes = earlier
        .iter()
        .map(|e| e.0.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        + 1;
    let n = i64::try_from(distinct.len()).unwrap_or(i64::MAX);
    let ok = n * 100 >= base_n * MIN_COVERAGE_PERCENT;
    Ok(Check {
        check: "run_coverage".to_owned(),
        ok,
        detail: format!(
            "the run of {gazettes} supplement(s) up to this one covers {n} ministers; the baseline before it ({base_id}) has {base_n}{}",
            if ok { "" } else { ": too few for a cabinet" }
        ),
    })
}

/// A run of supplements adds up to a cabinet: at least this share of the
/// baseline before it.
const MIN_COVERAGE_PERCENT: i64 = 30;

/// Parses, validates and stores one gazette.
pub async fn extract(
    pool: &PgPool,
    store: &dyn BlobStore,
    sandbox: &Sandbox,
    aliases: &Aliases,
    exceptions: &Exceptions,
    doc_id: &str,
) -> Result<Extracted, ExtractError> {
    let row: DocRow = sqlx::query_as(
        "SELECT d.issued_on,
                CASE WHEN d.number IS NULL THEN NULL ELSE d.number || COALESCE('/' || d.sub_number, '') END AS number,
                f.sha256, f.bytes, f.status, f.text_sha256
         FROM docs.documents d JOIN docs.files f ON f.doc_id = d.doc_id AND f.lang = 'en'
         WHERE d.doc_id = $1",
    )
    .bind(doc_id)
    .fetch_optional(pool)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or_else(|| ExtractError::NotReady(format!("{doc_id} has no English file")))?;
    let (Some(sha), Some(text_sha), "ok") = (
        row.sha256.as_deref(),
        row.text_sha256.as_deref(),
        row.status.as_str(),
    ) else {
        return Err(ExtractError::NotReady(format!(
            "{doc_id}: English file is {} (needs an archived file with text)",
            row.status
        )));
    };
    let text = read_text(store, text_sha).await?;
    let info = instrument::analyse(&lead_pages(&text), row.issued_on);
    if !info.is_cabinet_notification() {
        return Err(ExtractError::NotInstrument(format!(
            "{doc_id}: the notification cites Article {:?}",
            info.articles
        )));
    }
    let Some(kind) = info.kind else {
        return Err(ExtractError::NotInstrument(format!(
            "{doc_id}: the notification neither sets out a schedule nor amends one"
        )));
    };

    let mut checks: Vec<Check> = Vec::new();
    let mut amends: Option<String> = None;
    let mut sched = schedule::Schedule::default();
    let status = match kind {
        Kind::Amendment => {
            let named = info.amends.clone();
            amends = match &named {
                Some(n) => resolve_baseline(pool, n).await?,
                None => None,
            };
            let (ok, detail) = match (&named, &amends) {
                (_, Some(b)) => (true, format!("amends the baseline {b}")),
                (Some(n), None) => (
                    false,
                    format!("names {n}, which is not an extracted baseline"),
                ),
                (None, None) => (false, "the notification names no gazette".to_owned()),
            };
            checks.push(Check {
                check: "amended_baseline".to_owned(),
                ok,
                detail,
            });
            if ok { "validated" } else { "needs_review" }
        }
        Kind::Baseline | Kind::Supplement => {
            let blob = blob_ref(sha, row.bytes.unwrap_or(0))?;
            let pdf = store.get(&blob).await?;
            let words = sandbox.pdf_words(&pdf, 0, 0).await?;
            sched = schedule::parse(&words);
            if sched.portfolios.is_empty() {
                // the 2020 policy-statement layout
                sched = schedule_b::parse(&words);
            }
            checks = validate::validate(
                &sched,
                &text,
                exceptions.gaps(doc_id),
                exceptions.citations(doc_id),
            );
            let keys: Vec<String> = sched
                .portfolios
                .iter()
                .map(|p| portfolio_key(&p.title))
                .collect();
            if kind == Kind::Supplement {
                checks.push(run_check(pool, doc_id, info.effective_from, &keys).await?);
            }
            if validate::all_ok(&checks) {
                "validated"
            } else {
                "needs_review"
            }
        }
    };

    let mut tx = pool.begin().await.map_err(anyhow::Error::from)?;
    sqlx::query("DELETE FROM civic.portfolios WHERE doc_id = $1")
        .bind(doc_id)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;
    sqlx::query(
        "INSERT INTO civic.instruments
           (doc_id, kind, effective_from, amends, head_of_state, parser, parsed_at, status, checks)
         VALUES ($1, $2, $3, $4, $5, $6, now(), $7, $8)
         ON CONFLICT (doc_id) DO UPDATE SET
           kind = EXCLUDED.kind, effective_from = EXCLUDED.effective_from, amends = EXCLUDED.amends,
           head_of_state = EXCLUDED.head_of_state, parser = EXCLUDED.parser, parsed_at = now(),
           status = EXCLUDED.status, checks = EXCLUDED.checks",
    )
    .bind(doc_id)
    .bind(kind.as_str())
    .bind(info.effective_from)
    .bind(&amends)
    .bind(&info.head_of_state)
    .bind(PARSER)
    .bind(status)
    .bind(sqlx::types::Json(&checks))
    .execute(&mut *tx)
    .await
    .map_err(anyhow::Error::from)?;

    let (mut institutions, mut functions, mut laws) = (0, 0, 0);
    for p in &sched.portfolios {
        sqlx::query(
            "INSERT INTO civic.portfolios (doc_id, seq, title, portfolio_key, first_page)
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (doc_id, seq) DO NOTHING",
        )
        .bind(doc_id)
        .bind(p.seq)
        .bind(&p.title)
        .bind(portfolio_key(&p.title))
        .bind(p.first_page)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;
        let (mut cols, mut nos, mut texts, mut keys, mut pages) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (col, items) in [
            ("function", &p.functions),
            ("institution", &p.institutions),
            ("law", &p.laws),
        ] {
            for it in items {
                cols.push(col.to_owned());
                nos.push(it.item_no);
                texts.push(it.text.clone());
                keys.push((col == "institution").then(|| aliases.key_of(&it.text)));
                pages.push(it.page);
            }
        }
        institutions += p.institutions.len();
        functions += p.functions.len();
        laws += p.laws.len();
        sqlx::query(
            "INSERT INTO civic.items (doc_id, seq, col, item_no, text, entity_key, page)
             SELECT $1, $2, c, n, t, k, g
             FROM unnest($3::text[], $4::int[], $5::text[], $6::text[], $7::int[]) AS u(c, n, t, k, g)
             ON CONFLICT DO NOTHING",
        )
        .bind(doc_id)
        .bind(p.seq)
        .bind(&cols)
        .bind(&nos)
        .bind(&texts)
        .bind(&keys)
        .bind(&pages)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;
    }
    tx.commit().await.map_err(anyhow::Error::from)?;

    Ok(Extracted {
        doc_id: doc_id.to_owned(),
        number: row.number,
        kind: kind.as_str(),
        effective_from: info.effective_from,
        amends,
        head_of_state: info.head_of_state,
        status,
        checks,
        portfolios: sched.portfolios.len(),
        institutions,
        functions,
        laws,
    })
}

/// Records a reviewer's decision on an instrument (`approved` or `rejected`),
/// appended to its `checks` so the trail stays with the instrument.
pub async fn review(pool: &PgPool, doc_id: &str, approve: bool, by: &str) -> Result<()> {
    let status = if approve { "approved" } else { "rejected" };
    let detail = format!(
        "{status} by {by} on {}",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    let n = sqlx::query(
        "UPDATE civic.instruments
         SET status = $2,
             checks = checks || jsonb_build_array(jsonb_build_object('check', 'review', 'ok', $3::boolean, 'detail', $4::text))
         WHERE doc_id = $1",
    )
    .bind(doc_id)
    .bind(status)
    .bind(approve)
    .bind(detail)
    .execute(pool)
    .await?
    .rows_affected();
    anyhow::ensure!(n == 1, "{doc_id} is not an extracted instrument");
    Ok(())
}
