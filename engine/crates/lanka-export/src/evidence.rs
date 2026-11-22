//! Evidence sheets (03 §6): one static HTML page per review subject in
//! `store/evidence/<dsv | extract-<id>>/`, with the page images it shows.
//!
//! What a reviewer needs to approve a class B/C version without opening
//! the PDF: provenance (dataset, edition, artifact sha256 + URL, extractor,
//! template hash, run), the validation report including every template
//! check recomputed from the stored extract, the diff against the current
//! version (largest changes first), each source page rendered by the
//! sandbox with every cell box overlaid, a per-cell table ordered by risk
//! (failed-check neighbours, low confidence, large change, new) with a crop
//! of the printed token, the name → code matches, a checklist and — for
//! class C — the per-cell sign-off state and forms.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use lanka_extractd::Sandbox;
use lanka_ingest::Registry;
use lanka_ingest::drivers::pdf_table::{PdfTable, table_from_cells};
use lanka_ingest::extract::{Cell, CellType, ExtractArtifact};
use lanka_ingest::review::{self, ExtractRow, Target};
use lanka_ingest::template::{CheckResult, Template};
use lanka_store::{BlobRef, BlobStore, Ns};
use serde::Serialize;
use sqlx::PgPool;

/// Page render resolution (03 §3.1: `pdftoppm -r 150`).
pub const DPI: u32 = 150;

/// What was written.
#[derive(Debug, Clone, Serialize)]
pub struct Built {
    pub target: String,
    /// Store key of the sheet (`evidence/<subject>/index.html`).
    pub key: String,
    pub index: PathBuf,
    pub review_id: i64,
    pub cells: usize,
    pub pages: usize,
    pub checks_ok: bool,
    pub signoff_required: usize,
    pub signoff_missing: usize,
}

struct Version {
    id: i64,
    dataset_id: String,
    title: String,
    dataset_key: i32,
    seq: i32,
    status: String,
    class: String,
    vintage: Option<String>,
    run_id: Option<i64>,
    validation: Option<serde_json::Value>,
    diff: Option<serde_json::Value>,
}

/// One observation of the version, with its provenance and the value it replaces.
#[derive(Debug, Clone)]
struct Obs {
    indicator: String,
    geo: String,
    period: String,
    dims: String,
    value: Option<f64>,
    prev: Option<f64>,
    status: String,
    extract_id: Option<i64>,
    row: Option<i32>,
    locator: serde_json::Value,
}

struct Loaded {
    row: ExtractRow,
    cells: Vec<Cell>,
    artifacts: Vec<ExtractArtifact>,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        #[allow(clippy::cast_possible_truncation)]
        let i = v as i64;
        let s = i.abs().to_string();
        let mut out = String::new();
        for (k, ch) in s.chars().enumerate() {
            if k > 0 && (s.len() - k) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        if i < 0 { format!("-{out}") } else { out }
    } else {
        format!("{v}")
    }
}

/// PNG width and height from the IHDR chunk.
fn png_size(b: &[u8]) -> Result<(u32, u32)> {
    if b.len() < 24 || &b[..8] != b"\x89PNG\r\n\x1a\n" {
        bail!("not a PNG");
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    Ok((w, h))
}

async fn version(pool: &PgPool, dsv: &str) -> Result<Version> {
    let r: (i64, String, String, i32, i32, String, String, Option<String>, Option<i64>, Option<serde_json::Value>, Option<serde_json::Value>) =
        sqlx::query_as(
            "SELECT v.id, v.dataset_id, d.title, d.key, v.seq, v.status::text, v.publication_class::text,
                    v.vintage, v.run_id, v.validation, v.diff
             FROM data.dataset_versions v JOIN catalog.datasets d ON d.id = v.dataset_id
             WHERE v.dsv = $1 ORDER BY v.status = 'in_review' DESC, v.seq DESC LIMIT 1",
        )
        .bind(dsv)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("no dataset version {dsv}"))?;
    Ok(Version {
        id: r.0,
        dataset_id: r.1,
        title: r.2,
        dataset_key: r.3,
        seq: r.4,
        status: r.5,
        class: r.6,
        vintage: r.7,
        run_id: r.8,
        validation: r.9,
        diff: r.10,
    })
}

