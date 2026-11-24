//! The civic record, read side (13 §2.1): ministries and the institutions in
//! their charge over time, and the archived gazettes they come from.
//!
//! Shared by the REST server (`/v2/civic/*`, `/v2/documents/{doc_id}`), the
//! `lanka civic show` command and the tests. Only `validated` and `approved`
//! instruments are served, except [`instruments`], which lists every kind and
//! status for the timeline.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;

use crate::error::{CoreError, CoreResult};

/// Entities per page of [`entities`] at most.
pub const ENTITIES_MAX_LIMIT: i64 = 500;
pub const ENTITIES_DEFAULT_LIMIT: i64 = 50;

// ───────────────────────────── instruments ──────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct InstrumentRow {
    pub doc_id: String,
    /// `baseline` | `amendment`
    pub kind: String,
    pub effective_from: NaiveDate,
    /// The baseline an amendment amends.
    pub amends: Option<String>,
    pub head_of_state: Option<String>,
    /// `validated` | `needs_review` | `approved` | `rejected`
    pub status: String,
    /// Ministers (headings) parsed; 0 for amendments.
    pub portfolios: i64,
    /// Institutions listed in Column II; 0 for amendments.
    pub institutions: i64,
    pub title: Option<String>,
    /// Gazette number, `2412/08`.
    pub number: Option<String>,
}

/// Every instrument, newest first, all kinds and statuses (for the timeline).
pub async fn instruments(pool: &PgPool) -> CoreResult<Vec<InstrumentRow>> {
    Ok(sqlx::query_as::<_, InstrumentRow>(
        "SELECT i.doc_id, i.kind, i.effective_from, i.amends, i.head_of_state, i.status,
                (SELECT count(*) FROM civic.portfolios p WHERE p.doc_id = i.doc_id) AS portfolios,
                (SELECT count(*) FROM civic.items t WHERE t.doc_id = i.doc_id AND t.col = 'institution') AS institutions,
                d.title ->> 'en' AS title,
                CASE WHEN d.number IS NULL THEN NULL ELSE d.number || COALESCE('/' || d.sub_number, '') END AS number
         FROM civic.instruments i JOIN docs.documents d ON d.doc_id = i.doc_id
         ORDER BY i.effective_from DESC, i.doc_id DESC",
    )
    .fetch_all(pool)
    .await?)
}

// ───────────────────────────── structure at a date ──────────────────────────

