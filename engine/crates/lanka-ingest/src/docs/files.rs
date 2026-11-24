//! Files (13 §1.2 step 4): download the PDFs into the blob store, read their
//! text through the document sandbox, and index it.
//!
//! - Each file is streamed from the file proxy into `raw/<sha256>`
//!   (content-addressed, never held in memory) and checked for the `%PDF-`
//!   magic. 404 → `missing`; not a PDF or unreadable → `corrupt` (the blob is
//!   kept as evidence); anything else → `failed` with `last_error`, retried by
//!   later runs while `attempts < 5`.
//! - Text: the text layer through `pdf_words`; a page without text gets OCR in
//!   the language of the file (si → sin, ta → tam, en → eng), at most
//!   `ocr_page_cap` pages per file, the rest skipped and noted in `note`. The
//!   text (pages separated by `\f`) is stored as a content-addressed blob in
//!   the extract namespace and indexed in `docs.search` with the `simple`
//!   configuration.
//! - Without a document sandbox (`LANKA_EXTRACTD=require` and no socket, or
//!   the daemon is down) the PDF is still archived (`ok`, `text_status`
//!   `none`); `lanka docs harvest <c> --mode text` fills the text later.
//!
//! `text_status`: `text` (text layer only), `ocr` (some page needed OCR),
//! `empty` (no text found), `none` (not read yet).

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use futures::StreamExt;
use lanka_extractd::{Sandbox, Word, Words};
use lanka_store::{BlobRef, BlobStore, Ns};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::io::AsyncWriteExt;

use super::harvest::Collection;
use super::percent_encode;

/// Files are retried by later runs until they have failed this many times.
pub const MAX_ATTEMPTS: i32 = 5;

/// A page with fewer non-blank characters than this has no text layer.
const MIN_PAGE_CHARS: usize = 5;

const OCR_DPI: u32 = 300;

#[derive(Debug, Clone)]
pub struct FileOpts {
    pub limit: Option<usize>,
    /// Only read the text of files already archived.
    pub text_only: bool,
    pub concurrency: usize,
    pub ocr_page_cap: usize,
    /// Only files of documents issued in `[from, to)`.
    pub range: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
}

#[derive(Debug, Clone, Default)]
pub struct FileReport {
    pub ok: i32,
    pub failed: i32,
    pub missing: i32,
    pub corrupt: i32,
    pub text_done: i32,
}

#[derive(sqlx::FromRow)]
struct Row {
    doc_id: String,
    lang: String,
    upstream_path: String,
    sha256: Option<String>,
    bytes: Option<i64>,
}

enum Outcome {
    Ok { text: bool },
    Missing,
    Corrupt,
    Failed,
}

struct Ctx<'a> {
    pool: &'a PgPool,
    store: &'a dyn BlobStore,
    http: reqwest::Client,
    proxy: &'a str,
    sandbox: Option<Sandbox>,
    ocr_cap: usize,
}

pub async fn run(
    pool: &PgPool,
    store: &dyn BlobStore,
    col: &Collection,
    opts: &FileOpts,
) -> Result<FileReport> {
    let sandbox = open_sandbox().await;
    if opts.text_only && sandbox.is_none() {
        bail!(
            "text extraction needs the document sandbox (start the tools container, or unset LANKA_EXTRACTD=require for local tools)"
        );
    }
    let limit = opts.limit.map_or(i64::MAX, |l| l as i64);
    let (from, to) = match opts.range {
        Some((f, t)) => (Some(f), Some(t)),
        None => (None, None),
    };
    let rows: Vec<Row> = if opts.text_only {
        sqlx::query_as(
            "SELECT f.doc_id, f.lang, f.upstream_path, f.sha256, f.bytes
               FROM docs.files f JOIN docs.documents d USING (doc_id)
              WHERE d.collection = $1 AND f.status = 'ok' AND f.sha256 IS NOT NULL
                AND f.text_status IS NOT DISTINCT FROM 'none'
                AND ($3::date IS NULL OR (d.issued_on >= $3 AND d.issued_on < $4))
              ORDER BY d.issued_on DESC, f.doc_id, f.lang LIMIT $2",
        )
        .bind(&col.id)
        .bind(limit)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT f.doc_id, f.lang, f.upstream_path, f.sha256, f.bytes
               FROM docs.files f JOIN docs.documents d USING (doc_id)
              WHERE d.collection = $1 AND f.status IN ('pending', 'failed') AND f.attempts < $3
                AND ($4::date IS NULL OR (d.issued_on >= $4 AND d.issued_on < $5))
              ORDER BY d.issued_on DESC, f.doc_id, f.lang LIMIT $2",
        )
        .bind(&col.id)
        .bind(limit)
        .bind(MAX_ATTEMPTS)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?
    };
    tracing::info!(
        files = rows.len(),
        text_only = opts.text_only,
        "files to process"
    );
    let ctx = Ctx {
        pool,
        store,
        http: super::client(Duration::from_secs(300))?,
        proxy: &col.cfg.file_proxy,
        sandbox,
        ocr_cap: opts.ocr_page_cap,
    };
    let text_only = opts.text_only;
    let mut report = FileReport::default();
    let ctx = &ctx;
    let mut results = futures::stream::iter(rows)
        .map(|row| async move { process(ctx, row, text_only).await })
        .buffer_unordered(opts.concurrency.max(1));
    while let Some(o) = results.next().await {
        match o {
            Outcome::Ok { text } => {
                report.ok += 1;
                report.text_done += i32::from(text);
            }
            Outcome::Missing => report.missing += 1,
            Outcome::Corrupt => report.corrupt += 1,
            Outcome::Failed => report.failed += 1,
        }
    }
    Ok(report)
}

