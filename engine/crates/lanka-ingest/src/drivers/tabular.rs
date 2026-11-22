//! The tabular drivers (03 §2.2): `csv`, `xlsx_table`, `html_table`,
//! `sdmx`, `ckan`, `git_files` and the table form of `http_json`. They share
//! one implementation: a *discover* mode finds the files, a *format* reads
//! each file into an L1 extract (cells for sheets and HTML tables, records
//! for CSV and JSON), and the selector language of [`super::table`] maps
//! the extract to observations or features.
//!
//! ```yaml
//! source: cbsl-exchange-rates
//! driver: xlsx_table
//! version: 1.0.0
//! discover:
//!   kind: static                    # static | keys | listing_page | url_pattern | ckan | git
//!   files:
//!     - { key: monthly_avg, url: "https://www.cbsl.gov.lk/…/IF_Monthly_Average_Exchange_Rates.xlsx" }
//! extract:
//!   sheets: "Avg ExRate"            # xlsx: regex on sheet names (default: all)
//! standardise: { dataset: …, tables: […], outputs: […] }   # table.rs
//! expect: [ … ]                     # expect.rs
//! ```
//!
//! Discover modes:
//!
//! - `static`: `files: [{key, url, file?}]`.
//! - `keys`: `url` with `{key}` (and `{alpha2}`, `{alpha3}`) over `keys`.
//! - `listing_page`: `page` is fetched and its links matching `links[].pattern`
//!   (a regex on the absolute URL) become candidates; `pick: latest` keeps the
//!   one with the greatest first capture group (a dated file name).
//! - `url_pattern`: `url` with `{yyyymmdd}`/`{yyyy}`/`{mm}`/`{dd}`; `probe`
//!   walks back `days` from today in `step_days` and takes the first URL that
//!   answers a HEAD with 2xx.
//! - `ckan`: `package` (a `package_show` URL); `resources: [{key, name, format}]`
//!   pick the newest resource whose name matches the regex. The fingerprint
//!   is the resources' ids and `last_modified` stamps.
//! - `git`: `repo` (`owner/name` on GitHub), `ref` (branch or tag) and
//!   `files: [{key, path}]`; the ref is resolved to a commit (the
//!   fingerprint) and files are fetched at that commit.
//!
//! With `--from DIR` every candidate reads `DIR/<file>` (default: the last
//! URL segment, else the key).
//!
//! Formats (`extract.format`, default from the driver): `csv` / `tsv`
//! (`delimiter`, `columns` for header-less files, `skip_lines`), `xlsx`
//! (xls too; `sheets`), `html` (`tables`: a CSS selector, default `table`),
//! `json` (`tables: [{id, path, entries, fields}]`). `zip_member` (a regex)
//! reads one member of a zip archive first.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use calamine::{Data, Reader};
use regex::Regex;
use serde::Deserialize;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use super::json_path::{elements, select, text};
use super::table::{self, StdSpec, clean};
use crate::connector::{
    Candidate, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched, OutputSink, StdCtx,
};
use crate::connectors::dcs_census::col_letters;
use crate::extract::{Cell, CellType, ExtractOut, ExtractRef, Record};
use crate::registry::{ConnectorSpec, Loaded};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub discover: DiscoverSpec,
    #[serde(default)]
    pub extract: ExtractSpec,
    pub standardise: StdSpec,
    #[serde(default)]
    pub expect: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub key: String,
    #[serde(default)]
    pub url: Option<String>,
    /// git: path in the repository.
    #[serde(default)]
    pub path: Option<String>,
    /// File name under `--from` (default: the last URL segment).
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    /// This file's format when it differs from `extract.format`.
    #[serde(default)]
    pub format: Option<Format>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    pub key: String,
    /// Regex on the absolute link URL.
    pub pattern: String,
    #[serde(default)]
    pub format: Option<Format>,
    /// `latest` (default): the match with the greatest first capture group.
    #[serde(default)]
    pub pick: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSpec {
    #[serde(default = "probe_days")]
    pub days: u32,
    #[serde(default = "one_day")]
    pub step_days: u32,
}

const fn probe_days() -> u32 {
    14
}

