//! The harvester (13 §1.2): list a collection from the publisher's site,
//! upsert its documents, keep every change as a revision, withdraw what a
//! full sweep no longer lists, and (the tail of every run) fetch the files.
//!
//! Modes
//!
//! - `incremental` (daily): walk from page 1 until a full page has no new and
//!   no changed record.
//! - `sweep` (weekly): walk every page; documents not seen get `withdrawn_at`
//!   (cleared when they are listed again).
//! - `files`: download pending and failed files, extract their text.
//! - `text`: extract the text of archived files that have none yet.
//!
//! One listing request runs at a time (the upstream takes about 70 s per
//! page of 200); 429 and 5xx are retried with exponential backoff.
//!
//! Freshness and incidents: `ops.source_freshness` and the worker's
//! incidents are computed from `catalog.sources` and `ops.ingestion_runs`,
//! which a document collection is not. Until collections are registry
//! sources, a run is recorded in `docs.harvests` and mirrored to the event
//! feed (`ops.emit`, kind `docs_harvest` / `docs_harvest_failed`, with
//! `upstream_changed` in the message when the site changed shape).
// TODO(13 §1.2 step 5): mark the collection fresh in `ops` and raise the P1
// `upstream_changed` incident once collections are registered as sources.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDate, Utc};
use lanka_store::BlobStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

use super::record::{self, DocRecord, Kind, Sighting};
use super::rsc::{ListPage, parse_listing};
use super::{DocsError, action, files};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Incremental,
    Sweep,
    Files,
    Text,
}

impl Mode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::Sweep => "sweep",
            Self::Files => "files",
            Self::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "incremental" => Self::Incremental,
            "sweep" => Self::Sweep,
            "files" => Self::Files,
            "text" => Self::Text,
            o => bail!("unknown mode {o:?} (incremental | sweep | files | text)"),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `None` = incremental.
    pub mode: Option<Mode>,
    /// Stop after this many records (listing) or files (files, text, tail).
    pub limit: Option<usize>,
    /// Ignore listed documents issued before this date.
    pub since: Option<NaiveDate>,
    /// Walk at most this many listing pages.
    pub max_pages: Option<u32>,
    /// Do not run the files tail after a listing.
    pub skip_files: bool,
}

/// `docs.collections.harvest`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCfg {
    pub page_url: String,
    pub api_endpoint: String,
    #[serde(default = "d_page_size")]
    pub page_size: usize,
    pub file_proxy: String,
    pub doc_prefix: String,
    /// The last id that worked; refreshed whenever it is re-resolved.
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default = "d_list_timeout")]
    pub list_timeout_secs: u64,
    #[serde(default = "d_file_concurrency")]
    pub file_concurrency: usize,
    #[serde(default = "d_ocr_cap")]
    pub ocr_page_cap: usize,
}

fn d_page_size() -> usize {
    200
}
fn d_list_timeout() -> u64 {
    300
}
fn d_file_concurrency() -> usize {
    2
}
fn d_ocr_cap() -> usize {
    30
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: String,
    pub cfg: CollectionCfg,
}

pub async fn load_collection(pool: &PgPool, id: &str) -> Result<Collection> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT harvest FROM docs.collections WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let (v,) = row.with_context(|| format!("no collection {id:?} in docs.collections"))?;
    let cfg: CollectionCfg =
        serde_json::from_value(v).with_context(|| format!("harvest settings of {id}"))?;
    Ok(Collection {
        id: id.to_owned(),
        cfg,
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub mode: String,
    pub pages: u32,
    pub listed: i32,
    pub new: i32,
    pub changed: i32,
    pub reappeared: i32,
    pub skipped: i32,
    pub withdrawn: i32,
    pub files_ok: i32,
    pub files_failed: i32,
    pub files_missing: i32,
    pub files_corrupt: i32,
    pub text_done: i32,
}

// ── upsert ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upserted {
    pub doc_id: String,
    pub sighting: Sighting,
}