/// The document sandbox, when there is one that answers.
async fn open_sandbox() -> Option<Sandbox> {
    match Sandbox::from_env() {
        Ok(sb) => {
            if probe(&sb).await {
                tracing::info!("document tools: {}", sb.describe());
                Some(sb)
            } else {
                tracing::warn!(
                    "document sandbox ({}) does not answer: PDFs are archived without text",
                    sb.describe()
                );
                None
            }
        }
        Err(e) => {
            tracing::warn!("no document sandbox ({e:#}): PDFs are archived without text");
            None
        }
    }
}

/// A one-page PDF without text: the smallest document `pdfinfo` reads.
const BLANK_PDF: &[u8] = b"%PDF-1.1\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 72 72]>>endobj\ntrailer<</Root 1 0 R/Size 4>>\n%%EOF\n";

async fn probe(sb: &Sandbox) -> bool {
    match sb.page_count(BLANK_PDF).await {
        Ok(1) => true,
        Ok(n) => {
            tracing::warn!(pages = n, "sandbox probe read an unexpected page count");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "sandbox probe failed");
            false
        }
    }
}

async fn process(ctx: &Ctx<'_>, row: Row, text_only: bool) -> Outcome {
    let key = format!("{}/{}", row.doc_id, row.lang);
    let out = if text_only {
        read_text_only(ctx, &row).await
    } else {
        fetch_and_read(ctx, &row).await
    };
    match out {
        Ok(o) => {
            tracing::info!(file = key, "{}", describe(&o));
            o
        }
        Err(e) => {
            tracing::warn!(file = key, error = %format!("{e:#}"), "file failed");
            let msg = format!("{e:#}");
            let _ = sqlx::query(
                "UPDATE docs.files SET status = 'failed', attempts = attempts + 1, last_error = $3
                  WHERE doc_id = $1 AND lang = $2",
            )
            .bind(&row.doc_id)
            .bind(&row.lang)
            .bind(&msg)
            .execute(ctx.pool)
            .await;
            Outcome::Failed
        }
    }
}

fn describe(o: &Outcome) -> &'static str {
    match o {
        Outcome::Ok { text: true } => "archived, text read",
        Outcome::Ok { text: false } => "archived, no text yet",
        Outcome::Missing => "missing upstream (404)",
        Outcome::Corrupt => "corrupt",
        Outcome::Failed => "failed",
    }
}

async fn fetch_and_read(ctx: &Ctx<'_>, row: &Row) -> Result<Outcome> {
    let url = format!("{}{}", ctx.proxy, percent_encode(&row.upstream_path));
    match download(ctx, &url).await? {
        Fetched::Missing => {
            sqlx::query(
                "UPDATE docs.files SET status = 'missing', attempts = attempts + 1, last_error = 'HTTP 404'
                  WHERE doc_id = $1 AND lang = $2",
            )
            .bind(&row.doc_id)
            .bind(&row.lang)
            .execute(ctx.pool)
            .await?;
            Ok(Outcome::Missing)
        }
        Fetched::Blob { blob, pdf: false } => {
            sqlx::query(
                "UPDATE docs.files SET status = 'corrupt', sha256 = $3, bytes = $4, fetched_at = now(),
                        attempts = attempts + 1, last_error = 'not a PDF (no %PDF- magic)',
                        text_status = NULL, text_sha256 = NULL
                  WHERE doc_id = $1 AND lang = $2",
            )
            .bind(&row.doc_id)
            .bind(&row.lang)
            .bind(blob.hex())
            .bind(blob.bytes as i64)
            .execute(ctx.pool)
            .await?;
            Ok(Outcome::Corrupt)
        }
        Fetched::Blob { blob, pdf: true } => {
            sqlx::query(
                "UPDATE docs.files SET status = 'ok', sha256 = $3, bytes = $4, fetched_at = now(),
                        attempts = attempts + 1, last_error = NULL, text_status = 'none',
                        text_sha256 = NULL, pages = NULL, note = NULL
                  WHERE doc_id = $1 AND lang = $2",
            )
            .bind(&row.doc_id)
            .bind(&row.lang)
            .bind(blob.hex())
            .bind(blob.bytes as i64)
            .execute(ctx.pool)
            .await?;
            read_blob(ctx, &row.doc_id, &row.lang, &blob).await
        }
    }
}

