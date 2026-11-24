//! `lanka docs mirror`: the off-site copy of the documents archive on GitHub
//! (13 §1.4, ADR 0027).
//!
//! One GitHub release per month (`YYYY-MM`) carries the month's PDFs as a tar
//! (`gazettes-YYYY-MM.tar`, split below GitHub's 2 GiB asset limit), their
//! text (`text-YYYY-MM.tar.gz`) and a manifest (`manifest-YYYY-MM.jsonl`). The
//! repository's git carries the small, diffable part: `months/<YYYY-MM>.json`,
//! `metadata/documents/<year>.jsonl`, `metadata/revisions/<year>.jsonl` and
//! the statistics block of the README.
//!
//! The order of a run, for every selected month:
//!
//! 1. the month must be **complete** (no file `pending` or `failed`);
//! 2. every `ok` file is read from the store and its SHA-256 and size are
//!    checked against the database, before anything is packed;
//! 3. the packs are built deterministically (the same archive gives the same
//!    bytes, so a re-run does not churn the release);
//! 4. the release is created if needed, the assets uploaded with `gh`, the
//!    asset sizes checked against GitHub and the manifest downloaded back and
//!    compared by checksum.
//!
//! Only after every month is verified is anything committed to git, in one
//! commit, and pushed. A month whose new manifest has the same SHA-256 as the
//! committed `months/<YYYY-MM>.json` is left alone.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use lanka_store::{BlobRef, BlobStore, Ns};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use super::files::blob_ref;
use super::harvest::{Collection, load_collection};
use super::percent_encode;

/// The public repository the archive is mirrored to.
pub const DEFAULT_REPO: &str = "lanka-data-layer/archive-gazettes";

/// A tar part stays below this size (1.9 GiB); GitHub allows 2 GiB per asset.
pub const MAX_PART_BYTES: u64 = 2_040_109_465;

/// A hard stop for any single asset (GitHub's limit is 2 GiB).
const ASSET_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

const STATS_START: &str = "<!-- stats:start -->";
const STATS_END: &str = "<!-- stats:end -->";

#[derive(Debug, Clone)]
pub struct MirrorOptions {
    /// Local clone of the archive repository.
    pub repo_dir: PathBuf,
    /// Months to mirror (`YYYY-MM`); empty = every complete month that changed.
    pub months: Vec<String>,
    /// Do everything except upload, commit and push.
    pub dry_run: bool,
    /// At most this many months in one run (oldest first).
    pub max_months: Option<usize>,
    /// Testing only: mirror a month that still has `pending`/`failed` files.
    pub allow_incomplete: bool,
    /// `owner/name` of the GitHub repository.
    pub github_repo: String,
    /// The `gh` binary (a test points this at a stub script).
    pub gh_bin: String,
    /// Where packs are built (default: the system temp directory).
    pub work_dir: Option<PathBuf>,
    /// Split tars above this size.
    pub max_part_bytes: u64,
}

impl MirrorOptions {
    #[must_use]
    pub fn new(repo_dir: impl Into<PathBuf>) -> Self {
        Self {
            repo_dir: repo_dir.into(),
            months: Vec::new(),
            dry_run: false,
            max_months: None,
            allow_incomplete: false,
            github_repo: DEFAULT_REPO.to_owned(),
            gh_bin: "gh".to_owned(),
            work_dir: None,
            max_part_bytes: MAX_PART_BYTES,
        }
    }
}

// ── records ────────────────────────────────────────────────────────────────

/// One file of a document, as published in `metadata/documents`.
#[derive(Debug, Clone, Serialize)]
pub struct FileRec {
    pub lang: String,
    pub status: String,
    pub sha256: Option<String>,
    pub bytes: Option<i64>,
    pub pages: Option<i32>,
    pub text_status: Option<String>,
    #[serde(skip)]
    pub text_sha256: Option<String>,
    pub source_url: String,
}

/// One document, as published in `metadata/documents`.
#[derive(Debug, Clone, Serialize)]
pub struct DocRec {
    pub doc_id: String,
    pub number: Option<String>,
    pub sub_number: Option<String>,
    pub issued_on: NaiveDate,
    pub title: Value,
    pub keywords: Option<Value>,
    pub upstream_id: String,
    pub upstream_updated_at: Option<DateTime<Utc>>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub withdrawn_at: Option<DateTime<Utc>>,
    pub files: Vec<FileRec>,
}

/// One line of `manifest-YYYY-MM.jsonl`.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestLine {
    pub doc_id: String,
    pub lang: String,
    /// The entry name in the tar; absent for files that are not archived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub sha256: Option<String>,
    pub bytes: Option<i64>,
    pub pages: Option<i32>,
    pub text_status: Option<String>,
    pub text_sha256: Option<String>,
    pub source_url: String,
    pub status: String,
}

/// The tar entry name of an archived PDF.
#[must_use]
pub fn pdf_name(doc_id: &str, lang: &str) -> String {
    format!("{doc_id}_{lang}.pdf")
}

/// The tar entry name of a file's text.
#[must_use]
pub fn text_name(doc_id: &str, lang: &str) -> String {
    format!("{doc_id}_{lang}.txt")
}

/// The URL the file was fetched from: the collection's proxy plus the encoded path.
#[must_use]
pub fn source_url(file_proxy: &str, upstream_path: &str) -> String {
    format!("{file_proxy}{}", percent_encode(upstream_path))
}

/// A month is complete when none of its files is still to be fetched.
/// `missing` and `corrupt` are settled outcomes.
#[must_use]
pub fn month_complete(docs: &[DocRec]) -> bool {
    docs.iter()
        .flat_map(|d| &d.files)
        .all(|f| !matches!(f.status.as_str(), "pending" | "failed"))
}