/// Stores one listed record (13 §1.2 step 3). New: insert the document, its
/// first revision and its `pending` files. Changed (the hash differs): append
/// a revision and update the row. Either way bump `last_seen_at` and clear
/// `withdrawn_at`. `seen_at` is the sighting time (unique per document: the
/// revision key).
pub async fn upsert_record(
    conn: &mut PgConnection,
    collection: &str,
    rec: &DocRecord,
    seen_at: DateTime<Utc>,
) -> Result<Upserted> {
    let existing: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT doc_id, meta_hash, withdrawn_at IS NOT NULL
           FROM docs.documents WHERE collection = $1 AND upstream_id = $2",
    )
    .bind(collection)
    .bind(&rec.upstream_id)
    .fetch_optional(&mut *conn)
    .await?;
    let sighting = record::classify(
        existing.as_ref().map(|(_, h, w)| (h.as_str(), *w)),
        &rec.meta_hash,
    );
    let doc_id = match &existing {
        Some((id, _, _)) => id.clone(),
        None => {
            let owner: Option<(String,)> =
                sqlx::query_as("SELECT upstream_id FROM docs.documents WHERE doc_id = $1")
                    .bind(&rec.doc_id_base)
                    .fetch_optional(&mut *conn)
                    .await?;
            record::pick_doc_id(
                &rec.doc_id_base,
                &rec.upstream_id,
                owner.as_ref().map(|(o,)| o.as_str()),
            )
        }
    };
    match sighting.kind {
        Kind::New => {
            sqlx::query(
                "INSERT INTO docs.documents
                   (doc_id, collection, number, sub_number, issued_on, title, keywords,
                    upstream_id, upstream_updated_at, meta_hash, first_seen_at, last_seen_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)",
            )
            .bind(&doc_id)
            .bind(collection)
            .bind(&rec.number)
            .bind(&rec.sub_number)
            .bind(rec.issued_on)
            .bind(&rec.title)
            .bind(&rec.keywords)
            .bind(&rec.upstream_id)
            .bind(rec.upstream_updated_at)
            .bind(&rec.meta_hash)
            .bind(seen_at)
            .execute(&mut *conn)
            .await
            .with_context(|| format!("inserting {doc_id}"))?;
            insert_revision(conn, &doc_id, rec, seen_at).await?;
        }
        Kind::Changed => {
            sqlx::query(
                "UPDATE docs.documents SET number = $2, sub_number = $3, issued_on = $4, title = $5,
                        keywords = $6, upstream_updated_at = $7, meta_hash = $8,
                        last_seen_at = $9, withdrawn_at = NULL
                  WHERE doc_id = $1",
            )
            .bind(&doc_id)
            .bind(&rec.number)
            .bind(&rec.sub_number)
            .bind(rec.issued_on)
            .bind(&rec.title)
            .bind(&rec.keywords)
            .bind(rec.upstream_updated_at)
            .bind(&rec.meta_hash)
            .bind(seen_at)
            .execute(&mut *conn)
            .await?;
            insert_revision(conn, &doc_id, rec, seen_at).await?;
        }
        Kind::Unchanged => {
            sqlx::query(
                "UPDATE docs.documents SET last_seen_at = $2, withdrawn_at = NULL WHERE doc_id = $1",
            )
            .bind(&doc_id)
            .bind(seen_at)
            .execute(&mut *conn)
            .await?;
        }
    }
    // The files: a new path (the upstream replaced the PDF) puts the file back to pending.
    for f in &rec.files {
        sqlx::query(
            "INSERT INTO docs.files (doc_id, lang, upstream_path, original_name, status)
             VALUES ($1, $2, $3, $4, 'pending')
             ON CONFLICT (doc_id, lang) DO UPDATE SET
               original_name = EXCLUDED.original_name,
               upstream_path = EXCLUDED.upstream_path,
               status     = CASE WHEN docs.files.upstream_path IS DISTINCT FROM EXCLUDED.upstream_path
                                 THEN 'pending' ELSE docs.files.status END,
               attempts   = CASE WHEN docs.files.upstream_path IS DISTINCT FROM EXCLUDED.upstream_path
                                 THEN 0 ELSE docs.files.attempts END,
               last_error = CASE WHEN docs.files.upstream_path IS DISTINCT FROM EXCLUDED.upstream_path
                                 THEN NULL ELSE docs.files.last_error END",
        )
        .bind(&doc_id)
        .bind(f.lang)
        .bind(&f.path)
        .bind(&f.name)
        .execute(&mut *conn)
        .await?;
    }
    Ok(Upserted { doc_id, sighting })
}