async fn read_text_only(ctx: &Ctx<'_>, row: &Row) -> Result<Outcome> {
    let sha = row.sha256.as_deref().context("file has no sha256")?;
    let blob = blob_ref(sha, row.bytes.unwrap_or(0))?;
    read_blob(ctx, &row.doc_id, &row.lang, &blob).await
}

pub fn blob_ref(sha_hex: &str, bytes: i64) -> Result<BlobRef> {
    let raw = hex::decode(sha_hex).context("sha256 is not hex")?;
    let sha256: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow!("sha256 is not 32 bytes"))?;
    Ok(BlobRef {
        ns: Ns::Raw,
        key: Ns::Raw.cas_key(&sha256),
        sha256,
        bytes: u64::try_from(bytes).unwrap_or(0),
    })
}

/// Reads the text of an archived PDF and records the result.
async fn read_blob(ctx: &Ctx<'_>, doc_id: &str, lang: &str, blob: &BlobRef) -> Result<Outcome> {
    let Some(sb) = &ctx.sandbox else {
        set_text(
            ctx,
            doc_id,
            lang,
            None,
            "none",
            None,
            Some("no document sandbox: text not read"),
        )
        .await?;
        return Ok(Outcome::Ok { text: false });
    };
    let pdf = ctx.store.get(blob).await?;
    match extract(sb, &pdf, lang, ctx.ocr_cap).await {
        Err(TextError::Unreadable(e)) => {
            sqlx::query(
                "UPDATE docs.files SET status = 'corrupt', last_error = $3, text_status = NULL
                  WHERE doc_id = $1 AND lang = $2",
            )
            .bind(doc_id)
            .bind(lang)
            .bind(format!("unreadable PDF: {e:#}"))
            .execute(ctx.pool)
            .await?;
            Ok(Outcome::Corrupt)
        }
        Err(TextError::Failed(e)) => {
            set_text(
                ctx,
                doc_id,
                lang,
                None,
                "none",
                None,
                Some(&format!("text extraction failed: {e:#}")),
            )
            .await?;
            Ok(Outcome::Ok { text: false })
        }
        Ok(x) => {
            let (blob_sha, text_note) = if x.status == "empty" {
                (None, None)
            } else {
                let (b, _) = ctx
                    .store
                    .put_cas(Ns::Text, Bytes::from(x.text.clone().into_bytes()))
                    .await?;
                let note = index_text(ctx.pool, doc_id, lang, &x.text).await?;
                (Some(b.hex()), note)
            };
            if x.status == "empty" {
                sqlx::query("DELETE FROM docs.search WHERE doc_id = $1 AND lang = $2")
                    .bind(doc_id)
                    .bind(lang)
                    .execute(ctx.pool)
                    .await?;
            }
            let note = [x.note, text_note]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("; ");
            set_text(
                ctx,
                doc_id,
                lang,
                Some(i32::try_from(x.pages).unwrap_or(i32::MAX)),
                x.status,
                blob_sha.as_deref(),
                (!note.is_empty()).then_some(note.as_str()),
            )
            .await?;
            Ok(Outcome::Ok { text: true })
        }
    }
}

async fn set_text(
    ctx: &Ctx<'_>,
    doc_id: &str,
    lang: &str,
    pages: Option<i32>,
    status: &str,
    text_sha: Option<&str>,
    note: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE docs.files SET pages = coalesce($3, pages), text_status = $4, text_sha256 = $5, note = $6
          WHERE doc_id = $1 AND lang = $2",
    )
    .bind(doc_id)
    .bind(lang)
    .bind(pages)
    .bind(status)
    .bind(text_sha)
    .bind(note)
    .execute(ctx.pool)
    .await?;
    Ok(())
}