async fn observations(pool: &PgPool, v: &Version) -> Result<Vec<Obs>> {
    type Row = (
        String,
        String,
        chrono::NaiveDate,
        String,
        String,
        Option<f64>,
        Option<f64>,
        i16,
        Option<i64>,
        Option<i32>,
        serde_json::Value,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT i.id, g.code, o.time_start, o.freq::text, ds.dims, o.value,
                (SELECT q.value FROM data.observations q
                  WHERE q.dataset_key = o.dataset_key AND q.indicator_key = o.indicator_key
                    AND q.geo_key = o.geo_key AND q.time_start = o.time_start AND q.freq = o.freq
                    AND q.dims_key = o.dims_key AND q.vfrom < o.vfrom
                  ORDER BY q.vfrom DESC LIMIT 1) AS prev,
                o.status, p.extract_id, p.extract_row, p.locator
         FROM data.observations o
         JOIN data.obs_provenance p ON p.dataset_key = o.dataset_key AND p.obs_id = o.obs_id
         JOIN catalog.indicators i ON i.key = o.indicator_key
         JOIN geo.units g ON g.key = o.geo_key
         JOIN catalog.dim_sets ds ON ds.key = o.dims_key
         WHERE o.dataset_key = $1 AND o.vfrom = $2
         ORDER BY o.obs_id",
    )
    .bind(v.dataset_key)
    .bind(v.seq)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Obs {
            indicator: r.0,
            geo: r.1,
            period: match r.3.as_str() {
                "A" => r.2.format("%Y").to_string(),
                "Q" => {
                    use chrono::Datelike;
                    format!("{}-Q{}", r.2.year(), r.2.month0() / 3 + 1)
                }
                _ => r.2.to_string(),
            },
            dims: r.4,
            value: r.5,
            prev: r.6,
            status: lanka_core::Status::from_code(r.7)
                .map_or_else(|| r.7.to_string(), |s| s.as_str().to_owned()),
            extract_id: r.8,
            row: r.9,
            locator: r.10,
        })
        .collect())
}

/// Template checks of the tables in an extract, recomputed from its cells.
fn template_checks(registry: &Registry, e: &Loaded) -> Vec<(String, String, Vec<CheckResult>)> {
    let Some(spec) = registry.connectors.get(&e.row.source_id) else {
        return Vec::new();
    };
    let Ok(driver) = PdfTable::from_spec(spec) else {
        return Vec::new();
    };
    let mut ids: Vec<&str> = e.cells.iter().map(|c| c.table_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        let Some((t, tpl)) = driver.template_of(id) else {
            continue;
        };
        let Ok((_, ed)) = tpl.edition(driver.file_edition(&t.file)) else {
            continue;
        };
        let table = table_from_cells(&e.cells, id, ed);
        out.push((id.to_owned(), tpl.id.clone(), table.check(ed)));
    }
    out
}

fn template_for<'a>(registry: &'a Registry, source: &str, table: &str) -> Option<&'a Template> {
    let spec = registry.connectors.get(source)?;
    let t: &str = spec
        .json
        .get("extract")?
        .get("tables")?
        .as_array()?
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(table))?
        .get("template")?
        .as_str()?;
    registry.templates.get(t).map(|l| &l.spec)
}