/// Manifest lines of a set of documents, sorted by `(doc_id, lang)`.
#[must_use]
pub fn manifest_lines(docs: &[DocRec]) -> Vec<ManifestLine> {
    let mut lines: Vec<ManifestLine> = docs
        .iter()
        .flat_map(|d| {
            d.files.iter().map(|f| ManifestLine {
                doc_id: d.doc_id.clone(),
                lang: f.lang.clone(),
                name: (f.status == "ok").then(|| pdf_name(&d.doc_id, &f.lang)),
                sha256: f.sha256.clone(),
                bytes: f.bytes,
                pages: f.pages,
                text_status: f.text_status.clone(),
                text_sha256: f.text_sha256.clone(),
                source_url: f.source_url.clone(),
                status: f.status.clone(),
            })
        })
        .collect();
    lines.sort_by(|a, b| (&a.doc_id, &a.lang).cmp(&(&b.doc_id, &b.lang)));
    lines
}

/// Compact JSON, one object per line, each line ended by `\n`.
pub fn to_jsonl<T: Serialize>(items: &[T]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for i in items {
        serde_json::to_writer(&mut out, i)?;
        out.push(b'\n');
    }
    Ok(out)
}

#[must_use]
pub fn sha_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn sha_file(path: &Path) -> Result<(String, u64)> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((hex::encode(h.finalize()), n))
}

// ── months ─────────────────────────────────────────────────────────────────

/// `YYYY-MM` as the half-open date range `[first day, first day of the next month)`.
pub fn month_range(month: &str) -> Result<(NaiveDate, NaiveDate)> {
    let bad = || anyhow!("{month:?} is not a month (expected YYYY-MM)");
    let (y, m) = month.split_once('-').ok_or_else(bad)?;
    if y.len() != 4 || m.len() != 2 {
        return Err(bad());
    }
    let (y, m): (i32, u32) = (y.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    let from = NaiveDate::from_ymd_opt(y, m, 1).ok_or_else(bad)?;
    let to = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    }
    .ok_or_else(bad)?;
    Ok((from, to))
}

fn year_range(year: i32) -> Result<(NaiveDate, NaiveDate)> {
    Ok((
        NaiveDate::from_ymd_opt(year, 1, 1).context("year")?,
        NaiveDate::from_ymd_opt(year + 1, 1, 1).context("year")?,
    ))
}

fn month_title(month: &str) -> Result<String> {
    let (from, _) = month_range(month)?;
    Ok(format!("Extraordinary gazettes, {}", from.format("%B %Y")))
}

fn lang_name(lang: &str) -> &str {
    match lang {
        "en" => "English",
        "si" => "Sinhala",
        "ta" => "Tamil",
        other => other,
    }
}

fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Counts of a month, for the release notes and `months/<m>.json`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MonthStats {
    pub documents: usize,
    pub files_ok: usize,
    pub files_missing: usize,
    pub files_corrupt: usize,
    pub files_pending: usize,
    /// Archived files by language.
    pub languages: BTreeMap<String, usize>,
    pub pdf_bytes: u64,
    pub first_issued: Option<NaiveDate>,
    pub last_issued: Option<NaiveDate>,
}

#[must_use]
pub fn month_stats(docs: &[DocRec]) -> MonthStats {
    let mut s = MonthStats {
        documents: docs.len(),
        first_issued: docs.iter().map(|d| d.issued_on).min(),
        last_issued: docs.iter().map(|d| d.issued_on).max(),
        ..MonthStats::default()
    };
    for f in docs.iter().flat_map(|d| &d.files) {
        match f.status.as_str() {
            "ok" => {
                s.files_ok += 1;
                *s.languages.entry(f.lang.clone()).or_default() += 1;
                s.pdf_bytes += u64::try_from(f.bytes.unwrap_or(0)).unwrap_or(0);
            }
            "missing" => s.files_missing += 1,
            "corrupt" => s.files_corrupt += 1,
            _ => s.files_pending += 1,
        }
    }
    s
}

#[must_use]
pub fn release_notes(month: &str, s: &MonthStats) -> String {
    let langs = s
        .languages
        .iter()
        .map(|(l, n)| format!("{} {n}", lang_name(l)))
        .collect::<Vec<_>>()
        .join(", ");
    let range = match (s.first_issued, s.last_issued) {
        (Some(a), Some(b)) => format!("{a} to {b}"),
        _ => "-".to_owned(),
    };
    format!(
        "Extraordinary gazettes of Sri Lanka issued {range}, archived by Lanka Data Layer.\n\n\
         - Documents: {}\n\
         - Files archived: {} ({})\n\
         - Not available upstream: {} missing, {} corrupt\n\
         - PDF size: {} ({} bytes)\n\n\
         `gazettes-{month}.tar` holds the PDFs as `<doc_id>_<lang>.pdf`; `text-{month}.tar.gz` their text; \
         `manifest-{month}.jsonl` lists every file with its SHA-256, size and source URL. \
         Metadata and checksums are in the repository's `months/{month}.json`.",
        s.documents,
        s.files_ok,
        if langs.is_empty() { "none" } else { &langs },
        s.files_missing,
        s.files_corrupt,
        human_bytes(s.pdf_bytes),
        s.pdf_bytes,
    )
}

// ── tar packing ────────────────────────────────────────────────────────────

fn entry_header(size: u64, mtime: u64) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_size(size);
    h.set_mode(0o644);
    h.set_uid(0);
    h.set_gid(0);
    h.set_mtime(mtime);
    h
}

/// Midnight UTC of a date, as a tar mtime.
#[must_use]
pub fn mtime_of(d: NaiveDate) -> u64 {
    u64::try_from(
        d.and_hms_opt(0, 0, 0)
            .map_or(0, |t| t.and_utc().timestamp()),
    )
    .unwrap_or(0)
}

const fn entry_cost(len: u64) -> u64 {
    512 + len.div_ceil(512) * 512
}

/// Writes tar parts into `dir`, starting a new part when the next entry would
/// take the current one over `max_part` bytes. A single part is named
/// `<base>.tar`, several `<base>.part1.tar`, `<base>.part2.tar`, ….
pub struct TarPacker {
    dir: PathBuf,
    base: String,
    max_part: u64,
    parts: Vec<PathBuf>,
    cur: Option<tar::Builder<BufWriter<File>>>,
    cur_size: u64,
}