/// Upserts the tsvector of a document's text. A tsvector holds at most 1 MiB:
/// a very long text is cut until it fits, and the note says so.
pub async fn index_text(
    pool: &PgPool,
    doc_id: &str,
    lang: &str,
    text: &str,
) -> Result<Option<String>> {
    let mut cut = text;
    let mut truncated = false;
    for _ in 0..8 {
        let res = sqlx::query(
            "INSERT INTO docs.search (doc_id, lang, tsv) VALUES ($1, $2, to_tsvector('simple', $3))
             ON CONFLICT (doc_id, lang) DO UPDATE SET tsv = EXCLUDED.tsv",
        )
        .bind(doc_id)
        .bind(lang)
        .bind(cut)
        .execute(pool)
        .await;
        match res {
            Ok(_) => {
                return Ok(truncated.then(|| {
                    format!(
                        "search index covers the first {} characters",
                        cut.chars().count()
                    )
                }));
            }
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("54000") => {
                let mut end = cut.len() / 2;
                while !cut.is_char_boundary(end) {
                    end -= 1;
                }
                cut = &cut[..end];
                truncated = true;
            }
            Err(e) => return Err(e.into()),
        }
    }
    bail!("text of {doc_id}/{lang} does not fit a tsvector")
}

// ── download ───────────────────────────────────────────────────────────────

enum Fetched {
    Missing,
    Blob { blob: BlobRef, pdf: bool },
}

enum GetError {
    Retry(anyhow::Error),
    Fatal(anyhow::Error),
}

const FILE_BACKOFF: [u64; 2] = [5, 30];

async fn download(ctx: &Ctx<'_>, url: &str) -> Result<Fetched> {
    let mut tries = 0usize;
    loop {
        match download_once(ctx, url).await {
            Ok(f) => return Ok(f),
            Err(GetError::Retry(e)) if tries < FILE_BACKOFF.len() => {
                tracing::warn!(error = %e, url, "file download failed; backing off");
                tokio::time::sleep(Duration::from_secs(FILE_BACKOFF[tries])).await;
                tries += 1;
            }
            Err(GetError::Retry(e) | GetError::Fatal(e)) => return Err(e),
        }
    }
}

async fn download_once(ctx: &Ctx<'_>, url: &str) -> Result<Fetched, GetError> {
    let resp = ctx
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| GetError::Retry(anyhow!(e).context("file request")))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Ok(Fetched::Missing);
    }
    if status.as_u16() == 429 || status.is_server_error() {
        return Err(GetError::Retry(anyhow!(
            "file proxy answered HTTP {status}"
        )));
    }
    if !status.is_success() {
        return Err(GetError::Fatal(anyhow!(
            "file proxy answered HTTP {status}"
        )));
    }
    let tmp = ctx.store.tmp_file("doc").map_err(GetError::Fatal)?;
    let res = stream_to(resp, &tmp).await;
    let (sha, len, magic) = match res {
        Ok(x) => x,
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(GetError::Retry(e));
        }
    };
    let (blob, _) = ctx
        .store
        .adopt_tmp(Ns::Raw, &tmp, sha, len)
        .await
        .map_err(GetError::Fatal)?;
    Ok(Fetched::Blob {
        blob,
        pdf: magic.starts_with(b"%PDF-"),
    })
}

async fn stream_to(
    resp: reqwest::Response,
    tmp: &std::path::Path,
) -> Result<([u8; 32], u64, Vec<u8>)> {
    let mut file = tokio::fs::File::create(tmp)
        .await
        .with_context(|| format!("creating {}", tmp.display()))?;
    let mut stream = resp.bytes_stream();
    let mut h = Sha256::new();
    let mut len = 0u64;
    let mut magic: Vec<u8> = Vec::with_capacity(5);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading the file body")?;
        if magic.len() < 5 {
            magic.extend(chunk.iter().take(5 - magic.len()));
        }
        h.update(&chunk);
        len += chunk.len() as u64;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok((h.finalize().into(), len, magic))
}

// ── text ───────────────────────────────────────────────────────────────────

pub enum TextError {
    /// The page count could not be read: the PDF is damaged.
    Unreadable(anyhow::Error),
    /// The text layer could not be read: try again later.
    Failed(anyhow::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    pub pages: u32,
    /// Pages separated by `\f`.
    pub text: String,
    /// `text` | `ocr` | `empty`
    pub status: &'static str,
    pub ocr_pages: u32,
    pub note: Option<String>,
}

#[must_use]
pub fn ocr_lang(lang: &str) -> &'static str {
    match lang {
        "si" => "sin",
        "ta" => "tam",
        _ => "eng",
    }
}

#[must_use]
pub fn has_text(page: &str) -> bool {
    page.chars().filter(|c| !c.is_whitespace()).count() >= MIN_PAGE_CHARS
}