const fn one_day() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSpec {
    pub key: String,
    /// Regex on the resource name (or URL when the name is empty).
    pub name: String,
    /// CKAN resource format to match (`CSV`, `XLSX`).
    #[serde(default)]
    pub format: Option<String>,
    /// How to read it, when it differs from `extract.format`.
    #[serde(default)]
    pub read_as: Option<Format>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverKind {
    #[default]
    Static,
    Keys,
    ListingPage,
    UrlPattern,
    Ckan,
    Git,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    #[serde(default)]
    pub kind: DiscoverKind,
    #[serde(default)]
    pub files: Vec<FileSpec>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub links: Vec<LinkSpec>,
    #[serde(default)]
    pub probe: Option<ProbeSpec>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub resources: Vec<ResourceSpec>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Csv,
    Tsv,
    Xlsx,
    Html,
    Json,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonTable {
    pub id: String,
    /// Path to an array of records, or to an object whose entries are the
    /// records (`entries: true`: fields `_key`, `_value`).
    pub path: String,
    #[serde(default)]
    pub entries: bool,
    /// field name → path relative to the record (`$` = the record).
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Regex on the artifact key (default: every artifact).
    #[serde(default)]
    pub artifact: Option<String>,
    /// A path that must be present, else the response is an upstream error.
    #[serde(default)]
    pub require: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractSpec {
    #[serde(default)]
    pub format: Option<Format>,
    #[serde(default)]
    pub zip_member: Option<String>,
    #[serde(default)]
    pub delimiter: Option<String>,
    /// Header-less CSV: the column names.
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub skip_lines: usize,
    /// xlsx: regex on sheet names.
    #[serde(default)]
    pub sheets: Option<String>,
    /// html: CSS selector of the tables (default `table`).
    #[serde(default)]
    pub tables: Option<serde_json::Value>,
    /// csv: keep only rows whose column is one of the values (a country
    /// slice of a world file; applied while streaming, before the L1).
    #[serde(default)]
    pub filter: Vec<RowFilter>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowFilter {
    pub column: String,
    #[serde(rename = "in")]
    pub any_of: Vec<String>,
}

/// The tabular driver for one source.
#[derive(Debug, Clone)]
pub struct Tabular {
    spec: Spec,
    sha256: [u8; 32],
    format: Format,
    /// Per-key format overrides (`files[].format`, `links[].format`, `resources[].read_as`).
    formats: std::collections::HashMap<String, Format>,
    json_tables: Vec<JsonTable>,
}

impl Tabular {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: Spec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid {} spec", l.path.display(), l.spec.driver))?;
        Self::new(spec, l.sha256)
    }

    pub fn new(spec: Spec, sha256: [u8; 32]) -> Result<Self> {
        let format = match (spec.extract.format, spec.driver.as_str()) {
            (Some(f), _) => f,
            (None, "csv" | "sdmx" | "ckan" | "git_files") => Format::Csv,
            (None, "xlsx_table") => Format::Xlsx,
            (None, "html_table") => Format::Html,
            (None, "http_json") => Format::Json,
            (None, d) => bail!("driver {d}: give extract.format"),
        };
        let json_tables = match (format, &spec.extract.tables) {
            (Format::Json, Some(v)) => serde_json::from_value(v.clone())
                .context("extract.tables: [{id, path, entries?, fields}]")?,
            (Format::Json, None) => bail!("format json needs extract.tables"),
            _ => Vec::new(),
        };
        if let Some(z) = &spec.extract.zip_member {
            Regex::new(z).context("extract.zip_member")?;
        }
        ensure!(
            !spec.standardise.outputs.is_empty(),
            "standardise.outputs is empty"
        );
        let d = &spec.discover;
        let formats = d
            .files
            .iter()
            .filter_map(|f| f.format.map(|x| (f.key.clone(), x)))
            .chain(
                d.links
                    .iter()
                    .filter_map(|l| l.format.map(|x| (l.key.clone(), x))),
            )
            .chain(
                d.resources
                    .iter()
                    .filter_map(|r| r.read_as.map(|x| (r.key.clone(), x))),
            )
            .collect();
        Ok(Self {
            spec,
            sha256,
            format,
            formats,
            json_tables,
        })
    }

    #[must_use]
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }
}

fn render_vars(tpl: &str, vars: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let j = rest[i..]
            .find('}')
            .with_context(|| format!("unclosed placeholder in {tpl:?}"))?;
        let name = &rest[i + 1..i + j];
        out.push_str(
            &vars(name).with_context(|| format!("unknown placeholder {{{name}}} in {tpl:?}"))?,
        );
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Resolves `href` against `base`.
fn absolute(base: &reqwest::Url, href: &str) -> Option<String> {
    base.join(href.trim()).ok().map(|u| u.to_string())
}

async fn get_text(ctx: &DiscoverCtx<'_>, url: &str) -> Result<String> {
    ctx.http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("GET {url}"))?
        .text()
        .await
        .with_context(|| format!("reading {url}"))
}

impl Tabular {
    fn local(&self, ctx: &DiscoverCtx<'_>, c: &mut Candidate, file: Option<&str>) -> Result<()> {
        if let Some(dir) = &ctx.opts.from_dir {
            let name = file
                .map(str::to_owned)
                .or_else(|| {
                    c.url
                        .split('?')
                        .next()
                        .and_then(|u| u.rsplit('/').next())
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| c.key.clone());
            let p = dir.join(&name);
            let p = if p.is_file() { p } else { dir.join(&c.key) };
            ensure!(p.is_file(), "{} not found", p.display());
            c.local_path = Some(p);
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn candidates(&self, ctx: &DiscoverCtx<'_>) -> Result<(Vec<Candidate>, Vec<String>)> {
        let d = &self.spec.discover;
        let iso = &ctx.profile.spec.iso3166;
        let base = |key: &str| {
            let (a2, a3, k) = (iso.alpha2.clone(), iso.alpha3.clone(), key.to_owned());
            move |n: &str| match n {
                "alpha2" => Some(a2.clone()),
                "alpha3" => Some(a3.clone()),
                "key" => Some(k.clone()),
                _ => None,
            }
        };
        let offline = ctx.opts.from_dir.is_some();
        let mut out = Vec::new();
        let mut stamps = Vec::new();
        let mk = |key: &str, url: String, mt: Option<&String>| Candidate {
            key: key.to_owned(),
            url,
            media_type: mt.or(d.media_type.as_ref()).cloned(),
            ..Candidate::default()
        };
        match d.kind {
            DiscoverKind::Static => {
                ensure!(!d.files.is_empty(), "discover.files is empty");
                for f in &d.files {
                    let url = render_vars(
                        f.url.as_deref().context("discover.files[].url")?,
                        &base(&f.key),
                    )?;
                    let mut c = mk(&f.key, url, f.media_type.as_ref());
                    self.local(ctx, &mut c, f.file.as_deref())?;
                    out.push(c);
                }
            }
            DiscoverKind::Keys => {
                let tpl = d.url.as_deref().context("discover.url")?;
                ensure!(!d.keys.is_empty(), "discover.keys is empty");
                for k in &d.keys {
                    let mut c = mk(k, render_vars(tpl, &base(k))?, None);
                    self.local(ctx, &mut c, None)?;
                    out.push(c);
                }
                // plus fixed files (metadata endpoints)
                for f in &d.files {
                    let url = render_vars(
                        f.url.as_deref().context("discover.files[].url")?,
                        &base(&f.key),
                    )?;
                    let mut c = mk(&f.key, url, f.media_type.as_ref());
                    self.local(ctx, &mut c, f.file.as_deref())?;
                    out.push(c);
                }
            }
            DiscoverKind::ListingPage => {
                let page = render_vars(d.page.as_deref().context("discover.page")?, &base(""))?;
                let html = if offline {
                    String::new()
                } else {
                    get_text(ctx, &page).await?
                };
                let base_url = reqwest::Url::parse(&page).context("discover.page")?;
                let doc = scraper::Html::parse_document(&html);
                let sel = scraper::Selector::parse("a[href]")
                    .map_err(|e| anyhow::anyhow!("selector: {e}"))?;
                let hrefs: Vec<String> = doc
                    .select(&sel)
                    .filter_map(|a| a.value().attr("href"))
                    .filter_map(|h| absolute(&base_url, h))
                    .collect();
                for l in &d.links {
                    let rx = Regex::new(&l.pattern).context("links[].pattern")?;
                    let mut found: Vec<(String, String)> = hrefs
                        .iter()
                        .filter_map(|h| {
                            rx.captures(h).map(|c| {
                                (
                                    c.get(1).map_or_else(String::new, |m| m.as_str().to_owned()),
                                    h.clone(),
                                )
                            })
                        })
                        .collect();
                    found.sort();
                    found.dedup();
                    if offline {
                        let mut c = mk(&l.key, page.clone(), None);
                        c.local_path = None;
                        self.local(ctx, &mut c, Some(&l.key))?;
                        out.push(c);
                        continue;
                    }
                    let picked: Vec<String> = match l.pick.as_deref().unwrap_or("latest") {
                        "latest" => found.last().map(|(_, h)| h.clone()).into_iter().collect(),
                        "all" => found.into_iter().map(|(_, h)| h).collect(),
                        p => bail!("links[].pick: unknown {p:?}"),
                    };
                    ensure!(
                        !picked.is_empty(),
                        "listing page {page}: no link matches {:?}",
                        l.pattern
                    );
                    let n = picked.len();
                    for (i, url) in picked.into_iter().enumerate() {
                        let key = if n == 1 {
                            l.key.clone()
                        } else {
                            format!("{}_{i}", l.key)
                        };
                        stamps.push(url.clone());
                        out.push(mk(&key, url, None));
                    }
                }
            }
            DiscoverKind::UrlPattern => {
                let tpl = d.url.as_deref().context("discover.url")?;
                let probe = d.probe.clone().unwrap_or(ProbeSpec {
                    days: 14,
                    step_days: 1,
                });
                let key = d
                    .files
                    .first()
                    .map_or("file", |f| f.key.as_str())
                    .to_owned();
                let today = chrono::Utc::now().date_naive();
                let mut hit = None;
                for i in 0..probe.days {
                    let day = today - chrono::Duration::days(i64::from(i * probe.step_days.max(1)));
                    let url = render_vars(tpl, &|n: &str| match n {
                        "yyyymmdd" => Some(day.format("%Y%m%d").to_string()),
                        "yyyy" => Some(day.format("%Y").to_string()),
                        "mm" => Some(day.format("%m").to_string()),
                        "dd" => Some(day.format("%d").to_string()),
                        other => base(&key)(other),
                    })?;
                    if offline {
                        hit = Some(url);
                        break;
                    }
                    let ok = ctx
                        .http
                        .head(&url)
                        .send()
                        .await
                        .is_ok_and(|r| r.status().is_success());
                    if ok {
                        hit = Some(url);
                        break;
                    }
                }
                let url = hit.with_context(|| {
                    format!(
                        "url_pattern: no URL answered in the last {} probes",
                        probe.days
                    )
                })?;
                stamps.push(url.clone());
                let mut c = mk(&key, url, None);
                self.local(ctx, &mut c, Some(&key))?;
                out.push(c);
            }
            DiscoverKind::Ckan => {
                let pkg =
                    render_vars(d.package.as_deref().context("discover.package")?, &base(""))?;
                if offline {
                    for r in &d.resources {
                        let mut c = mk(&r.key, pkg.clone(), None);
                        self.local(ctx, &mut c, Some(&r.key))?;
                        out.push(c);
                    }
                } else {
                    let body = get_text(ctx, &pkg).await?;
                    let v: serde_json::Value =
                        serde_json::from_str(&body).context("package_show: not JSON")?;
                    ensure!(
                        v["success"].as_bool() == Some(true),
                        "package_show failed: {}",
                        super::json_path::truncate(&body, 300)
                    );
                    let res = v["result"]["resources"]
                        .as_array()
                        .context("package_show: no resources")?;
                    for r in &d.resources {
                        let rx = Regex::new(&r.name).context("resources[].name")?;
                        let mut hits: Vec<&serde_json::Value> = res
                            .iter()
                            .filter(|x| {
                                let name = x["name"].as_str().unwrap_or_default();
                                let url = x["url"].as_str().unwrap_or_default();
                                (rx.is_match(name) || rx.is_match(url))
                                    && r.format.as_ref().is_none_or(|f| {
                                        x["format"]
                                            .as_str()
                                            .is_some_and(|xf| xf.eq_ignore_ascii_case(f))
                                    })
                            })
                            .collect();
                        hits.sort_by_key(|x| {
                            x["last_modified"]
                                .as_str()
                                .or_else(|| x["created"].as_str())
                                .unwrap_or_default()
                                .to_owned()
                        });
                        let x = hits
                            .last()
                            .with_context(|| format!("{pkg}: no resource matches {:?}", r.name))?;
                        let url = x["url"].as_str().context("resource url")?.to_owned();
                        stamps.push(format!(
                            "{} {} {}",
                            x["id"].as_str().unwrap_or_default(),
                            x["last_modified"].as_str().unwrap_or_default(),
                            url
                        ));
                        out.push(mk(&r.key, url, None));
                    }
                }
            }
            DiscoverKind::Git => {
                let repo = d.repo.as_deref().context("discover.repo")?;
                let git_ref = d.git_ref.as_deref().unwrap_or("main");
                let sha = if offline {
                    git_ref.to_owned()
                } else {
                    let api = format!("https://api.github.com/repos/{repo}/commits/{git_ref}");
                    let body = ctx
                        .http
                        .get(&api)
                        .header("Accept", "application/vnd.github.sha")
                        .send()
                        .await
                        .and_then(reqwest::Response::error_for_status)
                        .with_context(|| format!("GET {api}"))?
                        .text()
                        .await?;
                    let sha = body.trim().to_owned();
                    ensure!(
                        sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                        "{api}: not a commit sha: {}",
                        super::json_path::truncate(&sha, 80)
                    );
                    sha
                };
                stamps.push(format!("{repo}@{sha}"));
                for f in &d.files {
                    let path = f.path.as_deref().context("discover.files[].path")?;
                    let url = format!("https://raw.githubusercontent.com/{repo}/{sha}/{path}");
                    let mut c = mk(&f.key, url, f.media_type.as_ref());
                    c.edition = Some(sha.clone());
                    self.local(ctx, &mut c, f.file.as_deref())?;
                    out.push(c);
                }
            }
        }
        ensure!(!out.is_empty(), "discover found nothing");
        Ok((out, stamps))
    }
}

// ───────────────────────────── formats ─────────────────────────────────────

/// Reads the zip member matching `pattern`.
fn unzip(bytes: &[u8], pattern: &str) -> Result<(String, Vec<u8>)> {
    let rx = Regex::new(pattern)?;
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip archive")?;
    let names: Vec<String> = z
        .file_names()
        .map(|n| n.map(|n| n.into_owned()))
        .collect::<Result<_, _>>()?;
    let name = names
        .iter()
        .find(|n| rx.is_match(n))
        .with_context(|| {
            format!(
                "zip has no member matching {pattern:?} ({})",
                names.join(", ")
            )
        })?
        .clone();
    let mut f = z.by_name(&name)?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)?;
    Ok((name, out))
}

/// RFC 4180 CSV (quotes, doubled quotes, newlines in quotes, CRLF).
pub fn parse_csv(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            c if c == delim => row.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

/// Streams CSV records (RFC 4180) from a reader: a record ends at a
/// newline outside quotes. Memory is one record, not the file.
struct CsvRows<R: std::io::BufRead> {
    r: R,
    delim: char,
    line: Vec<u8>,
}

impl<R: std::io::BufRead> CsvRows<R> {
    fn next_row(&mut self) -> Result<Option<Vec<String>>> {
        self.line.clear();
        loop {
            let n = self.r.read_until(b'\n', &mut self.line)?;
            if n == 0 {
                break;
            }
            if self.line.iter().filter(|b| **b == b'"').count() % 2 == 0 {
                break;
            }
        }
        if self.line.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&self.line);
        let mut rows = parse_csv(&text, self.delim);
        Ok(Some(rows.pop().unwrap_or_default()))
    }
}

fn csv_records(
    spec: &ExtractSpec,
    reader: impl Read,
    table_id: &str,
    delim: char,
    ai: u32,
    out: &mut Vec<Record>,
) -> Result<()> {
    let mut rows = CsvRows {
        r: std::io::BufReader::with_capacity(1 << 20, reader),
        delim,
        line: Vec::new(),
    };
    for _ in 0..spec.skip_lines {
        rows.next_row()?;
    }
    let header: Vec<String> = if spec.columns.is_empty() {
        rows.next_row()?
            .context("empty CSV")?
            .into_iter()
            .map(|h| clean(h.trim_start_matches('\u{feff}')))
            .collect()
    } else {
        spec.columns.clone()
    };
    let filters: Vec<(usize, &RowFilter)> = spec
        .filter
        .iter()
        .map(|f| {
            header
                .iter()
                .position(|h| h == &f.column)
                .map(|i| (i, f))
                .with_context(|| format!("extract.filter: no column {:?}", f.column))
        })
        .collect::<Result<_>>()?;
    let first_line = 1 + spec.skip_lines + usize::from(spec.columns.is_empty());
    let mut i = 0usize;
    let mut n = 0u32;
    while let Some(r) = rows.next_row()? {
        let line = first_line + i;
        i += 1;
        if r.len() == 1 && r[0].is_empty() {
            continue;
        }
        if !filters
            .iter()
            .all(|(c, f)| r.get(*c).is_some_and(|v| f.any_of.contains(v)))
        {
            continue;
        }
        let mut fields = BTreeMap::new();
        for (h, v) in header.iter().zip(r) {
            if !v.is_empty() && !h.is_empty() {
                fields.insert(h.clone(), v);
            }
        }
        out.push(Record {
            table_id: table_id.to_owned(),
            record_idx: n,
            path: format!("line:{line}"),
            fields,
            artifact_idx: ai,
        });
        n += 1;
    }
    Ok(())
}

/// Calls `f` with a streaming reader over the zip member matching `pattern`.
fn with_member<T>(
    bytes: &[u8],
    pattern: &str,
    f: impl FnOnce(&str, &mut dyn Read) -> Result<T>,
) -> Result<T> {
    let rx = Regex::new(pattern)?;
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip archive")?;
    let names: Vec<String> = z
        .file_names()
        .map(|n| n.map(|n| n.into_owned()))
        .collect::<Result<_, _>>()?;
    let name = names
        .iter()
        .find(|n| rx.is_match(n))
        .with_context(|| {
            format!(
                "zip has no member matching {pattern:?} ({})",
                names.join(", ")
            )
        })?
        .clone();
    let mut m = z.by_name(&name)?;
    f(&name, &mut m)
}

fn data_text(d: &Data) -> Option<(String, CellType)> {
    match d {
        Data::Int(i) => Some((i.to_string(), CellType::Number)),
        Data::Float(f) => Some((
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.0}")
            } else {
                // shortest round-trip representation of the stored double
                format!("{f}")
            },
            CellType::Number,
        )),
        Data::String(s) => Some((s.clone(), CellType::String)),
        Data::DateTime(dt) => {
            let (y, m, d, ..) = dt.to_ymd_hms_milli();
            Some((format!("{y:04}-{m:02}-{d:02}"), CellType::String))
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => Some((s.clone(), CellType::String)),
        Data::Bool(b) => Some((b.to_string(), CellType::String)),
        Data::Error(_) | Data::Empty => None,
    }
}

fn xlsx_cells(
    bytes: &[u8],
    key: &str,
    sheets: Option<&Regex>,
    ai: u32,
    out: &mut Vec<Cell>,
) -> Result<()> {
    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .context("opening workbook (xlsx/xls)")?;
    let names = wb.sheet_names();
    let mut n = 0;
    for name in names {
        if sheets.is_some_and(|r| !r.is_match(&name)) {
            continue;
        }
        n += 1;
        let range = wb
            .worksheet_range(&name)
            .with_context(|| format!("sheet {name:?}"))?;
        let Some((r0, c0)) = range.start() else {
            continue;
        };
        for (i, row) in range.rows().enumerate() {
            let r = r0 + u32::try_from(i)?;
            for (j, d) in row.iter().enumerate() {
                let c = c0 + u32::try_from(j)?;
                let Some((text, vtype)) = data_text(d) else {
                    continue;
                };
                if text.trim().is_empty() {
                    continue;
                }
                out.push(Cell {
                    table_id: format!("{key}!{name}"),
                    r,
                    c,
                    text,
                    vtype,
                    page: None,
                    bbox: None,
                    sheet: Some(name.clone()),
                    a1: Some(format!("{}{}", col_letters(c), r + 1)),
                    row_header: None,
                    col_header: None,
                    artifact_idx: ai,
                    conf: None,
                    reader: "calamine".to_owned(),
                });
            }
        }
    }
    ensure!(
        n > 0,
        "{key}: no sheet matches {:?}",
        sheets.map(Regex::as_str)
    );
    Ok(())
}

/// Every `<table>` matching `selector`: one cell per non-empty `td`/`th`,
/// column positions honouring `colspan`/`rowspan`. HTML comments and
/// markup are dropped; text is whitespace-collapsed.
pub fn html_cells(
    html: &str,
    key: &str,
    selector: &str,
    ai: u32,
    out: &mut Vec<Cell>,
) -> Result<()> {
    let doc = scraper::Html::parse_document(html);
    let tsel = scraper::Selector::parse(selector)
        .map_err(|e| anyhow::anyhow!("selector {selector:?}: {e}"))?;
    let rsel = scraper::Selector::parse("tr").map_err(|e| anyhow::anyhow!("{e}"))?;
    let csel = scraper::Selector::parse("td, th").map_err(|e| anyhow::anyhow!("{e}"))?;
    for (ti, table) in doc.select(&tsel).enumerate() {
        // rows of this table only (not of nested tables)
        let mut taken: BTreeMap<(u32, u32), ()> = BTreeMap::new();
        let mut r = 0u32;
        for tr in table.select(&rsel) {
            let owner = tr
                .ancestors()
                .filter_map(scraper::ElementRef::wrap)
                .find(|e| e.value().name() == "table");
            if owner.map(|e| e.id()) != Some(table.id()) {
                continue;
            }
            let mut c = 0u32;
            for td in tr.select(&csel) {
                if td
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .find(|e| e.value().name() == "tr")
                    .map(|e| e.id())
                    != Some(tr.id())
                {
                    continue;
                }
                while taken.contains_key(&(r, c)) {
                    c += 1;
                }
                let span = |a: &str| {
                    td.value()
                        .attr(a)
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .unwrap_or(1)
                        .clamp(1, 1000)
                };
                let (cs, rs) = (span("colspan"), span("rowspan"));
                for dr in 0..rs {
                    for dc in 0..cs {
                        taken.insert((r + dr, c + dc), ());
                    }
                }
                let text = clean(&td.text().collect::<String>());
                if !text.is_empty() {
                    let num = text.replace(',', "").parse::<f64>().is_ok();
                    out.push(Cell {
                        table_id: format!("{key}#{ti}"),
                        r,
                        c,
                        text,
                        vtype: if num {
                            CellType::Number
                        } else {
                            CellType::String
                        },
                        page: None,
                        bbox: None,
                        sheet: None,
                        a1: None,
                        row_header: None,
                        col_header: None,
                        artifact_idx: ai,
                        conf: None,
                        reader: "scraper".to_owned(),
                    });
                }
                c += cs;
            }
            r += 1;
        }
    }
    Ok(())
}

fn json_records(
    tables: &[JsonTable],
    bytes: &[u8],
    key: &str,
    alpha: (&str, &str),
    ai: u32,
    out: &mut Vec<Record>,
) -> Result<()> {
    let root: &RawValue =
        serde_json::from_slice(bytes).with_context(|| format!("{key}: response is not JSON"))?;
    let vars = |n: &str| match n {
        "key" => Some(key.to_owned()),
        "alpha2" => Some(alpha.0.to_owned()),
        "alpha3" => Some(alpha.1.to_owned()),
        _ => None,
    };
    for t in tables {
        if let Some(a) = &t.artifact
            && !Regex::new(a)?.is_match(key)
        {
            continue;
        }
        if let Some(req) = &t.require {
            ensure!(
                select(root, req)?.is_some(),
                "{key}: upstream error (no {req:?} in the response): {}",
                super::json_path::truncate(root.get(), 300)
            );
        }
        let path = render_vars(&t.path, &vars)?;
        let at = select(root, &path)?;
        let items: Vec<(Option<String>, &RawValue, String)> = if t.entries {
            match at {
                None => Vec::new(),
                Some(v) if v.get().trim() == "null" => Vec::new(),
                Some(v) => {
                    let m: BTreeMap<String, &RawValue> = serde_json::from_str(v.get())
                        .with_context(|| format!("{key}: {path} is not an object"))?;
                    m.into_iter()
                        .map(|(k, v)| {
                            let p = format!("{path}.{k}");
                            (Some(k), v, p)
                        })
                        .collect()
                }
            }
        } else {
            elements(at)
                .with_context(|| format!("{key}: {path}"))?
                .into_iter()
                .enumerate()
                .map(|(i, v)| (None, v, format!("{path}[{i}]")))
                .collect()
        };
        for (i, (k, v, path)) in items.into_iter().enumerate() {
            let mut fields = BTreeMap::new();
            if let Some(k) = k {
                fields.insert("_key".to_owned(), k);
                if let Some(s) = text(Some(v)).ok().flatten() {
                    fields.insert("_value".to_owned(), s);
                }
            }
            for (name, p) in &t.fields {
                let sel = if p == "$" { Some(v) } else { select(v, p)? };
                if let Some(s) = text(sel)? {
                    fields.insert(name.clone(), s);
                }
            }
            out.push(Record {
                table_id: t.id.clone(),
                record_idx: u32::try_from(i)?,
                path,
                fields,
                artifact_idx: ai,
            });
        }
    }
    Ok(())
}

#[async_trait]
impl Connector for Tabular {
    fn source_id(&self) -> &str {
        &self.spec.source
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    fn extractor(&self) -> &str {
        &self.spec.driver
    }

    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let (candidates, stamps) = self.candidates(ctx).await?;
        let fingerprint = (!stamps.is_empty()).then(|| {
            let mut h = Sha256::new();
            for c in &candidates {
                h.update(c.key.as_bytes());
                h.update(b"=");
                h.update(c.url.as_bytes());
                h.update(b"\n");
            }
            for s in &stamps {
                h.update(s.as_bytes());
                h.update(b"\n");
            }
            h.finalize().into()
        });
        // Static and keyed URLs carry no upstream stamp: the artifact hash
        // (after a conditional GET) is the change detector.
        let fingerprint = match self.spec.discover.kind {
            DiscoverKind::Ckan | DiscoverKind::Git => fingerprint,
            _ => None,
        };
        Ok(Discovery {
            candidates,
            fingerprint,
        })
    }

    async fn extract(&self, ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        let iso = &ctx.profile.spec.iso3166;
        let x = &self.spec.extract;
        let sheets = x.sheets.as_deref().map(Regex::new).transpose()?;
        let mut cells = Vec::new();
        let mut records = Vec::new();
        for (ai, f) in fetched.iter().enumerate() {
            let ai = u32::try_from(ai)?;
            let raw = f.read()?;
            let format = self
                .formats
                .get(f.key.as_str())
                .or_else(|| {
                    // listing_page with pick: all suffixes keys with _<n>
                    f.key
                        .rsplit_once('_')
                        .and_then(|(k, n)| n.parse::<u32>().ok().and(self.formats.get(k)))
                })
                .copied()
                .unwrap_or(self.format);
            if matches!(format, Format::Csv | Format::Tsv) {
                let delim = match (&x.delimiter, format) {
                    (Some(d), _) if d == "\\t" || d == "tab" => '\t',
                    (Some(d), _) => d.chars().next().context("empty delimiter")?,
                    (None, Format::Tsv) => '\t',
                    _ => ',',
                };
                match &x.zip_member {
                    Some(z) => with_member(&raw, z, |name, r| {
                        csv_records(x, r, &format!("{}/{name}", f.key), delim, ai, &mut records)
                    }),
                    None => csv_records(x, Cursor::new(&raw), &f.key, delim, ai, &mut records),
                }
                .with_context(|| f.key.clone())?;
                continue;
            }
            let (table_id, bytes) = match &x.zip_member {
                Some(z) => {
                    let (name, b) = unzip(&raw, z).with_context(|| f.key.clone())?;
                    (format!("{}/{name}", f.key), b)
                }
                None => (f.key.clone(), raw),
            };
            match format {
                Format::Csv | Format::Tsv => unreachable!("streamed above"),
                Format::Xlsx => xlsx_cells(&bytes, &table_id, sheets.as_ref(), ai, &mut cells)
                    .with_context(|| f.key.clone())?,
                Format::Html => {
                    let sel = x
                        .tables
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("table");
                    html_cells(
                        &String::from_utf8_lossy(&bytes),
                        &table_id,
                        sel,
                        ai,
                        &mut cells,
                    )
                    .with_context(|| f.key.clone())?;
                }
                Format::Json => json_records(
                    &self.json_tables,
                    &bytes,
                    &f.key,
                    (&iso.alpha2, &iso.alpha3),
                    ai,
                    &mut records,
                )?,
            }
        }
        let mut out = Vec::new();
        if !cells.is_empty() {
            out.push(ExtractOut::Cells(cells));
        }
        if !records.is_empty() {
            out.push(ExtractOut::Records(records));
        }
        ensure!(
            !out.is_empty(),
            "extract read nothing from {} file(s)",
            fetched.len()
        );
        Ok(out)
    }

    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        table::standardise(&self.spec.standardise, ctx, extracts, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parser_handles_quotes() {
        let rows = parse_csv(
            "a,b,c\r\n1,\"x, y\",\"he said \"\"hi\"\"\"\n2,,\"multi\nline\"\n",
            ',',
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], vec!["1", "x, y", "he said \"hi\""]);
        assert_eq!(rows[2], vec!["2", "", "multi\nline"]);
        assert_eq!(parse_csv("a\tb\n1\t2", '\t')[1], vec!["1", "2"]);
    }

    #[test]
    fn html_tables_skip_comments_and_honour_spans() {
        let html = r#"<table><tr><th>Items</th><!--<td>old</td>--><th colspan="2">2024</th></tr>
            <tr><td>1. Tea</td><!--<td>9</td>--><td>1,109.03</td><td>x</td></tr></table>
            <table><tr><td>other</td></tr></table>"#;
        let mut cells = Vec::new();
        html_cells(html, "k", "table", 0, &mut cells).expect("parse");
        let t0: Vec<(u32, u32, &str)> = cells
            .iter()
            .filter(|c| c.table_id == "k#0")
            .map(|c| (c.r, c.c, c.text.as_str()))
            .collect();
        assert_eq!(
            t0,
            vec![
                (0, 0, "Items"),
                (0, 1, "2024"),
                (1, 0, "1. Tea"),
                (1, 1, "1,109.03"),
                (1, 2, "x")
            ]
        );
        assert_eq!(cells.iter().filter(|c| c.table_id == "k#1").count(), 1);
        assert_eq!(cells[3].vtype, CellType::Number);
    }
}