async fn insert_revision(
    conn: &mut PgConnection,
    doc_id: &str,
    rec: &DocRecord,
    seen_at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO docs.document_revisions (doc_id, seen_at, meta_hash, record) VALUES ($1, $2, $3, $4)",
    )
    .bind(doc_id)
    .bind(seen_at)
    .bind(&rec.meta_hash)
    .bind(&rec.record)
    .execute(conn)
    .await
    .with_context(|| format!("revision of {doc_id}"))?;
    Ok(())
}

/// Sets `withdrawn_at` on the collection's documents not seen since `run_started`.
pub async fn withdraw_unseen(
    pool: &PgPool,
    collection: &str,
    run_started: DateTime<Utc>,
) -> Result<u64> {
    Ok(sqlx::query(
        "UPDATE docs.documents SET withdrawn_at = now()
          WHERE collection = $1 AND withdrawn_at IS NULL AND last_seen_at < $2",
    )
    .bind(collection)
    .bind(run_started)
    .execute(pool)
    .await?
    .rows_affected())
}

// ── listing ────────────────────────────────────────────────────────────────

const BACKOFF: [u64; 3] = [5, 20, 60];

enum Attempt {
    /// The server action was refused (id gone) or the answer has no `data` frame.
    Rejected(String),
    Retry(anyhow::Error),
    Fatal(anyhow::Error),
}

struct Lister {
    http: reqwest::Client,
    cfg: CollectionCfg,
    action_id: Option<String>,
    /// The id changed during this run: persist it.
    refreshed: bool,
}

impl Lister {
    async fn ensure_id(&mut self) -> Result<String> {
        if let Some(id) = &self.action_id {
            return Ok(id.clone());
        }
        let id = action::resolve(&self.http, &self.cfg.page_url).await?;
        self.refreshed = self.cfg.action_id.as_deref() != Some(id.as_str());
        self.action_id = Some(id.clone());
        Ok(id)
    }

    async fn once(&self, id: &str, page: u32, limit: usize) -> Result<ListPage, Attempt> {
        let body = json!([{
            "apiEndpoint": self.cfg.api_endpoint,
            "page": page,
            "limit": limit,
            "q": "",
            "search": "",
            "forwarded": {},
        }]);
        let resp = self
            .http
            .post(&self.cfg.page_url)
            .header("next-action", id)
            .header("accept", "text/x-component")
            .header("content-type", "text/plain;charset=UTF-8")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| Attempt::Retry(anyhow!(e).context("listing request")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Attempt::Retry(anyhow!(e).context("listing body")))?;
        if text.contains("Server action not found") {
            return Err(Attempt::Rejected(format!(
                "HTTP {status}: Server action not found"
            )));
        }
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(Attempt::Retry(anyhow!("listing answered HTTP {status}")));
        }
        if !status.is_success() {
            return Err(Attempt::Rejected(format!("HTTP {status}")));
        }
        match parse_listing(&text) {
            Ok(Some(p)) => Ok(p),
            Ok(None) => Err(Attempt::Rejected(format!(
                "HTTP {status} without a data frame ({} bytes)",
                text.len()
            ))),
            Err(e) => Err(Attempt::Fatal(e)),
        }
    }

    /// One listing page; re-resolves the action id once when it is refused.
    async fn page(&mut self, page: u32, limit: usize) -> Result<ListPage> {
        let mut reresolved = false;
        loop {
            let id = self.ensure_id().await?;
            let mut tries = 0usize;
            let outcome = loop {
                match self.once(&id, page, limit).await {
                    Err(Attempt::Retry(e)) if tries < BACKOFF.len() => {
                        let wait = BACKOFF[tries];
                        tracing::warn!(error = %e, page, wait, "listing failed; backing off");
                        tokio::time::sleep(Duration::from_secs(wait)).await;
                        tries += 1;
                    }
                    other => break other,
                }
            };
            match outcome {
                Ok(p) => return Ok(p),
                Err(Attempt::Retry(e) | Attempt::Fatal(e)) => return Err(e),
                Err(Attempt::Rejected(why)) => {
                    self.action_id = None;
                    if reresolved {
                        return Err(DocsError::UpstreamChanged(format!(
                            "the listing action was rejected after re-resolving its id ({why})"
                        ))
                        .into());
                    }
                    tracing::warn!(why, "listing action rejected; re-resolving its id");
                    reresolved = true;
                }
            }
        }
    }
}