/// Builds (or rebuilds) the evidence sheet of a review subject.
#[allow(clippy::too_many_lines)]
pub async fn build(
    pool: &PgPool,
    store: &dyn BlobStore,
    registry: &Registry,
    sandbox: &Sandbox,
    target: &Target,
) -> Result<Built> {
    let subject = target.subject();
    let dir = store
        .local_path(Ns::Evidence, &subject)
        .context("evidence sheets need a filesystem store")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    // ── subject, extracts, observations ──
    let (ver, extract_ids, obs) = match target {
        Target::Version(dsv) => {
            let v = version(pool, dsv).await?;
            let ids: Vec<i64> = sqlx::query_scalar(
                "SELECT extract_id FROM data.dataset_version_extracts WHERE version_id = $1 ORDER BY extract_id",
            )
            .bind(v.id)
            .fetch_all(pool)
            .await?;
            let obs = observations(pool, &v).await?;
            (Some(v), ids, obs)
        }
        Target::Extract(id) => (None, vec![*id], Vec::new()),
    };
    let review_id = review::open_review(pool, target, "evidence review").await?;
    let mut extracts = Vec::new();
    for id in &extract_ids {
        let (row, data, artifacts) = review::load_extract(pool, store, *id).await?;
        extracts.push(Loaded {
            row,
            cells: data.cells().to_vec(),
            artifacts,
        });
    }

    // ── per extract: checks, sign-off, page renders ──
    let mut checks_ok = true;
    let mut signoff_required = 0;
    let mut signoff_missing = 0;
    let decisions = review::decisions(pool, review_id).await?;
    let decided: HashMap<(i64, i32), &review::CellDecision> = decisions
        .iter()
        .map(|d| ((d.extract_id, d.row_idx), d))
        .collect();
    let mut html_tables = String::new();
    let mut provenance = String::new();
    let mut check_html = String::new();
    let mut pages_written = 0;
    let mut n_cells = 0;
    let obs_by_cell: HashMap<(i64, i32), Vec<&Obs>> =
        obs.iter().fold(HashMap::new(), |mut m, o| {
            if let (Some(e), Some(r)) = (o.extract_id, o.row) {
                m.entry((e, r)).or_default().push(o);
            }
            m
        });
    for e in &extracts {
        let sha = e.row.sha()?;
        let class_c = e.row.is_class_c();
        let required: BTreeSet<u32> = if class_c {
            review::required_cells(&sha, &e.cells).into_iter().collect()
        } else {
            BTreeSet::new()
        };
        signoff_required += required.len();
        signoff_missing += required
            .iter()
            .filter(|r| !decided.contains_key(&(e.row.id, i32::try_from(**r).unwrap_or(-1))))
            .count();
        let checks = template_checks(registry, e);
        let mut failing: BTreeSet<(String, String, String)> = BTreeSet::new();
        for (table, tpl, results) in &checks {
            for r in results {
                checks_ok &= r.ok;
                for (row, col) in &r.cells {
                    failing.insert((table.clone(), row.clone(), col.clone()));
                }
                let _ = write!(
                    check_html,
                    "<tr class=\"{}\"><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    if r.ok { "ok" } else { "bad" },
                    if r.ok { "pass" } else { "FAIL" },
                    esc(tpl),
                    esc(&r.check),
                    r.n,
                    esc(&r.failures.join("; "))
                );
            }
        }
        if checks.is_empty() {
            let _ = write!(
                check_html,
                "<tr><td>—</td><td colspan=4>extract {} ({}): no template checks (class C reading or non-template extractor)</td></tr>",
                e.row.id,
                esc(e.row.reader.as_deref().unwrap_or(&e.row.extractor))
            );
        }
        let _ = write!(
            provenance,
            "<tr><th>extract</th><td><code>x{}</code> · {} rows · extractor <code>{} {}</code> · reader <code>{}</code>{}{}{}<br><small>sha256 <code>{}</code>{}</small></td></tr>",
            e.row.id,
            e.row.rows,
            esc(&e.row.extractor),
            esc(&e.row.extractor_ver),
            esc(e.row.reader.as_deref().unwrap_or("?")),
            e.row
                .model
                .as_deref()
                .map(|m| format!(" · model <code>{}</code>", esc(m)))
                .unwrap_or_default(),
            e.row
                .parent_id
                .map(|p| format!(" · corrects <code>x{p}</code>"))
                .unwrap_or_default(),
            e.row
                .approved_by
                .as_deref()
                .map(|b| format!(" · <b>approved by {}</b>", esc(b)))
                .unwrap_or_default(),
            hex::encode(sha),
            e.row
                .template_sha256
                .as_deref()
                .map(|t| format!(" · template+spec sha256 <code>{}</code>", hex::encode(t)))
                .unwrap_or_default()
        );

        // Artifacts.
        let ids: Vec<i64> = e.artifacts.iter().map(|a| a.artifact_id).collect();
        let arts: Vec<(i64, String, Vec<u8>, Option<String>, String, chrono::DateTime<chrono::Utc>, i64)> =
            sqlx::query_as(
                "SELECT id, url, sha256, edition, archive_path, fetched_at, bytes FROM ops.artifacts WHERE id = ANY($1)",
            )
            .bind(&ids)
            .fetch_all(pool)
            .await?;
        let mut pdfs: HashMap<u32, Vec<u8>> = HashMap::new();
        for (ai, a) in e.artifacts.iter().enumerate() {
            let Some(m) = arts.iter().find(|r| r.0 == a.artifact_id) else {
                continue;
            };
            let _ = write!(
                provenance,
                "<tr><th>artifact {ai}</th><td><a href=\"{}\">{}</a> · edition {} · {} bytes · fetched {}<br><small>sha256 <code>{}</code> · key <code>{}</code></small></td></tr>",
                esc(&m.1),
                esc(&m.1),
                esc(m.3.as_deref().unwrap_or("—")),
                m.6,
                m.5.format("%Y-%m-%d %H:%M UTC"),
                hex::encode(&m.2),
                esc(&a.key)
            );
            let key = m.4.strip_prefix("raw/").unwrap_or(&m.4).to_owned();
            let sha: [u8; 32] = m.2.as_slice().try_into().context("artifact sha")?;
            if let Ok(b) = store
                .get(&BlobRef {
                    ns: Ns::Raw,
                    key,
                    sha256: sha,
                    bytes: 0,
                })
                .await
            {
                pdfs.insert(u32::try_from(ai)?, b.to_vec());
            }
        }

        // Tables of this extract.
        let mut tables: Vec<&str> = e.cells.iter().map(|c| c.table_id.as_str()).collect();
        tables.sort_unstable();
        tables.dedup();
        for table in tables {
            let cells: Vec<(u32, &Cell)> = e
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.table_id == table)
                .map(|(i, c)| (u32::try_from(i).unwrap_or(u32::MAX), c))
                .collect();
            let header_text: HashMap<&str, &str> = cells
                .iter()
                .filter(|(_, c)| c.r == 0)
                .filter_map(|(_, c)| c.col_header.as_deref().map(|k| (k, c.text.as_str())))
                .collect();
            let tpl = template_for(registry, &e.row.source_id, table);
            let _ = write!(
                html_tables,
                "<section><h2>Table <code>{}</code> · extract <code>x{}</code>{}</h2>",
                esc(table),
                e.row.id,
                tpl.map(|t| format!(" · template <code>{}</code>", esc(&t.id)))
                    .unwrap_or_default()
            );
            // Pages with the cell overlays.
            let mut pages: BTreeSet<(u32, i32)> = BTreeSet::new();
            for (_, c) in &cells {
                if let Some(p) = c.page {
                    pages.insert((c.artifact_idx, p));
                }
            }
            let k = f64::from(DPI) / 72.0;
            let mut page_files: HashMap<(u32, i32), String> = HashMap::new();
            for (ai, p) in &pages {
                let Some(pdf) = pdfs.get(ai) else { continue };
                let file = format!("page-a{ai}-p{p}.png");
                let path = dir.join(&file);
                let png = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(_) => {
                        let b = sandbox.render(pdf, u32::try_from(*p)?, DPI).await?;
                        std::fs::write(&path, &b)?;
                        b
                    }
                };
                let (w, h) = png_size(&png)?;
                pages_written += 1;
                page_files.insert((*ai, *p), file.clone());
                let _ = write!(
                    html_tables,
                    "<figure class=\"page\"><div class=\"pagewrap\" style=\"width:{w}px\"><img src=\"{file}\" width=\"{w}\" height=\"{h}\" alt=\"artifact {ai} page {p}\"><svg viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\">"
                );
                for (i, c) in cells
                    .iter()
                    .filter(|(_, c)| c.page == Some(*p) && c.artifact_idx == *ai)
                {
                    let Some(b) = c.bbox else { continue };
                    let cls = if c.r == 0 {
                        "hdr"
                    } else if c.c == 0 {
                        "lbl"
                    } else if failing.contains(&(
                        table.to_owned(),
                        c.row_header.clone().unwrap_or_default(),
                        c.col_header.clone().unwrap_or_default(),
                    )) {
                        "fail"
                    } else if c.conf.is_some_and(|v| v < 1.0) {
                        "low"
                    } else if obs_by_cell.contains_key(&(e.row.id, i32::try_from(*i).unwrap_or(-1)))
                    {
                        "obs"
                    } else {
                        "cell"
                    };
                    let _ = write!(
                        html_tables,
                        "<a href=\"#c{}-{i}\"><rect class=\"{cls}\" x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\"><title>row {i}: {} / {} = {}</title></rect></a>",
                        e.row.id,
                        f64::from(b[0]) * k - 1.0,
                        f64::from(b[1]) * k - 1.0,
                        f64::from(b[2] - b[0]) * k + 2.0,
                        f64::from(b[3] - b[1]) * k + 2.0,
                        esc(c.row_header.as_deref().unwrap_or("")),
                        esc(c.col_header.as_deref().unwrap_or("")),
                        esc(&c.text)
                    );
                }
                let _ = write!(
                    html_tables,
                    "</svg></div><figcaption>artifact {ai}, page {p}: <span class=\"k obs\">mapped to an observation</span> <span class=\"k cell\">extracted, not mapped</span> <span class=\"k low\">confidence &lt; 1</span> <span class=\"k fail\">failed check</span> <span class=\"k lbl\">row label</span> <span class=\"k hdr\">header</span></figcaption></figure>"
                );
            }

            // Per-cell table, ordered by risk.
            struct Row<'a> {
                idx: u32,
                cell: &'a Cell,
                risk: f64,
                why: Vec<&'static str>,
                obs: Vec<&'a Obs>,
            }
            let mut rows: Vec<Row> = Vec::new();
            for (i, c) in cells.iter().filter(|(_, c)| c.r > 0 && c.c > 0) {
                let os = obs_by_cell
                    .get(&(e.row.id, i32::try_from(*i).unwrap_or(-1)))
                    .cloned()
                    .unwrap_or_default();
                let mut risk = 0.0;
                let mut why = Vec::new();
                if failing.contains(&(
                    table.to_owned(),
                    c.row_header.clone().unwrap_or_default(),
                    c.col_header.clone().unwrap_or_default(),
                )) {
                    risk += 100.0;
                    why.push("failed check");
                }
                if let Some(cf) = c.conf.filter(|v| *v < 1.0) {
                    risk += 50.0 + f64::from(1.0 - cf) * 50.0;
                    why.push("low confidence");
                }
                if required.contains(i) {
                    risk += 10.0;
                    why.push(if c.conf.is_some_and(|v| v < 1.0) {
                        "sign-off"
                    } else {
                        "sample"
                    });
                }
                if c.vtype == CellType::String {
                    risk += 20.0;
                    why.push("not a number");
                }
                for o in &os {
                    match (o.value, o.prev) {
                        (Some(v), Some(p)) if p != 0.0 && ((v - p) / p).abs() > 0.05 => {
                            risk += 30.0;
                            why.push("change > 5 %");
                        }
                        (Some(v), Some(p)) if (v - p).abs() > f64::EPSILON => {
                            risk += 10.0;
                            why.push("revised");
                        }
                        (_, None) => {
                            risk += 1.0;
                            why.push("new");
                        }
                        _ => {}
                    }
                }
                rows.push(Row {
                    idx: *i,
                    cell: c,
                    risk,
                    why,
                    obs: os,
                });
            }
            n_cells += rows.len();
            rows.sort_by(|a, b| b.risk.total_cmp(&a.risk).then(a.idx.cmp(&b.idx)));
            let _ = write!(
                html_tables,
                "<table class=\"cells\"><thead><tr><th>#</th><th>risk</th><th>row</th><th>column</th><th>printed</th><th>crop</th><th>observation</th><th>value</th><th>previous</th><th>Δ</th><th>status</th><th>conf · reader</th>{}</tr></thead><tbody>",
                if class_c { "<th>sign-off</th>" } else { "" }
            );
            for r in &rows {
                let c = r.cell;
                let crop = match (
                    c.bbox,
                    c.page.and_then(|p| page_files.get(&(c.artifact_idx, p))),
                ) {
                    (Some(b), Some(f)) => {
                        let pad = 4.0;
                        let x = f64::from(b[0]) * k - pad;
                        let y = f64::from(b[1]) * k - pad;
                        let w = f64::from(b[2] - b[0]) * k + 2.0 * pad;
                        let h = f64::from(b[3] - b[1]) * k + 2.0 * pad;
                        format!(
                            "<span class=\"crop\" style=\"background-image:url({f});background-position:-{x:.0}px -{y:.0}px;width:{w:.0}px;height:{h:.0}px\"></span>"
                        )
                    }
                    _ => String::new(),
                };
                let (ob, val, prev, delta) = if r.obs.is_empty() {
                    (
                        "<span class=muted>not mapped</span>".to_owned(),
                        String::new(),
                        String::new(),
                        String::new(),
                    )
                } else {
                    let o = r.obs[0];
                    let d = match (o.value, o.prev) {
                        (Some(v), Some(p)) => {
                            let rel = if p == 0.0 {
                                String::new()
                            } else {
                                format!(" ({:+.2} %)", (v - p) / p * 100.0)
                            };
                            format!("{}{rel}", num(v - p))
                        }
                        (_, None) => "new".to_owned(),
                        _ => String::new(),
                    };
                    (
                        format!(
                            "<code>{}</code><br><small>{} · {} {}</small>",
                            esc(&o.indicator),
                            esc(&o.geo),
                            esc(&o.period),
                            esc(&o.dims)
                        ),
                        o.value.map(num).unwrap_or_default(),
                        o.prev.map(num).unwrap_or_else(|| "—".to_owned()),
                        d,
                    )
                };
                let status = r.obs.first().map(|o| o.status.clone()).unwrap_or_default();
                let sign = if class_c {
                    match decided.get(&(e.row.id, i32::try_from(r.idx).unwrap_or(-1))) {
                        Some(d) => format!(
                            "<b>{}</b>{} <small>by {}</small>",
                            esc(&d.decision),
                            d.corrected_text
                                .as_deref()
                                .map(|t| format!(" → <code>{}</code>", esc(t)))
                                .unwrap_or_default(),
                            esc(&d.reviewer)
                        ),
                        None if required.contains(&r.idx) => format!(
                            "<form method=\"post\" action=\"sign\"><input type=\"hidden\" name=\"extract\" value=\"{0}\"><input type=\"hidden\" name=\"row\" value=\"{1}\">\
                             <select name=\"decision\"><option>accept</option><option>correct</option><option>reject</option></select>\
                             <input name=\"text\" placeholder=\"text as printed\" size=\"8\"><input name=\"by\" placeholder=\"reviewer\" size=\"8\" required>\
                             <button>sign</button></form><small class=muted>or <code>lanka review sign x{0} --row {1} --decision accept --by NAME</code></small>",
                            e.row.id, r.idx
                        ),
                        None => "<span class=muted>—</span>".to_owned(),
                    }
                } else {
                    String::new()
                };
                let _ = write!(
                    html_tables,
                    "<tr id=\"c{}-{}\" class=\"{}\"><td>{}</td><td>{:.0}<br><small>{}</small></td><td>{}</td><td><code>{}</code><br><small>{}</small></td><td><code>{}</code></td><td>{crop}</td><td>{ob}</td><td class=n>{val}</td><td class=n>{prev}</td><td class=n>{delta}</td><td>{}</td><td>{} · {}</td>{}</tr>",
                    e.row.id,
                    r.idx,
                    if r.risk >= 50.0 {
                        "hi"
                    } else if r.risk >= 10.0 {
                        "mid"
                    } else {
                        ""
                    },
                    r.idx,
                    r.risk,
                    esc(&r.why.join(", ")),
                    esc(c.row_header.as_deref().unwrap_or("")),
                    esc(c.col_header.as_deref().unwrap_or("")),
                    esc(c
                        .col_header
                        .as_deref()
                        .and_then(|k| header_text.get(k).copied())
                        .unwrap_or("")),
                    esc(&c.text),
                    esc(&status),
                    c.conf.map_or_else(|| "—".to_owned(), |v| format!("{v:.2}")),
                    esc(&c.reader),
                    if class_c {
                        format!("<td>{sign}</td>")
                    } else {
                        String::new()
                    }
                );
            }
            html_tables.push_str("</tbody></table></section>");
        }
    }

    // ── name matches ──
    let mut names: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for o in &obs {
        if let Some(m) = o.locator.get("geo_match").filter(|m| !m.is_null()) {
            let name = m
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let method = m
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            *names.entry((name, o.geo.clone(), method)).or_default() += 1;
        }
    }
    let mut names_html = String::new();
    for ((name, code, method), n) in &names {
        let _ = write!(
            names_html,
            "<tr class=\"{}\"><td>{}</td><td><code>{}</code></td><td>{}</td><td class=n>{n}</td></tr>",
            if method == "name_exact" || method == "root_label" {
                ""
            } else {
                "mid"
            },
            esc(name),
            esc(code),
            esc(method)
        );
    }

    // ── diff: largest changes first ──
    let mut changes: Vec<&Obs> = obs
        .iter()
        .filter(|o| o.prev.is_some() && o.value != o.prev)
        .collect();
    changes.sort_by(|a, b| {
        let d = |o: &Obs| (o.value.unwrap_or(0.0) - o.prev.unwrap_or(0.0)).abs();
        d(b).total_cmp(&d(a))
    });
    let mut diff_html = String::new();
    let added = obs.iter().filter(|o| o.prev.is_none()).count();
    let _ = write!(
        diff_html,
        "<p>{} observation(s) in this version: {added} new key(s), {} changed value(s).{}</p>",
        obs.len(),
        changes.len(),
        ver.as_ref()
            .and_then(|v| v.diff.as_ref())
            .map(|d| format!(
                " Stage diff vs the released version: <code>{}</code>",
                esc(&d.to_string())
            ))
            .unwrap_or_else(
                || " First version of the dataset (nothing to compare against).".to_owned()
            )
    );
    if !changes.is_empty() {
        diff_html.push_str(
            "<table><tr><th>observation</th><th>previous</th><th>new</th><th>Δ</th></tr>",
        );
        for o in changes.iter().take(25) {
            let (v, p) = (o.value.unwrap_or(0.0), o.prev.unwrap_or(0.0));
            let _ = write!(
                diff_html,
                "<tr><td><code>{}</code> {} {} {}</td><td class=n>{}</td><td class=n>{}</td><td class=n>{}{}</td></tr>",
                esc(&o.indicator),
                esc(&o.geo),
                esc(&o.period),
                esc(&o.dims),
                num(p),
                num(v),
                num(v - p),
                if p == 0.0 {
                    String::new()
                } else {
                    format!(" ({:+.2} %)", (v - p) / p * 100.0)
                }
            );
        }
        diff_html.push_str("</table>");
    }

    // ── header ──
    let (title, meta) = match &ver {
        Some(v) => {
            let src = registry
                .datasets
                .get(&v.dataset_id)
                .map(|d| d.spec.source_ids.clone())
                .unwrap_or_default();
            let licences: Vec<String> = src
                .iter()
                .filter(|s| extracts.iter().any(|e| &e.row.source_id == *s))
                .filter_map(|s| registry.sources.get(s))
                .map(|s| {
                    let l = &s.spec.licence;
                    format!(
                        "{} — licence <code>{}</code> ({}){}{}",
                        esc(&s.spec.id),
                        esc(&l.id),
                        esc(&l.status),
                        l.attribution.as_deref().map(|a| format!(", attribution “{}”", esc(a))).unwrap_or_default(),
                        if l.status == "permission_unclear" { " — published with attribution and a takedown contact (policy); not a blocker" } else { "" }
                    )
                })
                .collect();
            (
                format!("{} · {}", v.dataset_id, target),
                format!(
                    "<tr><th>dataset</th><td><code>{}</code> — {}</td></tr><tr><th>version</th><td><code>{}</code> · seq {} · status <b>{}</b> · class {} · vintage {}</td></tr><tr><th>run</th><td>{}</td></tr><tr><th>source</th><td>{}</td></tr>",
                    esc(&v.dataset_id),
                    esc(&v.title),
                    esc(&target.to_string()),
                    v.seq,
                    esc(&v.status),
                    esc(&v.class),
                    esc(v.vintage.as_deref().unwrap_or("—")),
                    v.run_id.map_or_else(|| "—".to_owned(), |r| r.to_string()),
                    licences.join("<br>")
                ),
            )
        }
        None => {
            let e = extracts.first().context("no extract")?;
            (
                format!("extract x{} · {}", e.row.id, e.row.source_id),
                format!(
                    "<tr><th>subject</th><td>class C extract <code>x{}</code> of <code>{}</code> — every cell below confidence 1.0 and a 10 % sample need a decision before it can be approved; only then can standardise use it</td></tr>",
                    e.row.id,
                    esc(&e.row.source_id)
                ),
            )
        }
    };
    let validation = ver
        .as_ref()
        .and_then(|v| v.validation.as_ref())
        .map(|v| {
            format!(
                "<p>Staging validation: <code>{}</code></p>",
                esc(&v.to_string())
            )
        })
        .unwrap_or_default();
    let signoff = if signoff_required > 0 {
        format!(
            "<p class=\"{}\">Per-cell sign-off: <b>{} of {}</b> required cell(s) decided. Approve with <code>lanka review approve {}</code> once complete.</p>",
            if signoff_missing == 0 { "ok" } else { "bad" },
            signoff_required - signoff_missing,
            signoff_required,
            match target {
                Target::Extract(id) => format!("x{id} --by NAME"),
                Target::Version(d) => format!("{d} --by NAME"),
            }
        )
    } else {
        String::new()
    };
    let generated = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Evidence · {title_e}</title>