impl TarPacker {
    #[must_use]
    pub fn new(dir: &Path, base: &str, max_part: u64) -> Self {
        Self {
            dir: dir.to_owned(),
            base: base.to_owned(),
            max_part,
            parts: Vec::new(),
            cur: None,
            cur_size: 0,
        }
    }

    /// Adds one file. Entries must arrive in the order they should appear.
    pub fn add(&mut self, name: &str, mtime: u64, data: &[u8]) -> Result<()> {
        let cost = entry_cost(data.len() as u64);
        ensure!(
            cost + 1024 < ASSET_LIMIT,
            "{name} ({} bytes) cannot fit in a release asset",
            data.len()
        );
        if self.cur.is_some() && self.cur_size + cost + 1024 > self.max_part {
            self.close_part()?;
        }
        if self.cur.is_none() {
            let path = self
                .dir
                .join(format!("{}.part{}.tar", self.base, self.parts.len() + 1));
            let f = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
            self.parts.push(path);
            self.cur = Some(tar::Builder::new(BufWriter::new(f)));
            self.cur_size = 0;
        }
        let mut h = entry_header(data.len() as u64, mtime);
        let b = self.cur.as_mut().context("no open part")?;
        b.append_data(&mut h, name, data)
            .with_context(|| format!("adding {name} to the tar"))?;
        self.cur_size += cost;
        Ok(())
    }

    fn close_part(&mut self) -> Result<()> {
        if let Some(b) = self.cur.take() {
            b.into_inner()?.flush()?;
        }
        Ok(())
    }

    /// Closes the last part and returns the part paths in order.
    pub fn finish(mut self) -> Result<Vec<PathBuf>> {
        self.close_part()?;
        if self.parts.len() == 1 {
            let single = self.dir.join(format!("{}.tar", self.base));
            std::fs::rename(&self.parts[0], &single)?;
            self.parts[0] = single;
        }
        Ok(self.parts)
    }
}

/// A gzip-compressed tar of in-memory entries (already sorted by name).
fn write_tar_gz(path: &Path, entries: &[(String, u64, Vec<u8>)]) -> Result<()> {
    let f = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut b = tar::Builder::new(GzEncoder::new(BufWriter::new(f), Compression::default()));
    for (name, mtime, data) in entries {
        let mut h = entry_header(data.len() as u64, *mtime);
        b.append_data(&mut h, name, data.as_slice())?;
    }
    let mut w = b.into_inner()?.finish()?;
    w.flush()?;
    Ok(())
}

// ── README ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YearStats {
    pub year: i32,
    pub documents: i64,
    pub ok: i64,
    pub missing: i64,
    pub corrupt: i64,
    /// `pending` and `failed`: still to be fetched.
    pub pending: i64,
    pub bytes: i64,
}

/// The statistics block (without its markers).
#[must_use]
pub fn render_stats(
    years: &[YearStats],
    months_published: &BTreeMap<i32, usize>,
    mirrored_at: DateTime<Utc>,
    months: &[String],
) -> String {
    let mut out = String::new();
    let total = years.iter().fold(YearStats::default(), |mut t, y| {
        t.documents += y.documents;
        t.ok += y.ok;
        t.missing += y.missing;
        t.corrupt += y.corrupt;
        t.pending += y.pending;
        t.bytes += y.bytes;
        t
    });
    out.push_str(&format!(
        "Last mirrored: {} ({}).\n\n",
        mirrored_at.format("%Y-%m-%d %H:%M UTC"),
        months.join(", ")
    ));
    out.push_str(
        "| Year | Documents | Files ok | Missing | Corrupt | Pending | Months published |\n",
    );
    out.push_str("|---:|---:|---:|---:|---:|---:|---:|\n");
    let mut published_total = 0usize;
    for y in years {
        let m = months_published.get(&y.year).copied().unwrap_or(0);
        published_total += m;
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {m} |\n",
            y.year, y.documents, y.ok, y.missing, y.corrupt, y.pending
        ));
    }
    out.push_str(&format!(
        "| **Total** | **{}** | **{}** | **{}** | **{}** | **{}** | **{published_total}** |\n\n",
        total.documents, total.ok, total.missing, total.corrupt, total.pending
    ));
    out.push_str(&format!(
        "Archive total: {} documents, {} archived files, {} of PDFs.",
        total.documents,
        total.ok,
        human_bytes(u64::try_from(total.bytes).unwrap_or(0))
    ));
    out
}

/// Replaces the text between the stats markers; everything else is kept.
pub fn replace_stats_block(readme: &str, block: &str) -> Result<String> {
    let s = readme
        .find(STATS_START)
        .with_context(|| format!("README has no {STATS_START} marker"))?;
    let e = readme
        .find(STATS_END)
        .with_context(|| format!("README has no {STATS_END} marker"))?;
    ensure!(s + STATS_START.len() <= e, "stats markers are out of order");
    Ok(format!(
        "{}{STATS_START}\n{}\n{}",
        &readme[..s],
        block.trim_end(),
        &readme[e..]
    ))
}

// ── database ───────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DocRow {
    doc_id: String,
    number: Option<String>,
    sub_number: Option<String>,
    issued_on: NaiveDate,
    title: Value,
    keywords: Option<Value>,
    upstream_id: String,
    upstream_updated_at: Option<DateTime<Utc>>,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    withdrawn_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct FileRow {
    doc_id: String,
    lang: String,
    upstream_path: String,
    status: String,
    sha256: Option<String>,
    bytes: Option<i64>,
    pages: Option<i32>,
    text_status: Option<String>,
    text_sha256: Option<String>,
}