// ── the run ────────────────────────────────────────────────────────────────

/// Runs one harvest and records it in `docs.harvests`.
pub async fn run(
    pool: &PgPool,
    store: &dyn BlobStore,
    collection_id: &str,
    opts: &Options,
) -> Result<Report> {
    let mode = opts.mode.unwrap_or(Mode::Incremental);
    let col = load_collection(pool, collection_id).await?;
    let started = Utc::now();
    let (hid,): (i64,) = sqlx::query_as(
        "INSERT INTO docs.harvests (collection, started_at, mode) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(collection_id)
    .bind(started)
    .bind(mode.as_str())
    .fetch_one(pool)
    .await?;
    let mut report = Report {
        mode: mode.as_str().to_owned(),
        ..Report::default()
    };
    let result = run_inner(pool, store, &col, opts, mode, started, &mut report).await;
    let error = result.as_ref().err().map(|e| format!("{e:#}"));
    sqlx::query(
        "UPDATE docs.harvests SET finished_at = now(), listed = $2, new = $3, changed = $4, withdrawn = $5,
                files_ok = $6, files_failed = $7, error = $8 WHERE id = $1",
    )
    .bind(hid)
    .bind(report.listed)
    .bind(report.new)
    .bind(report.changed)
    .bind(report.withdrawn)
    .bind(report.files_ok)
    .bind(report.files_failed + report.files_missing + report.files_corrupt)
    .bind(&error)
    .execute(pool)
    .await?;
    emit_event(pool, collection_id, mode, &report, error.as_deref()).await;
    result.map(|()| report)
}

async fn emit_event(
    pool: &PgPool,
    collection: &str,
    mode: Mode,
    report: &Report,
    error: Option<&str>,
) {
    use crate::run::{Event, Level, Stage, emit};
    let data = serde_json::to_value(report).ok();
    let (level, kind, msg) = match error {
        None => (
            Level::Info,
            "docs_harvest",
            format!(
                "{collection} {}: {} listed, {} new, {} changed, {} withdrawn, {} files ok",
                mode.as_str(),
                report.listed,
                report.new,
                report.changed,
                report.withdrawn,
                report.files_ok
            ),
        ),
        Some(e) => (
            Level::Error,
            "docs_harvest_failed",
            format!("{collection} {} failed: {e}", mode.as_str()),
        ),
    };
    emit(
        pool,
        level,
        Stage::Run,
        &msg,
        Event {
            kind: Some(kind),
            source_id: Some(collection),
            data,
            ..Event::default()
        },
    )
    .await;
}

async fn run_inner(
    pool: &PgPool,
    store: &dyn BlobStore,
    col: &Collection,
    opts: &Options,
    mode: Mode,
    started: DateTime<Utc>,
    report: &mut Report,
) -> Result<()> {
    match mode {
        Mode::Incremental | Mode::Sweep => {
            list(pool, col, opts, mode, started, report).await?;
            if !opts.skip_files {
                files_tail(pool, store, col, opts.limit, false, report).await?;
            }
        }
        Mode::Files => files_tail(pool, store, col, opts.limit, false, report).await?,
        Mode::Text => files_tail(pool, store, col, opts.limit, true, report).await?,
    }
    Ok(())
}

async fn files_tail(
    pool: &PgPool,
    store: &dyn BlobStore,
    col: &Collection,
    limit: Option<usize>,
    text_only: bool,
    report: &mut Report,
) -> Result<()> {
    let fr = files::run(
        pool,
        store,
        col,
        &files::FileOpts {
            limit,
            text_only,
            concurrency: col.cfg.file_concurrency.max(1),
            ocr_page_cap: col.cfg.ocr_page_cap,
        },
    )
    .await?;
    report.files_ok += fr.ok;
    report.files_failed += fr.failed;
    report.files_missing += fr.missing;
    report.files_corrupt += fr.corrupt;
    report.text_done += fr.text_done;
    Ok(())
}

async fn list(
    pool: &PgPool,
    col: &Collection,
    opts: &Options,
    mode: Mode,
    started: DateTime<Utc>,
    report: &mut Report,
) -> Result<()> {
    let http = super::client(Duration::from_secs(col.cfg.list_timeout_secs))?;
    let mut lister = Lister {
        http,
        cfg: col.cfg.clone(),
        action_id: col.cfg.action_id.clone(),
        refreshed: false,
    };
    // A small --limit asks for a small page: the upstream is slow per record.
    let page_size = opts
        .limit
        .map_or(col.cfg.page_size, |l| l.clamp(1, col.cfg.page_size));
    let mut remaining = opts.limit;
    let mut page = 1u32;
    let mut upstream_total: Option<i64> = None;
    let mut complete = false;
    let walk = loop {
        let lp = match lister.page(page, page_size).await {
            Ok(p) => p,
            Err(e) => break Err(e),
        };
        if lister.refreshed {
            persist_action_id(pool, &col.id, lister.action_id.as_deref()).await;
            lister.refreshed = false;
        }
        report.pages += 1;
        upstream_total = lp.pagination.total.or(upstream_total);
        let total_pages = lp
            .pagination
            .total_pages
            .or_else(|| upstream_total.map(|t| (t + page_size as i64 - 1) / page_size as i64));
        let page_len = lp.records.len();
        let mut new_or_changed = 0usize;
        let mut older = 0usize;
        let mut bad = 0usize;
        let mut stored = 0usize;
        let mut tx = pool.begin().await?;
        for raw in &lp.records {
            if remaining == Some(0) {
                break;
            }
            let rec = match record::normalise(raw, &col.cfg.doc_prefix) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(id = raw.id, error = %e, "listed record skipped");
                    report.skipped += 1;
                    bad += 1;
                    continue;
                }
            };
            if opts.since.is_some_and(|s| rec.issued_on < s) {
                older += 1;
                continue;
            }
            let up = upsert_record(&mut tx, &col.id, &rec, Utc::now()).await?;
            stored += 1;
            report.listed += 1;
            match up.sighting.kind {
                Kind::New => {
                    report.new += 1;
                    new_or_changed += 1;
                }
                Kind::Changed => {
                    report.changed += 1;
                    new_or_changed += 1;
                }
                Kind::Unchanged => {}
            }
            if up.sighting.reappeared {
                report.reappeared += 1;
            }
            if let Some(r) = remaining.as_mut() {
                *r -= 1;
            }
        }
        tx.commit().await?;
        tracing::info!(
            page,
            page_len,
            stored,
            new = report.new,
            changed = report.changed,
            "listing page stored"
        );

        let since_cut = older > 0 && stored == 0 && older + bad >= page_len;
        let last_page = page_len == 0 || total_pages.is_some_and(|t| i64::from(page) >= t);
        if last_page && remaining != Some(0) {
            complete = true;
            break Ok(());
        }
        if remaining == Some(0) {
            break Ok(());
        }
        if mode == Mode::Incremental
            && record::incremental_stop(
                i64::from(page),
                total_pages,
                page_len,
                new_or_changed,
                since_cut,
            )
        {
            break Ok(());
        }
        if opts.max_pages.is_some_and(|m| page >= m) {
            break Ok(());
        }
        page += 1;
    };
    walk?;
    if mode == Mode::Sweep {
        let whole = complete && opts.since.is_none();
        if !record::may_withdraw(whole, i64::from(report.listed), upstream_total) {
            if whole {
                return Err(DocsError::UpstreamChanged(format!(
                    "the sweep listed {} documents but the site reports {}; nothing withdrawn",
                    report.listed,
                    upstream_total.map_or("?".to_owned(), |t| t.to_string())
                ))
                .into());
            }
            tracing::warn!("partial sweep: nothing withdrawn");
        } else {
            report.withdrawn = i32::try_from(withdraw_unseen(pool, &col.id, started).await?)?;
        }
    }
    Ok(())
}