/// Words in reading order to text: a new line when the vertical centre moves
/// by more than half a line.
pub fn words_to_text<'a>(words: impl Iterator<Item = &'a Word>) -> String {
    let mut out = String::new();
    let mut last: Option<(f32, f32)> = None;
    for w in words {
        if let Some((cy, h)) = last {
            if (w.cy() - cy).abs() > 0.5 * h.max(w.height()).max(1.0) {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(&w.text);
        last = Some((w.cy(), w.height()));
    }
    out
}

/// The text of every page `1..=pages` (empty strings for pages without words).
#[must_use]
pub fn page_texts(words: &Words, pages: u32) -> Vec<String> {
    (1..=pages)
        .map(|p| words_to_text(words.on_page(p)))
        .collect()
}

/// Joins pages with `\f`.
#[must_use]
pub fn join_pages(pages: &[String]) -> String {
    pages.join("\u{c}")
}

pub async fn extract(
    sb: &Sandbox,
    pdf: &[u8],
    lang: &str,
    ocr_cap: usize,
) -> Result<Extracted, TextError> {
    let pages = sb.page_count(pdf).await.map_err(TextError::Unreadable)?;
    let words = sb.pdf_words(pdf, 0, 0).await.map_err(TextError::Failed)?;
    let mut texts = page_texts(&words, pages);
    let blank: Vec<u32> = (1..=pages)
        .filter(|p| !has_text(&texts[(*p - 1) as usize]))
        .collect();
    let mut ocr_pages = 0u32;
    let mut ocr_failed = 0u32;
    for &p in blank.iter().take(ocr_cap) {
        match sb.ocr(pdf, p, OCR_DPI, &[ocr_lang(lang)]).await {
            Ok(w) => {
                let t = words_to_text(w.words.iter());
                if has_text(&t) {
                    texts[(p - 1) as usize] = t;
                    ocr_pages += 1;
                }
            }
            Err(e) => {
                tracing::warn!(page = p, error = %format!("{e:#}"), "OCR failed");
                ocr_failed += 1;
            }
        }
    }
    let skipped = blank.len().saturating_sub(ocr_cap);
    let mut notes = Vec::new();
    if skipped > 0 {
        notes.push(format!(
            "OCR capped at {ocr_cap} pages: {skipped} of {} pages without text skipped",
            blank.len()
        ));
    }
    if ocr_failed > 0 {
        notes.push(format!("OCR failed on {ocr_failed} pages"));
    }
    let any = texts.iter().any(|t| has_text(t));
    let status = if !any {
        "empty"
    } else if ocr_pages > 0 {
        "ocr"
    } else {
        "text"
    };
    Ok(Extracted {
        pages,
        text: join_pages(&texts),
        status,
        ocr_pages,
        note: (!notes.is_empty()).then(|| notes.join("; ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanka_extractd::PageBox;

    fn w(page: u32, text: &str, x: f32, y: f32) -> Word {
        Word {
            page,
            text: text.into(),
            bbox: [x, y, x + 30.0, y + 10.0],
            conf: None,
        }
    }

    #[test]
    fn words_become_lines_and_pages() {
        let words = Words {
            pages: vec![PageBox {
                page: 1,
                width: 600.0,
                height: 800.0,
            }],
            words: vec![
                w(1, "Rates", 10.0, 10.0),
                w(1, "of", 50.0, 10.5),
                w(1, "Exchange", 70.0, 9.8),
                w(1, "2026", 10.0, 40.0),
                w(3, "last", 10.0, 10.0),
            ],
        };
        let t = page_texts(&words, 3);
        assert_eq!(t, vec!["Rates of Exchange\n2026", "", "last"]);
        assert_eq!(join_pages(&t), "Rates of Exchange\n2026\u{c}\u{c}last");
        assert!(has_text(&t[0]));
        assert!(!has_text(&t[1]));
        assert!(!has_text("  1 \n"));
    }

    #[test]
    fn ocr_languages_follow_the_file() {
        assert_eq!(ocr_lang("si"), "sin");
        assert_eq!(ocr_lang("ta"), "tam");
        assert_eq!(ocr_lang("en"), "eng");
    }

    #[test]
    fn blob_refs_round_trip() {
        let sha = hex::encode([7u8; 32]);
        let b = blob_ref(&sha, 12).expect("ref");
        assert_eq!(b.hex(), sha);
        assert_eq!(b.key, Ns::Raw.cas_key(&[7u8; 32]));
        assert!(blob_ref("zz", 1).is_err());
        assert!(blob_ref("abcd", 1).is_err());
    }
}