/// Documents issued in `[from, to)` with their files, sorted by `doc_id`.
pub async fn load_docs(
    pool: &PgPool,
    col: &Collection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<DocRec>> {
    let docs: Vec<DocRow> = sqlx::query_as(
        "SELECT doc_id, number, sub_number, issued_on, title, keywords, upstream_id,
                upstream_updated_at, first_seen_at, last_seen_at, withdrawn_at
           FROM docs.documents
          WHERE collection = $1 AND issued_on >= $2 AND issued_on < $3
          ORDER BY doc_id",
    )
    .bind(&col.id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let files: Vec<FileRow> = sqlx::query_as(
        "SELECT f.doc_id, f.lang, f.upstream_path, f.status, f.sha256, f.bytes, f.pages,
                f.text_status, f.text_sha256
           FROM docs.files f JOIN docs.documents d USING (doc_id)
          WHERE d.collection = $1 AND d.issued_on >= $2 AND d.issued_on < $3
          ORDER BY f.doc_id, f.lang",
    )
    .bind(&col.id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    let mut by_doc: BTreeMap<String, Vec<FileRec>> = BTreeMap::new();
    for f in files {
        by_doc.entry(f.doc_id).or_default().push(FileRec {
            lang: f.lang,
            status: f.status,
            sha256: f.sha256,
            bytes: f.bytes,
            pages: f.pages,
            text_status: f.text_status,
            text_sha256: f.text_sha256,
            source_url: source_url(&col.cfg.file_proxy, &f.upstream_path),
        });
    }
    Ok(docs
        .into_iter()
        .map(|d| DocRec {
            files: by_doc.remove(&d.doc_id).unwrap_or_default(),
            doc_id: d.doc_id,
            number: d.number,
            sub_number: d.sub_number,
            issued_on: d.issued_on,
            title: d.title,
            keywords: d.keywords,
            upstream_id: d.upstream_id,
            upstream_updated_at: d.upstream_updated_at,
            first_seen_at: d.first_seen_at,
            last_seen_at: d.last_seen_at,
            withdrawn_at: d.withdrawn_at,
        })
        .collect())
}

#[derive(Serialize)]
struct RevisionLine {
    doc_id: String,
    seen_at: DateTime<Utc>,
    meta_hash: String,
    record: Value,
}

async fn load_revisions(
    pool: &PgPool,
    col: &Collection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<RevisionLine>> {
    let rows: Vec<(String, DateTime<Utc>, String, Value)> = sqlx::query_as(
        "SELECT r.doc_id, r.seen_at, r.meta_hash, r.record
           FROM docs.document_revisions r JOIN docs.documents d USING (doc_id)
          WHERE d.collection = $1 AND d.issued_on >= $2 AND d.issued_on < $3
          ORDER BY r.doc_id, r.seen_at",
    )
    .bind(&col.id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(doc_id, seen_at, meta_hash, record)| RevisionLine {
            doc_id,
            seen_at,
            meta_hash,
            record,
        })
        .collect())
}

async fn all_months(pool: &PgPool, col: &Collection) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT DISTINCT to_char(issued_on, 'YYYY-MM') FROM docs.documents
          WHERE collection = $1 ORDER BY 1",
    )
    .bind(&col.id)
    .fetch_all(pool)
    .await?)
}

async fn year_stats(pool: &PgPool, col: &Collection) -> Result<Vec<YearStats>> {
    let rows: Vec<(i32, i64, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT extract(year FROM d.issued_on)::int,
                count(DISTINCT d.doc_id),
                count(*) FILTER (WHERE f.status = 'ok'),
                count(*) FILTER (WHERE f.status = 'missing'),
                count(*) FILTER (WHERE f.status = 'corrupt'),
                count(*) FILTER (WHERE f.status IN ('pending', 'failed')),
                coalesce(sum(f.bytes) FILTER (WHERE f.status = 'ok'), 0)::bigint
           FROM docs.documents d LEFT JOIN docs.files f USING (doc_id)
          WHERE d.collection = $1
          GROUP BY 1 ORDER BY 1",
    )
    .bind(&col.id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(year, documents, ok, missing, corrupt, pending, bytes)| YearStats {
                year,
                documents,
                ok,
                missing,
                corrupt,
                pending,
                bytes,
            },
        )
        .collect())
}

// ── gh and git ─────────────────────────────────────────────────────────────

struct Gh {
    bin: String,
    repo: String,
}

struct Out {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

impl Gh {
    fn run(&self, args: &[&str]) -> Result<Out> {
        let o = Command::new(&self.bin)
            .args(args)
            .output()
            .with_context(|| format!("running {} (is the GitHub CLI installed?)", self.bin))?;
        Ok(Out {
            ok: o.status.success(),
            stdout: o.stdout,
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        })
    }

    fn run_ok(&self, args: &[&str]) -> Result<Vec<u8>> {
        let o = self.run(args)?;
        ensure!(
            o.ok,
            "gh {} failed: {}",
            args.first().copied().unwrap_or(""),
            o.stderr.trim()
        );
        Ok(o.stdout)
    }