pub use crate::civic_timeline::{
    Added, DiffCounts, Kept, Moved, PortfolioDiff, PortfolioSide, Removed, short_title,
};
use crate::civic_timeline::{Changes, Heading, Instrument, Listing, Ministry, Side, Step};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InstrumentRef {
    pub doc_id: String,
    pub effective_from: NaiveDate,
    pub number: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ComposedRef {
    pub doc_id: String,
    pub effective_from: NaiveDate,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct AmendmentRef {
    pub doc_id: String,
    pub effective_from: NaiveDate,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TextItem {
    pub item_no: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct InstitutionItem {
    pub item_no: i32,
    pub text: String,
    pub entity_key: Option<String>,
    /// From `civic.entities` (after `lanka civic build`).
    pub kind: Option<String>,
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PortfolioOut {
    /// The gazette this heading is printed in (the evidence for its pages).
    pub doc_id: String,
    pub seq: i32,
    /// As printed: `Minister of Defence`.
    pub title: String,
    /// Without `Minister of`: `Defence`.
    pub short_title: String,
    pub portfolio_key: String,
    pub page: Option<i32>,
    pub functions: Vec<TextItem>,
    pub institutions: Vec<InstitutionItem>,
    pub laws: Vec<TextItem>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Structure {
    pub at: NaiveDate,
    /// The latest gazette the structure is made of.
    pub instrument: InstrumentRef,
    /// Every gazette it is made of: one baseline, or a run of supplements.
    pub composed_of: Vec<ComposedRef>,
    pub amendments_since: Vec<AmendmentRef>,
    pub portfolios: Vec<PortfolioOut>,
}

/// The served structures over time (baselines and runs of supplements).
async fn steps(pool: &PgPool) -> CoreResult<Vec<Step>> {
    let insts: Vec<(String, String, NaiveDate)> = sqlx::query_as(
        "SELECT doc_id, kind, effective_from FROM civic.instruments
         WHERE kind IN ('baseline', 'supplement') AND status IN ('validated', 'approved')",
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<String> = insts.iter().map(|i| i.0.clone()).collect();
    let heads: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT doc_id, seq, portfolio_key FROM civic.portfolios WHERE doc_id = ANY($1) ORDER BY doc_id, seq",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let list: Vec<Instrument> = insts
        .into_iter()
        .map(|(doc_id, kind, effective_from)| Instrument {
            headings: heads
                .iter()
                .filter(|h| h.0 == doc_id)
                .map(|h| Heading {
                    doc_id: doc_id.clone(),
                    seq: h.1,
                    portfolio_key: h.2.clone(),
                })
                .collect(),
            doc_id,
            kind,
            effective_from,
        })
        .collect();
    Ok(crate::civic_timeline::compose(&list))
}

/// The first date a structure is served for, if any.
pub async fn earliest(pool: &PgPool) -> CoreResult<Option<NaiveDate>> {
    Ok(steps(pool).await?.first().map(|s| s.effective_from))
}

fn in_force(steps: &[Step], at: NaiveDate) -> CoreResult<&Step> {
    crate::civic_timeline::step_at(steps, at).ok_or_else(|| {
        let first = steps.first().map_or_else(
            || "no ministry schedule has been parsed yet".to_owned(),
            |s| format!("the earliest schedule is from {}", s.effective_from),
        );
        CoreError::not_found(format!("no ministry schedule in force on {at}: {first}"))
    })
}

async fn number_of(pool: &PgPool, doc_id: &str) -> CoreResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT CASE WHEN number IS NULL THEN NULL ELSE number || COALESCE('/' || sub_number, '') END
         FROM docs.documents WHERE doc_id = $1",
    )
    .bind(doc_id)
    .fetch_optional(pool)
    .await?
    .flatten())
}

#[derive(FromRow)]
struct ItemRow {
    doc_id: String,
    seq: i32,
    col: String,
    item_no: i32,
    text: String,
    entity_key: Option<String>,
    kind: Option<String>,
    page: Option<i32>,
}

/// The ministries, with their functions, institutions and laws, in force on `at`.
pub async fn structure(pool: &PgPool, at: NaiveDate) -> CoreResult<Structure> {
    let all = steps(pool).await?;
    let step = in_force(&all, at)?;
    let docs: Vec<String> = step.composed.iter().map(|c| c.0.clone()).collect();
    let ports = sqlx::query_as::<_, (String, i32, String, String, Option<i32>)>(
        "SELECT doc_id, seq, title, portfolio_key, first_page FROM civic.portfolios WHERE doc_id = ANY($1)",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;
    let items = sqlx::query_as::<_, ItemRow>(
        "SELECT t.doc_id, t.seq, t.col, t.item_no, t.text, t.entity_key, e.kind, t.page
         FROM civic.items t LEFT JOIN civic.entities e ON e.entity_key = t.entity_key
         WHERE t.doc_id = ANY($1) ORDER BY t.doc_id, t.seq, t.col, t.item_no",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;
    let mut portfolios: Vec<PortfolioOut> = Vec::new();
    let mut pos: BTreeMap<(String, i32), usize> = BTreeMap::new();
    for h in &step.headings {
        if let Some((_, _, title, key, page)) =
            ports.iter().find(|p| p.0 == h.doc_id && p.1 == h.seq)
        {
            pos.insert((h.doc_id.clone(), h.seq), portfolios.len());
            portfolios.push(PortfolioOut {
                doc_id: h.doc_id.clone(),
                seq: h.seq,
                title: title.clone(),
                short_title: short_title(title),
                portfolio_key: key.clone(),
                page: *page,
                functions: Vec::new(),
                institutions: Vec::new(),
                laws: Vec::new(),
            });
        }
    }
    for it in items {
        let Some(&i) = pos.get(&(it.doc_id.clone(), it.seq)) else {
            continue;
        };
        let p = &mut portfolios[i];
        match it.col.as_str() {
            "function" => p.functions.push(TextItem {
                item_no: it.item_no,
                text: it.text,
            }),
            "institution" => p.institutions.push(InstitutionItem {
                item_no: it.item_no,
                text: it.text,
                entity_key: it.entity_key,
                kind: it.kind,
                page: it.page,
            }),
            _ => p.laws.push(TextItem {
                item_no: it.item_no,
                text: it.text,
            }),
        }
    }
    let first = step.composed.first().map_or(step.effective_from, |c| c.1);
    let amendments_since = sqlx::query_as::<_, AmendmentRef>(
        "SELECT i.doc_id, i.effective_from, d.title ->> 'en' AS title
         FROM civic.instruments i JOIN docs.documents d ON d.doc_id = i.doc_id
         WHERE i.kind = 'amendment' AND i.status IN ('validated', 'approved')
           AND i.amends = ANY($1) AND i.effective_from > $2 AND i.effective_from <= $3
         ORDER BY i.effective_from, i.doc_id",
    )
    .bind(&docs)
    .bind(first)
    .bind(at)
    .fetch_all(pool)
    .await?;
    Ok(Structure {
        at,
        instrument: InstrumentRef {
            doc_id: step.latest.clone(),
            effective_from: step.effective_from,
            number: number_of(pool, &step.latest).await?,
        },
        composed_of: step
            .composed
            .iter()
            .map(|(doc_id, effective_from)| ComposedRef {
                doc_id: doc_id.clone(),
                effective_from: *effective_from,
            })
            .collect(),
        amendments_since,
        portfolios,
    })
}

// ───────────────────────────── diff ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DiffInstrument {
    pub doc_id: String,
    pub effective_from: NaiveDate,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Diff {
    pub from: DiffInstrument,
    pub to: DiffInstrument,
    pub moved: Vec<Moved>,
    pub added: Vec<Added>,
    pub removed: Vec<Removed>,
    pub portfolios: PortfolioDiff,
    pub counts: DiffCounts,
}

#[derive(FromRow)]
struct ListingRow {
    doc_id: String,
    seq: i32,
    text: String,
    entity_key: Option<String>,
    page: Option<i32>,
    name: Option<String>,
    kind: Option<String>,
    portfolio_key: String,
}

/// One structure for the comparison: its ministries and each institution
/// under the ministry that lists it first.
async fn side_of(pool: &PgPool, step: &Step) -> CoreResult<Side> {
    let docs: Vec<String> = step.composed.iter().map(|c| c.0.clone()).collect();
    let rows = sqlx::query_as::<_, ListingRow>(
        "SELECT t.doc_id, t.seq, t.text, t.entity_key, t.page, e.name, e.kind, p.portfolio_key
         FROM civic.items t
         JOIN civic.portfolios p ON p.doc_id = t.doc_id AND p.seq = t.seq
         LEFT JOIN civic.entities e ON e.entity_key = t.entity_key
         WHERE t.col = 'institution' AND t.doc_id = ANY($1)
         ORDER BY t.doc_id, t.seq, t.item_no",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;
    let titles: Vec<(String, i32, String, String)> = sqlx::query_as(
        "SELECT doc_id, seq, title, portfolio_key FROM civic.portfolios WHERE doc_id = ANY($1)",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;
    let mut side = Side::default();
    for h in &step.headings {
        if let Some(t) = titles.iter().find(|t| t.0 == h.doc_id && t.1 == h.seq) {
            side.ministries.push(Ministry {
                key: t.3.clone(),
                title: t.2.clone(),
            });
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    // listings in the order of the step's headings
    for h in &step.headings {
        for r in rows
            .iter()
            .filter(|r| r.doc_id == h.doc_id && r.seq == h.seq)
        {
            let Some(key) = &r.entity_key else { continue };
            if seen.insert(key.clone()) {
                side.listings.push(Listing {
                    entity_key: key.clone(),
                    name: r.name.clone().unwrap_or_else(|| r.text.clone()),
                    kind: r.kind.clone().unwrap_or_else(|| "other".to_owned()),
                    page: r.page,
                    ministry_key: r.portfolio_key.clone(),
                });
            }
        }
    }
    Ok(side)
}

/// What changed between the structures in force on two dates: institutions
/// that moved ministry, were added or removed, and the ministries kept (with
/// renames recognised by overlap), added and removed.
pub async fn diff(pool: &PgPool, from: NaiveDate, to: NaiveDate) -> CoreResult<Diff> {
    if from > to {
        return Err(CoreError::bad_request("from must not be after to"));
    }
    let all = steps(pool).await?;
    let (sf, st) = (in_force(&all, from)?, in_force(&all, to)?);
    let (a, b) = (side_of(pool, sf).await?, side_of(pool, st).await?);
    let Changes {
        moved,
        added,
        removed,
        portfolios,
        counts,
    } = crate::civic_timeline::compare(&a, &b);
    Ok(Diff {
        from: DiffInstrument {
            doc_id: sf.latest.clone(),
            effective_from: sf.effective_from,
        },
        to: DiffInstrument {
            doc_id: st.latest.clone(),
            effective_from: st.effective_from,
        },
        moved,
        added,
        removed,
        portfolios,
        counts,
    })
}

// ───────────────────────────── entities ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CurrentPortfolio {
    pub portfolio_title: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EntityRow {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    /// The ministry the institution is in now; null when it is no longer listed.
    pub current: Option<CurrentPortfolio>,
}

#[derive(FromRow)]
struct EntityListRow {
    entity_key: String,
    name: String,
    kind: String,
    current_title: Option<String>,
}

/// Escapes `%`, `_` and `\` for a LIKE pattern.
fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Institutions by name, alias or key (`q`, case-insensitive substring), those
/// in a ministry now first.
pub async fn entities(
    pool: &PgPool,
    q: Option<&str>,
    limit: Option<i64>,
) -> CoreResult<Vec<EntityRow>> {
    let limit = limit.unwrap_or(ENTITIES_DEFAULT_LIMIT);
    if !(1..=ENTITIES_MAX_LIMIT).contains(&limit) {
        return Err(CoreError::bad_request(format!(
            "limit must be 1..={ENTITIES_MAX_LIMIT}"
        )));
    }
    let pat = q
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", like_escape(s)));
    let rows = sqlx::query_as::<_, EntityListRow>(
        "SELECT e.entity_key, e.name, e.kind,
                (SELECT a.portfolio_title FROM civic.assignments a
                 WHERE a.entity_key = e.entity_key AND a.valid_to IS NULL LIMIT 1) AS current_title
         FROM civic.entities e
         WHERE $1::text IS NULL OR e.name ILIKE $1 OR e.entity_key ILIKE $1
            OR EXISTS (SELECT 1 FROM unnest(e.aliases) al WHERE al ILIKE $1)
         ORDER BY (EXISTS (SELECT 1 FROM civic.assignments a WHERE a.entity_key = e.entity_key AND a.valid_to IS NULL)) DESC,
                  e.name, e.entity_key
         LIMIT $2",
    )
    .bind(pat)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| EntityRow {
            entity_key: r.entity_key,
            name: r.name,
            kind: r.kind,
            current: r
                .current_title
                .map(|portfolio_title| CurrentPortfolio { portfolio_title }),
        })
        .collect())
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct HistoryRow {
    pub portfolio_title: Option<String>,
    pub portfolio_key: Option<String>,
    pub valid_from: NaiveDate,
    /// Null while the assignment is in force.
    pub valid_to: Option<NaiveDate>,
    pub doc_id: Option<String>,
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EntityDetail {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    pub aliases: Vec<String>,
    pub history: Vec<HistoryRow>,
}

/// One institution with every ministry it sat under, oldest first.
pub async fn entity(pool: &PgPool, key: &str) -> CoreResult<EntityDetail> {
    let (entity_key, name, kind, aliases) =
        sqlx::query_as::<_, (String, String, String, Vec<String>)>(
            "SELECT entity_key, name, kind, aliases FROM civic.entities WHERE entity_key = $1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| CoreError::not_found(format!("no institution {key}")))?;
    let history = sqlx::query_as::<_, HistoryRow>(
        "SELECT portfolio_title, portfolio_key, valid_from, valid_to, doc_id, page
         FROM civic.assignments WHERE entity_key = $1 ORDER BY valid_from",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    Ok(EntityDetail {
        entity_key,
        name,
        kind,
        aliases,
        history,
    })
}

// ───────────────────────────── documents ────────────────────────────────────

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DocumentFile {
    pub lang: String,
    pub status: String,
    pub pages: Option<i32>,
    pub bytes: Option<i64>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Document {
    pub doc_id: String,
    pub number: Option<String>,
    pub issued_on: NaiveDate,
    /// `{"en": "…", "si": "…", "ta": "…"}`
    #[schema(value_type = Object)]
    pub title: serde_json::Value,
    pub files: Vec<DocumentFile>,
    /// Where the publisher serves the English file (else the first file).
    pub source_url: Option<String>,
}

/// Percent-encodes everything except the RFC 3986 unreserved characters.
fn percent_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn source_url(proxy: Option<&str>, path: &str) -> Option<String> {
    proxy.map(|p| format!("{p}{}", percent_encode(path)))
}

#[derive(FromRow)]
struct DocRow {
    doc_id: String,
    number: Option<String>,
    issued_on: NaiveDate,
    title: serde_json::Value,
    proxy: Option<String>,
}

#[derive(FromRow)]
struct FileRow {
    lang: String,
    status: String,
    pages: Option<i32>,
    bytes: Option<i64>,
    sha256: Option<String>,
    upstream_path: String,
}

async fn doc_row(pool: &PgPool, doc_id: &str) -> CoreResult<DocRow> {
    sqlx::query_as::<_, DocRow>(
        "SELECT d.doc_id,
                CASE WHEN d.number IS NULL THEN NULL ELSE d.number || COALESCE('/' || d.sub_number, '') END AS number,
                d.issued_on, d.title, c.harvest ->> 'fileProxy' AS proxy
         FROM docs.documents d JOIN docs.collections c ON c.id = d.collection
         WHERE d.doc_id = $1",
    )
    .bind(doc_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| CoreError::not_found(format!("no document {doc_id}")))
}

/// A document with its files and the publisher's URL.
pub async fn document(pool: &PgPool, doc_id: &str) -> CoreResult<Document> {
    let d = doc_row(pool, doc_id).await?;
    let files = sqlx::query_as::<_, FileRow>(
        "SELECT lang, status, pages, bytes, sha256, upstream_path FROM docs.files
         WHERE doc_id = $1 ORDER BY CASE lang WHEN 'en' THEN 0 WHEN 'si' THEN 1 ELSE 2 END",
    )
    .bind(doc_id)
    .fetch_all(pool)
    .await?;
    let src = files
        .first()
        .and_then(|f| source_url(d.proxy.as_deref(), &f.upstream_path));
    Ok(Document {
        doc_id: d.doc_id,
        number: d.number,
        issued_on: d.issued_on,
        title: d.title,
        files: files
            .into_iter()
            .map(|f| DocumentFile {
                lang: f.lang,
                status: f.status,
                pages: f.pages,
                bytes: f.bytes,
                sha256: f.sha256,
            })
            .collect(),
        source_url: src,
    })
}

/// The archived PDF of one language: its blob hash and the publisher's URL.
#[derive(Debug, Clone)]
pub struct PdfFile {
    pub sha256: String,
    pub bytes: Option<i64>,
    pub source_url: Option<String>,
}

pub async fn pdf_file(pool: &PgPool, doc_id: &str, lang: &str) -> CoreResult<PdfFile> {
    let d = doc_row(pool, doc_id).await?;
    let f = sqlx::query_as::<_, FileRow>(
        "SELECT lang, status, pages, bytes, sha256, upstream_path FROM docs.files
         WHERE doc_id = $1 AND lang = $2",
    )
    .bind(doc_id)
    .bind(lang)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| CoreError::not_found(format!("{doc_id} has no {lang} file")))?;
    let sha = match (f.status.as_str(), f.sha256) {
        ("ok", Some(s)) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) => s,
        _ => {
            return Err(CoreError::not_found(format!(
                "{doc_id} {lang} file is not archived (status {})",
                f.status
            )));
        }
    };
    Ok(PdfFile {
        sha256: sha,
        bytes: f.bytes,
        source_url: source_url(d.proxy.as_deref(), &f.upstream_path),
    })
}

/// Adds the source and licence of the gazettes behind every civic response.
pub fn gazette_meta(meta: &mut crate::Meta) {
    use crate::envelope::{LicenceRef, SourceRef};
    const ATTRIBUTION: &str =
        "Presidential Secretariat, Government of Sri Lanka (official gazette text)";
    meta.source.push(SourceRef {
        id: "lk-gazette-extraordinary".to_owned(),
        publisher: "Government of Sri Lanka, Department of Government Printing".to_owned(),
        title: "Gazette Extraordinary of the Democratic Socialist Republic of Sri Lanka".to_owned(),
        url: "https://documents.gov.lk/web/extra_gazettes".to_owned(),
        licence: "public-domain-official-text".to_owned(),
        attribution: ATTRIBUTION.to_owned(),
    });
    meta.licences.push(LicenceRef {
        id: "public-domain-official-text".to_owned(),
        status: "open".to_owned(),
        url: None,
        attribution: Some(ATTRIBUTION.to_owned()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_urls_are_encoded() {
        assert_eq!(
            source_url(
                Some("https://documents.gov.lk/api/content-file-proxy?file="),
                "extra-gazette-content/G 48538 (E)_1.pdf"
            )
            .as_deref(),
            Some(
                "https://documents.gov.lk/api/content-file-proxy?file=extra-gazette-content%2FG%2048538%20%28E%29_1.pdf"
            )
        );
        assert_eq!(source_url(None, "x"), None);
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like_escape("100%_a\\b"), "100\\%\\_a\\\\b");
    }
}