<style>
:root {{ --bg:#fbfaf7; --fg:#1d1d1b; --muted:#6b6a65; --line:#dedbd3; --ok:#1f7a3d; --bad:#b3261e; --mid:#a15c00; --obs:#1f7a3d; --cell:#3b6fb6; --low:#d98a00; --fail:#d0312d; --lbl:#7a5cc4; --hdr:#888; --hi:#fde8e6; --midbg:#fff4dc; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#17171a; --fg:#ecebe6; --muted:#9c9b95; --line:#34343a; --hi:#3b1f1d; --midbg:#3a2f14; }} }}
body {{ background:var(--bg); color:var(--fg); font:14px/1.45 system-ui, sans-serif; margin:0 16px 64px; }}
h1 {{ font-size:20px; margin:20px 0 4px; }} h2 {{ font-size:16px; margin:28px 0 8px; }}
table {{ border-collapse:collapse; margin:8px 0; }} th, td {{ border-bottom:1px solid var(--line); padding:4px 8px; text-align:left; vertical-align:top; }}
td.n {{ text-align:right; font-variant-numeric:tabular-nums; }} code {{ font-size:12px; }} small, .muted {{ color:var(--muted); }}
tr.bad td, p.bad {{ color:var(--bad); }} tr.ok td:first-child, p.ok {{ color:var(--ok); }} tr.hi {{ background:var(--hi); }} tr.mid {{ background:var(--midbg); }}
.page {{ margin:12px 0; overflow-x:auto; }} .pagewrap {{ position:relative; }} .pagewrap img, .pagewrap svg {{ position:absolute; left:0; top:0; }} .pagewrap img {{ position:relative; display:block; border:1px solid var(--line); }}
rect {{ fill:none; stroke-width:1.5; }} rect.obs {{ stroke:var(--obs); }} rect.cell {{ stroke:var(--cell); }} rect.low {{ stroke:var(--low); stroke-width:2.5; }} rect.fail {{ stroke:var(--fail); stroke-width:3; }} rect.lbl {{ stroke:var(--lbl); stroke-dasharray:3 2; }} rect.hdr {{ stroke:var(--hdr); stroke-dasharray:2 2; }}
.k {{ border-bottom:3px solid; margin-right:10px; }} .k.obs {{ border-color:var(--obs); }} .k.cell {{ border-color:var(--cell); }} .k.low {{ border-color:var(--low); }} .k.fail {{ border-color:var(--fail); }} .k.lbl {{ border-color:var(--lbl); }} .k.hdr {{ border-color:var(--hdr); }}
.crop {{ display:inline-block; background-repeat:no-repeat; border:1px solid var(--line); background-color:#fff; }}
.checklist label {{ display:block; margin:4px 0; }}
</style></head><body>
<h1>Evidence sheet — {title_e}</h1>
<p class="muted">Generated {generated} by lanka-export::evidence. Review subject <code>{target}</code>; review #{review_id}.</p>
<table>{meta}{provenance}</table>
{signoff}
<h2>Validation</h2>{validation}
<table><tr><th></th><th>template</th><th>check</th><th>n</th><th>failures</th></tr>{check_html}</table>
<h2>Diff against the current version</h2>{diff_html}
{html_tables}
<h2>Name matches</h2>
{names_block}
<h2>Checklist</h2>
<div class="checklist">
<label><input type="checkbox"> Totals agree with the printed totals (template checks above; spot-check the crops of the total rows).</label>
<label><input type="checkbox"> Units and multipliers read from the table header (e.g. "Thousands", "LKR billion") match the standardise scale.</label>
<label><input type="checkbox"> Footnotes read (provisional markers, coverage notes, breaks in series).</label>
<label><input type="checkbox"> The edition is the one intended (artifact URL and edition above).</label>
</div>
<p class="muted">Approve: <code>lanka review approve {target} --by NAME</code> · Reject: <code>lanka review reject {target} --by NAME --note TEXT</code></p>
</body></html>
"#,
        title_e = esc(&title),
        target = esc(&target.to_string()),
        names_block = if names_html.is_empty() {
            "<p class=muted>No geography was matched by name in this version.</p>".to_owned()
        } else {
            format!(
                "<table><tr><th>printed name</th><th>code</th><th>method</th><th>observations</th></tr>{names_html}</table>"
            )
        },
    );
    let index = dir.join("index.html");
    std::fs::write(&index, html)?;
    let key = format!("{}/{subject}/index.html", Ns::Evidence.dir());
    review::set_evidence_key(pool, review_id, &key).await?;
    Ok(Built {
        target: target.to_string(),
        key,
        index,
        review_id,
        cells: n_cells,
        pages: pages_written,
        checks_ok,
        signoff_required,
        signoff_missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(esc("<a&\"b'>"), "&lt;a&amp;&quot;b&#39;&gt;");
        assert_eq!(num(21_919_000.0), "21,919,000");
        assert_eq!(num(-1234.0), "-1,234");
        assert_eq!(num(47.4), "47.4");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1240u32.to_be_bytes());
        png.extend(1754u32.to_be_bytes());
        assert_eq!(png_size(&png).expect("png"), (1240, 1754));
        assert!(png_size(b"nope").is_err());
    }
}