    /// The release object of a tag, or `None` when GitHub answers 404.
    fn release(&self, tag: &str) -> Result<Option<Value>> {
        let path = format!("repos/{}/releases/tags/{tag}", self.repo);
        let o = self.run(&["api", &path])?;
        if o.ok {
            return Ok(Some(
                serde_json::from_slice(&o.stdout).context("release JSON")?,
            ));
        }
        if o.stderr.contains("404") || o.stderr.contains("Not Found") {
            return Ok(None);
        }
        bail!("gh api {path} failed: {}", o.stderr.trim());
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git")?;
    ensure!(
        o.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

// ── the run ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(skip)]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthResult {
    pub month: String,
    pub complete: bool,
    pub stats: MonthStats,
    pub assets: Vec<AssetInfo>,
    pub manifest_sha256: String,
    pub published: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MirrorReport {
    pub dry_run: bool,
    pub repo: String,
    /// Months whose new manifest differs from the committed one.
    pub selected: Vec<String>,
    /// Complete months already mirrored with the same manifest.
    pub unchanged: Vec<String>,
    /// Months with files still to fetch (not mirrored).
    pub incomplete: Vec<String>,
    /// Months whose documents have no file rows at all (nothing to publish).
    pub empty: Vec<String>,
    pub months: Vec<MonthResult>,
    pub committed: bool,
    /// Where the packs were built (kept for a dry run).
    pub work_dir: Option<PathBuf>,
    pub message: String,
}

struct Plan {
    month: String,
    docs: Vec<DocRec>,
    complete: bool,
    manifest: Vec<u8>,
    manifest_sha: String,
}

fn committed_manifest_sha(repo: &Path, month: &str) -> Option<String> {
    let text = std::fs::read_to_string(repo.join("months").join(format!("{month}.json"))).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("manifest_sha256")?.as_str().map(str::to_owned)
}

/// Mirrors the collection's complete, changed months. See the module docs.
pub async fn run(
    pool: &PgPool,
    store: &dyn BlobStore,
    collection: &str,
    opts: &MirrorOptions,
) -> Result<MirrorReport> {
    let col = load_collection(pool, collection).await?;
    let mut report = MirrorReport {
        dry_run: opts.dry_run,
        repo: opts.github_repo.clone(),
        ..MirrorReport::default()
    };
    ensure!(
        opts.repo_dir.join(".git").exists(),
        "{} is not a git clone (clone the archive repository first)",
        opts.repo_dir.display()
    );
    let explicit = !opts.months.is_empty();
    for m in &opts.months {
        month_range(m)?;
    }
    ensure!(
        git(&opts.repo_dir, &["diff", "--cached", "--name-only"])?
            .trim()
            .is_empty(),
        "the index of {} has staged changes; commit or reset them first",
        opts.repo_dir.display()
    );
    git(&opts.repo_dir, &["pull", "--ff-only"])?;

    // ── select ──
    let mut candidates = if explicit {
        opts.months.clone()
    } else {
        all_months(pool, &col).await?
    };
    candidates.sort();
    candidates.dedup();
    let mut plans: Vec<Plan> = Vec::new();
    for month in candidates {
        if opts.max_months.is_some_and(|n| plans.len() >= n) {
            break;
        }
        let (from, to) = month_range(&month)?;
        let docs = load_docs(pool, &col, from, to).await?;
        ensure!(
            !docs.is_empty() || !explicit,
            "no documents were issued in {month}"
        );
        if docs.is_empty() {
            continue;
        }
        let complete = month_complete(&docs);
        if !complete && !opts.allow_incomplete {
            let todo = month_stats(&docs).files_pending;
            ensure!(
                !explicit,
                "{month} is not complete: {todo} files are pending or failed \
                 (run `lanka docs harvest {collection} --mode files --month {month}`, \
                 or pass --allow-incomplete for a test)"
            );
            report.incomplete.push(month);
            continue;
        }
        let lines = manifest_lines(&docs);
        if lines.is_empty() {
            ensure!(!explicit, "{month}: its documents have no files to publish");
            report.empty.push(month);
            continue;
        }
        let manifest = to_jsonl(&lines)?;
        let manifest_sha = sha_hex(&manifest);
        if committed_manifest_sha(&opts.repo_dir, &month).as_deref() == Some(manifest_sha.as_str())
        {
            report.unchanged.push(month);
            continue;
        }
        plans.push(Plan {
            month,
            docs,
            complete,
            manifest,
            manifest_sha,
        });
    }
    report.selected = plans.iter().map(|p| p.month.clone()).collect();
    if plans.is_empty() {
        report.message = format!(
            "nothing to mirror: {} unchanged, {} incomplete, {} without files",
            report.unchanged.len(),
            report.incomplete.len(),
            report.empty.len()
        );
        return Ok(report);
    }

    // ── pack, verify, publish ──
    let work = opts
        .work_dir
        .clone()
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("lanka-mirror-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    report.work_dir = Some(work.clone());
    let gh = Gh {
        bin: opts.gh_bin.clone(),
        repo: opts.github_repo.clone(),
    };
    let result = publish_all(store, &gh, opts, &plans, &work, &mut report).await;
    if !opts.dry_run {
        let _ = std::fs::remove_dir_all(&work);
        report.work_dir = None;
    }
    result?;
    if opts.dry_run {
        report.message = format!(
            "dry run: would mirror {} (packs in {})",
            report.selected.join(", "),
            work.display()
        );
        return Ok(report);
    }

    // ── git ──
    commit_all(pool, &col, opts, &plans, &report).await?;
    report.committed = true;
    report.message = format!("mirrored {}", report.selected.join(", "));
    Ok(report)
}

async fn publish_all(
    store: &dyn BlobStore,
    gh: &Gh,
    opts: &MirrorOptions,
    plans: &[Plan],
    work: &Path,
    report: &mut MirrorReport,
) -> Result<()> {
    for p in plans {
        let dir = work.join(&p.month);
        std::fs::create_dir_all(&dir)?;
        eprintln!("mirror {}: verifying and packing", p.month);
        let assets = pack_month(store, p, &dir, opts.max_part_bytes)
            .await
            .with_context(|| format!("month {}: nothing was published", p.month))?;
        let mut res = MonthResult {
            month: p.month.clone(),
            complete: p.complete,
            stats: month_stats(&p.docs),
            assets,
            manifest_sha256: p.manifest_sha.clone(),
            published: false,
        };
        if !opts.dry_run {
            eprintln!("mirror {}: uploading {} assets", p.month, res.assets.len());
            upload_month(gh, &res).with_context(|| format!("month {}", p.month))?;
            res.published = true;
        }
        report.months.push(res);
    }
    Ok(())
}

/// Reads and verifies every archived file of the month, then builds the
/// assets in `dir`. Any mismatch stops the month.
async fn pack_month(
    store: &dyn BlobStore,
    p: &Plan,
    dir: &Path,
    max_part: u64,
) -> Result<Vec<AssetInfo>> {
    let month = &p.month;
    let mut packer = TarPacker::new(dir, &format!("gazettes-{month}"), max_part);
    let mut texts: Vec<(String, u64, Vec<u8>)> = Vec::new();
    let mut any_pdf = false;
    let mut docs: Vec<&DocRec> = p.docs.iter().collect();
    docs.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
    for d in docs {
        let mtime = mtime_of(d.issued_on);
        let mut files: Vec<&FileRec> = d.files.iter().filter(|f| f.status == "ok").collect();
        files.sort_by(|a, b| a.lang.cmp(&b.lang));
        for f in files {
            let who = format!("{} {}", d.doc_id, f.lang);
            let sha = f
                .sha256
                .as_deref()
                .with_context(|| format!("{who}: status ok but no sha256"))?;
            let want_bytes = f
                .bytes
                .with_context(|| format!("{who}: status ok but no byte count"))?;
            let r = blob_ref(sha, want_bytes)?;
            let data = store
                .get(&r)
                .await
                .with_context(|| format!("{who}: reading blob {sha}"))?;
            let got = sha_hex(&data);
            ensure!(
                got.eq_ignore_ascii_case(sha),
                "{who}: blob sha256 {got} does not match the database ({sha})"
            );
            ensure!(
                i64::try_from(data.len()).ok() == Some(want_bytes),
                "{who}: blob is {} bytes, the database says {want_bytes}",
                data.len()
            );
            packer.add(&pdf_name(&d.doc_id, &f.lang), mtime, &data)?;
            any_pdf = true;
            if let Some(tsha) = f.text_sha256.as_deref() {
                let raw = hex::decode(tsha).with_context(|| format!("{who}: text sha256"))?;
                let sha256: [u8; 32] = raw
                    .try_into()
                    .map_err(|_| anyhow!("{who}: text sha256 is not 32 bytes"))?;
                let tr = BlobRef {
                    ns: Ns::Text,
                    key: Ns::Text.cas_key(&sha256),
                    sha256,
                    bytes: 0,
                };
                let text = store
                    .get(&tr)
                    .await
                    .with_context(|| format!("{who}: reading text blob {tsha}"))?;
                let got = sha_hex(&text);
                ensure!(
                    got.eq_ignore_ascii_case(tsha),
                    "{who}: text sha256 {got} does not match the database ({tsha})"
                );
                texts.push((text_name(&d.doc_id, &f.lang), mtime, text.to_vec()));
            }
        }
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    if any_pdf {
        paths.extend(packer.finish()?);
    }
    if !texts.is_empty() {
        texts.sort_by(|a, b| a.0.cmp(&b.0));
        let path = dir.join(format!("text-{month}.tar.gz"));
        write_tar_gz(&path, &texts)?;
        paths.push(path);
    }
    let manifest_path = dir.join(format!("manifest-{month}.jsonl"));
    std::fs::write(&manifest_path, &p.manifest)?;
    paths.push(manifest_path);

    let mut assets = Vec::new();
    for path in paths {
        let (sha256, bytes) = sha_file(&path)?;
        ensure!(
            bytes < ASSET_LIMIT,
            "{} is {bytes} bytes, over GitHub's 2 GiB asset limit",
            path.display()
        );
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("asset name")?
            .to_owned();
        assets.push(AssetInfo {
            name,
            bytes,
            sha256,
            path,
        });
    }
    Ok(assets)
}

fn upload_month(gh: &Gh, res: &MonthResult) -> Result<()> {
    let tag = res.month.as_str();
    let title = month_title(tag)?;
    let notes = release_notes(tag, &res.stats);
    let repo = gh.repo.as_str();
    if gh.release(tag)?.is_none() {
        gh.run_ok(&[
            "release", "create", tag, "-R", repo, "--title", &title, "--notes", &notes,
        ])?;
    } else {
        gh.run_ok(&[
            "release", "edit", tag, "-R", repo, "--title", &title, "--notes", &notes,
        ])?;
    }
    let paths: Vec<String> = res
        .assets
        .iter()
        .map(|a| a.path.display().to_string())
        .collect();
    let mut args: Vec<&str> = vec!["release", "upload", tag];
    args.extend(paths.iter().map(String::as_str));
    args.extend(["--clobber", "-R", repo]);
    gh.run_ok(&args)?;

    // every expected asset exists on GitHub with the exact size
    let release = gh
        .release(tag)?
        .with_context(|| format!("release {tag} is not there after the upload"))?;
    let listed: BTreeMap<String, u64> = release
        .get("assets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x.get("name")?.as_str()?.to_owned(),
                        x.get("size")?.as_u64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    for a in &res.assets {
        match listed.get(&a.name) {
            Some(&size) if size == a.bytes => {}
            Some(&size) => bail!(
                "asset {} is {size} bytes on GitHub, {} locally",
                a.name,
                a.bytes
            ),
            None => bail!("asset {} is not on release {tag}", a.name),
        }
    }
    let expected: Vec<&str> = res.assets.iter().map(|a| a.name.as_str()).collect();
    for name in listed.keys() {
        if !expected.contains(&name.as_str()) {
            eprintln!("warning: release {tag} has an extra asset {name} (left in place)");
        }
    }

    // the manifest, downloaded back, is the one we built
    let name = format!("manifest-{tag}.jsonl");
    let back = gh.run_ok(&[
        "release", "download", tag, "-R", repo, "-p", &name, "-O", "-",
    ])?;
    let got = sha_hex(&back);
    ensure!(
        got == res.manifest_sha256,
        "the manifest downloaded from release {tag} has sha256 {got}, expected {}",
        res.manifest_sha256
    );
    Ok(())
}

async fn commit_all(
    pool: &PgPool,
    col: &Collection,
    opts: &MirrorOptions,
    plans: &[Plan],
    report: &MirrorReport,
) -> Result<()> {
    let repo = &opts.repo_dir;
    git(repo, &["pull", "--ff-only"])?;
    let now = Utc::now();
    let mirrored_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);

    std::fs::create_dir_all(repo.join("months"))?;
    for res in &report.months {
        let plan = plans
            .iter()
            .find(|p| p.month == res.month)
            .context("plan of a mirrored month")?;
        let doc = json!({
            "tag": res.month,
            "complete": plan.complete,
            "documents": res.stats.documents,
            "files": {
                "ok": res.stats.files_ok,
                "missing": res.stats.files_missing,
                "corrupt": res.stats.files_corrupt,
                "pending": res.stats.files_pending,
            },
            "languages": res.stats.languages,
            "pdf_bytes": res.stats.pdf_bytes,
            "first_issued": res.stats.first_issued,
            "last_issued": res.stats.last_issued,
            "assets": res.assets.iter().map(|a| json!({
                "name": a.name, "bytes": a.bytes, "sha256": a.sha256,
            })).collect::<Vec<_>>(),
            "manifest_sha256": res.manifest_sha256,
            "mirrored_at": mirrored_at,
        });
        let mut text = serde_json::to_string_pretty(&doc)?;
        text.push('\n');
        std::fs::write(
            repo.join("months").join(format!("{}.json", res.month)),
            text,
        )?;
    }

    // metadata of every year touched, rebuilt from the database
    let mut years: Vec<i32> = plans
        .iter()
        .map(|p| month_range(&p.month).map(|(f, _)| f.year()))
        .collect::<Result<_>>()?;
    years.sort_unstable();
    years.dedup();
    for y in &years {
        let (from, to) = year_range(*y)?;
        let docs = load_docs(pool, col, from, to).await?;
        let revs = load_revisions(pool, col, from, to).await?;
        for (dir, bytes) in [
            ("documents", to_jsonl(&docs)?),
            ("revisions", to_jsonl(&revs)?),
        ] {
            let d = repo.join("metadata").join(dir);
            std::fs::create_dir_all(&d)?;
            std::fs::write(d.join(format!("{y}.jsonl")), bytes)?;
        }
    }

    // README statistics
    let mut published: BTreeMap<i32, usize> = BTreeMap::new();
    for e in std::fs::read_dir(repo.join("months"))? {
        let name = e?.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".json")
            && let Ok((f, _)) = month_range(stem)
        {
            *published.entry(f.year()).or_default() += 1;
        }
    }
    let stats = year_stats(pool, col).await?;
    let block = render_stats(&stats, &published, now, &report.selected);
    let readme_path = repo.join("README.md");
    let readme = std::fs::read_to_string(&readme_path).context("reading README.md")?;
    std::fs::write(&readme_path, replace_stats_block(&readme, &block)?)?;

    git(repo, &["add", "--", "months", "metadata", "README.md"])?;
    let msg = format!("data: mirror {}", report.selected.join(", "));
    git(repo, &["commit", "-m", &msg])?;
    git(repo, &["push"])?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn file(lang: &str, status: &str, sha: Option<&str>) -> FileRec {
        FileRec {
            lang: lang.to_owned(),
            status: status.to_owned(),
            sha256: sha.map(str::to_owned),
            bytes: sha.map(|_| 10),
            pages: sha.map(|_| 2),
            text_status: sha.map(|_| "text".to_owned()),
            text_sha256: sha.map(|_| "bb".to_owned()),
            source_url: format!("https://x/?file={lang}"),
        }
    }

    fn doc(id: &str, files: Vec<FileRec>) -> DocRec {
        DocRec {
            doc_id: id.to_owned(),
            number: None,
            sub_number: None,
            issued_on: NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            title: json!({"en": "t"}),
            keywords: None,
            upstream_id: "u".to_owned(),
            upstream_updated_at: None,
            first_seen_at: DateTime::UNIX_EPOCH,
            last_seen_at: DateTime::UNIX_EPOCH,
            withdrawn_at: None,
            files,
        }
    }

    #[test]
    fn completeness_counts_only_pending_and_failed() {
        let settled = vec![doc(
            "a",
            vec![
                file("en", "ok", Some("aa")),
                file("si", "missing", None),
                file("ta", "corrupt", None),
            ],
        )];
        assert!(month_complete(&settled));
        for open in ["pending", "failed"] {
            let docs = vec![
                doc("a", vec![file("en", "ok", Some("aa"))]),
                doc("b", vec![file("en", open, None)]),
            ];
            assert!(!month_complete(&docs), "{open}");
        }
        assert!(month_complete(&[]));
    }

    #[test]
    fn manifest_is_sorted_and_names_only_archived_files() {
        let docs = vec![
            doc(
                "gze-2-01",
                vec![file("ta", "ok", Some("cc")), file("en", "missing", None)],
            ),
            doc("gze-1-01", vec![file("si", "ok", Some("aa"))]),
        ];
        let lines = manifest_lines(&docs);
        let keys: Vec<_> = lines
            .iter()
            .map(|l| (l.doc_id.as_str(), l.lang.as_str()))
            .collect();
        assert_eq!(
            keys,
            [("gze-1-01", "si"), ("gze-2-01", "en"), ("gze-2-01", "ta")]
        );
        let bytes = to_jsonl(&lines).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[0]["name"], "gze-1-01_si.pdf");
        assert!(rows[1].get("name").is_none());
        assert_eq!(rows[1]["status"], "missing");
        assert_eq!(rows[1]["sha256"], Value::Null);
        assert!(text.ends_with('\n'));
        // the same input gives the same manifest
        assert_eq!(to_jsonl(&manifest_lines(&docs)).unwrap(), text.into_bytes());
    }

    #[test]
    fn source_urls_encode_the_path() {
        assert_eq!(
            source_url(
                "https://documents.gov.lk/api/content-file-proxy?file=",
                "extra-gazette-content/G 48538 (E)_1794986733.pdf"
            ),
            "https://documents.gov.lk/api/content-file-proxy?file=extra-gazette-content%2FG%2048538%20%28E%29_1794986733.pdf"
        );
    }

    #[test]
    fn months_parse_strictly() {
        let (a, b) = month_range("2026-12").unwrap();
        assert_eq!(
            (a.to_string(), b.to_string()),
            ("2026-12-01".into(), "2027-01-01".into())
        );
        for bad in ["2026-13", "2026-1", "26-01", "2026", "2026-00", "x-y"] {
            assert!(month_range(bad).is_err(), "{bad}");
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("lanka-mirror-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn pack(dir: &Path, max: u64, n: usize) -> Vec<PathBuf> {
        let mut p = TarPacker::new(dir, "gazettes-2026-11", max);
        for i in 0..n {
            let data = vec![u8::try_from(i).unwrap(); 700];
            p.add(&format!("gze-{i:03}_en.pdf"), 1_793_000_000, &data)
                .unwrap();
        }
        p.finish().unwrap()
    }

    #[test]
    fn tar_output_is_deterministic_with_fixed_headers() {
        let (a, b) = (scratch("det-a"), scratch("det-b"));
        let pa = pack(&a, MAX_PART_BYTES, 3);
        let pb = pack(&b, MAX_PART_BYTES, 3);
        assert_eq!(pa.len(), 1);
        assert_eq!(pa[0].file_name().unwrap(), "gazettes-2026-11.tar");
        let (ba, bb) = (
            std::fs::read(&pa[0]).unwrap(),
            std::fs::read(&pb[0]).unwrap(),
        );
        assert_eq!(ba, bb);
        let mut ar = tar::Archive::new(ba.as_slice());
        let mut names = Vec::new();
        for e in ar.entries().unwrap() {
            let e = e.unwrap();
            let h = e.header();
            assert_eq!(
                (h.uid().unwrap(), h.gid().unwrap(), h.mode().unwrap()),
                (0, 0, 0o644)
            );
            assert_eq!(h.mtime().unwrap(), 1_793_000_000);
            assert!(h.username().unwrap().unwrap_or("").is_empty());
            assert!(h.groupname().unwrap().unwrap_or("").is_empty());
            names.push(e.path().unwrap().to_string_lossy().into_owned());
        }
        assert_eq!(
            names,
            ["gze-000_en.pdf", "gze-001_en.pdf", "gze-002_en.pdf"]
        );
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn text_tar_gz_is_deterministic() {
        let d = scratch("gz");
        let entries = vec![("a_en.txt".to_owned(), 5u64, b"hello\x0cworld".to_vec())];
        write_tar_gz(&d.join("1.tar.gz"), &entries).unwrap();
        write_tar_gz(&d.join("2.tar.gz"), &entries).unwrap();
        assert_eq!(
            std::fs::read(d.join("1.tar.gz")).unwrap(),
            std::fs::read(d.join("2.tar.gz")).unwrap()
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn tars_split_into_parts_at_the_threshold() {
        // one entry costs 512 header + 1024 data = 1536; two plus the 1024 trailer = 4096
        let d = scratch("split");
        let parts = pack(&d, 4096, 5);
        let names: Vec<_> = parts
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "gazettes-2026-11.part1.tar",
                "gazettes-2026-11.part2.tar",
                "gazettes-2026-11.part3.tar"
            ]
        );
        let mut all = Vec::new();
        for p in &parts {
            let bytes = std::fs::read(p).unwrap();
            assert!(bytes.len() <= 4096, "{} is {}", p.display(), bytes.len());
            let mut ar = tar::Archive::new(bytes.as_slice());
            for e in ar.entries().unwrap() {
                all.push(e.unwrap().path().unwrap().to_string_lossy().into_owned());
            }
        }
        assert_eq!(all.len(), 5);
        assert_eq!(all[0], "gze-000_en.pdf");
        assert_eq!(all[4], "gze-004_en.pdf");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn mtime_is_midnight_utc_of_the_issue_date() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        assert_eq!(mtime_of(d) % 86_400, 0);
        assert_eq!(
            DateTime::from_timestamp(i64::try_from(mtime_of(d)).unwrap(), 0)
                .unwrap()
                .date_naive(),
            d
        );
    }

    #[test]
    fn stats_block_is_replaced_and_the_rest_kept() {
        let readme = "# t\n\nprose\n\n<!-- stats:start -->\n*Not yet mirrored.*\n<!-- stats:end -->\n\ntail\n";
        let years = vec![
            YearStats {
                year: 2025,
                documents: 3,
                ok: 5,
                missing: 1,
                corrupt: 0,
                pending: 2,
                bytes: 2048,
            },
            YearStats {
                year: 2026,
                documents: 4,
                ok: 8,
                missing: 0,
                corrupt: 1,
                pending: 0,
                bytes: 1024,
            },
        ];
        let published = BTreeMap::from([(2026, 2)]);
        let block = render_stats(
            &years,
            &published,
            DateTime::UNIX_EPOCH,
            &["2026-10".into(), "2026-11".into()],
        );
        assert!(block.contains("| 2025 | 3 | 5 | 1 | 0 | 2 | 0 |"));
        assert!(block.contains("| 2026 | 4 | 8 | 0 | 1 | 0 | 2 |"));
        assert!(block.contains("| **Total** | **7** | **13** | **1** | **1** | **2** | **2** |"));
        assert!(block.contains("Last mirrored: 1970-01-01 00:00 UTC (2026-10, 2026-11)"));
        assert!(block.contains("Archive total: 7 documents, 13 archived files, 3.0 KiB"));
        let out = replace_stats_block(readme, &block).unwrap();
        assert!(out.starts_with("# t\n\nprose\n\n<!-- stats:start -->\nLast mirrored"));
        assert!(out.ends_with("<!-- stats:end -->\n\ntail\n"));
        assert!(!out.contains("Not yet mirrored"));
        // replacing again with the same block changes nothing
        assert_eq!(replace_stats_block(&out, &block).unwrap(), out);
        assert!(replace_stats_block("no markers", &block).is_err());
    }

    #[test]
    fn release_notes_name_languages_and_range() {
        let docs = vec![
            doc(
                "a",
                vec![file("en", "ok", Some("aa")), file("si", "ok", Some("bb"))],
            ),
            doc("b", vec![file("ta", "missing", None)]),
        ];
        let n = release_notes("2026-11", &month_stats(&docs));
        assert!(n.contains("English 1, Sinhala 1"));
        assert!(n.contains("1 missing"));
        assert!(n.contains("2026-11-03 to 2026-11-03"));
        assert_eq!(
            month_title("2026-11").unwrap(),
            "Extraordinary gazettes, November 2026"
        );
    }
}