async fn persist_action_id(pool: &PgPool, collection: &str, id: Option<&str>) {
    let Some(id) = id else { return };
    let res = sqlx::query(
        "UPDATE docs.collections SET harvest = jsonb_set(harvest, '{actionId}', to_jsonb($2::text)) WHERE id = $1",
    )
    .bind(collection)
    .bind(id)
    .execute(pool)
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "could not cache the action id");
    }
}

// ── status and search (CLI) ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Status {
    pub collection: String,
    pub documents: i64,
    pub withdrawn: i64,
    pub newest: Option<NaiveDate>,
    pub oldest: Option<NaiveDate>,
    pub files_by_status: Vec<(String, i64)>,
    pub text_by_status: Vec<(String, i64)>,
    pub searchable: i64,
    pub harvests: Vec<HarvestRow>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HarvestRow {
    pub id: i64,
    pub mode: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub listed: i32,
    pub new: i32,
    pub changed: i32,
    pub withdrawn: i32,
    pub files_ok: i32,
    pub files_failed: i32,
    pub error: Option<String>,
}

pub async fn status(pool: &PgPool, collection: &str) -> Result<Status> {
    let (documents, withdrawn, newest, oldest): (i64, i64, Option<NaiveDate>, Option<NaiveDate>) =
        sqlx::query_as(
            "SELECT count(*), count(withdrawn_at), max(issued_on), min(issued_on)
               FROM docs.documents WHERE collection = $1",
        )
        .bind(collection)
        .fetch_one(pool)
        .await?;
    let files_by_status: Vec<(String, i64)> = sqlx::query_as(
        "SELECT f.status, count(*) FROM docs.files f JOIN docs.documents d USING (doc_id)
          WHERE d.collection = $1 GROUP BY 1 ORDER BY 1",
    )
    .bind(collection)
    .fetch_all(pool)
    .await?;
    let text_by_status: Vec<(String, i64)> = sqlx::query_as(
        "SELECT coalesce(f.text_status, '-'), count(*) FROM docs.files f JOIN docs.documents d USING (doc_id)
          WHERE d.collection = $1 AND f.status = 'ok' GROUP BY 1 ORDER BY 1",
    )
    .bind(collection)
    .fetch_all(pool)
    .await?;
    let (searchable,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM docs.search s JOIN docs.documents d USING (doc_id) WHERE d.collection = $1",
    )
    .bind(collection)
    .fetch_one(pool)
    .await?;
    let harvests: Vec<HarvestRow> = sqlx::query_as(
        "SELECT id, mode, started_at, finished_at, listed, new, changed, withdrawn, files_ok, files_failed, error
           FROM docs.harvests WHERE collection = $1 ORDER BY id DESC LIMIT 8",
    )
    .bind(collection)
    .fetch_all(pool)
    .await?;
    Ok(Status {
        collection: collection.to_owned(),
        documents,
        withdrawn,
        newest,
        oldest,
        files_by_status,
        text_by_status,
        searchable,
        harvests,
    })
}

pub async fn collections(pool: &PgPool) -> Result<Vec<String>> {
    Ok(
        sqlx::query_as::<_, (String,)>("SELECT id FROM docs.collections ORDER BY id")
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|(i,)| i)
            .collect(),
    )
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Hit {
    pub doc_id: String,
    pub lang: String,
    pub issued_on: NaiveDate,
    pub title: Option<String>,
    pub rank: f32,
}

/// Full-text search over `docs.search` (`websearch_to_tsquery`, 'simple').
pub async fn search(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Hit>> {
    Ok(sqlx::query_as(
        "SELECT d.doc_id, s.lang, d.issued_on, d.title ->> 'en' AS title,
                ts_rank(s.tsv, query) AS rank
           FROM docs.search s
           JOIN docs.documents d USING (doc_id),
                websearch_to_tsquery('simple', $1) query
          WHERE s.tsv @@ query
          ORDER BY rank DESC, d.issued_on DESC
          LIMIT $2",
    )
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}
