//! `pdf_table`: text-layer PDF tables read through registry templates
//! (03 §2.2, §3). Class B: every version goes to review with an evidence
//! sheet.
//!
//! ```yaml
//! source: dcs-statistical-abstract
//! driver: pdf_table
//! version: 1.0.0
//! discover:
//!   files:
//!     - { key: abstract-2025-2.1a, url: https://www.statistics.gov.lk/abstract2025/CHAP2/2.1a.pdf, edition: "2025" }
//! extract:
//!   tables:
//!     - { id: t2_1a, file: abstract-2025-2.1a, template: dcs-statistical-abstract/table-2.1a-midyear-population-district }
//! standardise:
//!   dataset: dcs-midyear-population-district
//!   vintage: "Statistical Abstract {edition}"
//!   outputs:
//!     - table: t2_1a
//!       indicator: pop.midyear_district
//!       geo: { from_label: { level: 2, root: ["Sri Lanka"] } }
//!       period: { from_header: year }
//!       columns: { total_2024: { sex: total }, male_2024: { sex: male } }
//!       scale: 1000
//!       status: { header_markers: { "(1)": provisional } }
//! ```
//!
//! An output may name its own `dataset` (default `standardise.dataset`), so one
//! publication feeds several datasets; each dataset is one output of the run.
//! `geo.from_label.level` 2 uses the census district index, any other level the
//! registry's unit names (+ reviewed aliases); `skip` drops labels that are not units.
//!
//! Extract: the sandbox reads the text layer (`pdftotext -bbox-layout`),
//! the template locates and reads the table, the template checks run and a
//! failure fails the extract. One `cells` extract holds every table: per
//! table a header row (`r = 0`: header text as printed, `col_header` = the
//! column key) and one row per data line (`c = 0` the label, then one cell
//! per printed token with page and bbox, reader `pdftotext`).
//!
//! Standardise maps (row, column) cells to observations: geography from the
//! row label (name → p-code through `geo.units` and the reviewed alias list)
//! or fixed; period from the column header, the row label or fixed; dims from
//! the column and row maps; `scale` applies the table's unit multiplier.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use lanka_core::{Basis, Flags, Method, Status};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::connector::{
    Candidate, CellProv, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched, ObsRow,
    OutputHeader, OutputKind, OutputSink, StdCtx, UnitRef, dims_key,
};
use crate::connectors::dcs_census::NameIndex;
use crate::extract::{Cell, CellType, ExtractOut, ExtractRef};
use crate::registry::{ConnectorSpec, Loaded};
use crate::template::{self, Template, Value, failed_summary, norm};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub discover: DiscoverSpec,
    pub extract: ExtractSpec,
    pub standardise: StdSpec,
    #[serde(default)]
    pub expect: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    pub files: Vec<FileSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub key: String,
    pub url: String,
    /// Edition label (selects the template edition).
    #[serde(default)]
    pub edition: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractSpec {
    pub tables: Vec<TableSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    pub id: String,
    /// `discover.files[].key`.
    pub file: String,
    /// `registry/templates/<template>.yaml`.
    pub template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdSpec {
    pub dataset: String,
    #[serde(default)]
    pub vintage: Option<String>,
    pub outputs: Vec<OutSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelGeo {
    /// Admin level of the printed names.
    pub level: i16,
    /// Labels meaning the whole country.
    #[serde(default)]
    pub root: Vec<String>,
    /// Labels that are not units (reporting areas, notes): rows dropped.
    #[serde(default)]
    pub skip: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoSel {
    /// `root` or a code.
    #[serde(default)]
    pub fixed: Option<String>,
    #[serde(default)]
    pub from_label: Option<LabelGeo>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeriodParse {
    /// The first four-digit year in the text.
    Year,
    /// "End June 2026" → `2026-Q2` (the quarter a month falls in).
    QuarterEnd,
    /// "2026 January" → `2026-01`; a label with only a month name takes the
    /// year of the nearest row above that printed one (from_label only).
    Month,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodSel {
    #[serde(default)]
    pub from_header: Option<PeriodParse>,
    #[serde(default)]
    pub from_label: Option<PeriodParse>,
    #[serde(default)]
    pub fixed: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSel {
    #[serde(default)]
    pub default: Option<Status>,
    /// Marker in the column header text → status.
    #[serde(default)]
    pub header_markers: BTreeMap<String, Status>,
    /// Marker in the row label → status.
    #[serde(default)]
    pub label_markers: BTreeMap<String, Status>,
}

pub type DimMap = BTreeMap<String, String>;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutSpec {
    pub table: String,
    /// Default: `standardise.dataset`.
    #[serde(default)]
    pub dataset: Option<String>,
    pub indicator: String,
    pub geo: GeoSel,
    pub period: PeriodSel,
    /// Column key → dims (`{}` for none).
    pub columns: BTreeMap<String, DimMap>,
    /// Row reference (`"Label"`, `"Label#2"`) → dims; absent = every row.
    #[serde(default)]
    pub rows: Option<BTreeMap<String, DimMap>>,
    /// Multiplier from the printed unit to the indicator's unit.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub status: StatusSel,
}

const fn one() -> f64 {
    1.0
}

/// The `pdf_table` driver for one source.
#[derive(Debug, Clone)]
pub struct PdfTable {
    spec: Spec,
    templates: HashMap<String, Template>,
    /// Connector spec + every template it uses.
    sha256: [u8; 32],
}

/// Reader recorded on text-layer cells.
pub const READER: &str = "pdftotext";

impl PdfTable {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: Spec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid pdf_table spec", l.path.display()))?;
        let root = l
            .path
            .parent()
            .and_then(Path::parent)
            .context("connector spec outside a registry")?;
        Self::new(spec, l.sha256, root)
    }

    pub fn new(spec: Spec, spec_sha: [u8; 32], registry_root: &Path) -> Result<Self> {
        let mut h = Sha256::new();
        h.update(spec_sha);
        let mut templates = HashMap::new();
        let mut ids: Vec<&str> = spec
            .extract
            .tables
            .iter()
            .map(|t| t.template.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let (t, sha) = template::load(registry_root, id)?;
            h.update(id.as_bytes());
            h.update(sha);
            templates.insert(id.to_owned(), t);
        }
        for t in &spec.extract.tables {
            ensure!(
                spec.discover.files.iter().any(|f| f.key == t.file),
                "table {}: no file {:?} in discover.files",
                t.id,
                t.file
            );
        }
        for o in &spec.standardise.outputs {
            ensure!(
                spec.extract.tables.iter().any(|t| t.id == o.table),
                "output {}: unknown table {:?}",
                o.indicator,
                o.table
            );
            let g = &o.geo;
            ensure!(
                g.fixed.is_some() != g.from_label.is_some(),
                "output {}: geo needs exactly one of fixed, from_label",
                o.indicator
            );
            let p = &o.period;
            ensure!(
                usize::from(p.from_header.is_some())
                    + usize::from(p.from_label.is_some())
                    + usize::from(p.fixed.is_some())
                    == 1,
                "output {}: period needs exactly one of from_header, from_label, fixed",
                o.indicator
            );
            ensure!(
                o.scale.is_finite() && o.scale > 0.0,
                "output {}: bad scale",
                o.indicator
            );
        }
        Ok(Self {
            spec,
            templates,
            sha256: h.finalize().into(),
        })
    }

    #[must_use]
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// The template of a table id.
    #[must_use]
    pub fn template_of(&self, table_id: &str) -> Option<(&TableSpec, &Template)> {
        let t = self.spec.extract.tables.iter().find(|t| t.id == table_id)?;
        Some((t, self.templates.get(&t.template)?))
    }

    fn file(&self, key: &str) -> Option<&FileSpec> {
        self.spec.discover.files.iter().find(|f| f.key == key)
    }

    /// Edition label of a discovered file.
    #[must_use]
    pub fn file_edition(&self, key: &str) -> Option<&str> {
        self.file(key).and_then(|f| f.edition.as_deref())
    }
}

/// Rebuilds a [`template::Table`] from extract cells, so template checks can
/// be re-run on a stored extract (evidence sheets) exactly as at extraction.
#[must_use]
pub fn table_from_cells(cells: &[Cell], table_id: &str, ed: &template::Edition) -> template::Table {
    let mine: Vec<&Cell> = cells.iter().filter(|c| c.table_id == table_id).collect();
    let mut headers: Vec<(u32, template::THeader)> = mine
        .iter()
        .filter(|c| c.r == 0)
        .map(|c| {
            (
                c.c,
                template::THeader {
                    key: c.col_header.clone().unwrap_or_default(),
                    text: c.text.clone(),
                    page: c.page.and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
                    bbox: c.bbox.unwrap_or([0.0; 4]),
                },
            )
        })
        .collect();
    headers.sort_by_key(|h| h.0);
    let mut rows: BTreeMap<u32, template::TRow> = BTreeMap::new();
    for c in mine.iter().filter(|c| c.r > 0) {
        let page = c.page.and_then(|p| u32::try_from(p).ok()).unwrap_or(0);
        let row = rows.entry(c.r).or_insert_with(|| template::TRow {
            label: String::new(),
            label_page: page,
            label_bbox: [0.0; 4],
            cells: BTreeMap::new(),
        });
        if c.c == 0 {
            row.label.clone_from(&c.text);
            row.label_page = page;
            row.label_bbox = c.bbox.unwrap_or([0.0; 4]);
        } else if let Some(k) = &c.col_header {
            row.cells.insert(
                k.clone(),
                template::TCell {
                    text: c.text.clone(),
                    page,
                    bbox: c.bbox.unwrap_or([0.0; 4]),
                    value: ed.number.parse(&c.text),
                },
            );
        }
    }
    template::Table {
        template_id: String::new(),
        edition_idx: 0,
        first_page: headers.first().map_or(0, |h| h.1.page),
        headers: headers.into_iter().map(|h| h.1).collect(),
        rows: rows.into_values().collect(),
    }
}

/// Template + table ids of every `pdf_table` table in a set of cells, for
/// evidence sheets.
#[must_use]
pub fn table_ids(cells: &[Cell]) -> Vec<String> {
    let mut v: Vec<String> = cells.iter().map(|c| c.table_id.clone()).collect();
    v.dedup();
    v.sort();
    v.dedup();
    v
}

/// Reads one table: words → template → checks → cells.
pub async fn read_table(
    sandbox: &lanka_extractd::Sandbox,
    pdf: &[u8],
    template: &Template,
    edition: Option<&str>,
    table_id: &str,
    artifact_idx: u32,
) -> Result<(Vec<Cell>, Vec<template::CheckResult>)> {
    let words = sandbox
        .pdf_words(pdf, 0, 0)
        .await
        .context("reading the text layer")?;
    ensure!(
        !words.words.is_empty(),
        "{}: the PDF has no text layer (scanned?): use `lanka extract propose` (class C)",
        template.id
    );
    let table = template.read(&words, edition)?;
    let (_, ed) = template.edition(edition)?;
    let results = table.check(ed);
    Ok((
        table_cells(&table, table_id, artifact_idx, READER, None),
        results,
    ))
}

/// Cells of a read table (`r = 0` headers, then rows; `c = 0` the label).
#[must_use]
pub fn table_cells(
    table: &template::Table,
    table_id: &str,
    artifact_idx: u32,
    reader: &str,
    conf: Option<f32>,
) -> Vec<Cell> {
    let mut out = Vec::new();
    let col_of: HashMap<&str, u32> = table
        .headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.key.as_str(), u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    let base = |r: u32, c: u32| Cell {
        table_id: table_id.to_owned(),
        r,
        c,
        text: String::new(),
        vtype: CellType::String,
        page: None,
        bbox: None,
        sheet: None,
        a1: None,
        row_header: None,
        col_header: None,
        artifact_idx,
        conf: Some(1.0),
        reader: reader.to_owned(),
    };
    for (c, h) in table.headers.iter().enumerate() {
        let c = u32::try_from(c).unwrap_or(u32::MAX);
        let mut cell = base(0, c);
        cell.text.clone_from(&h.text);
        cell.page = i32::try_from(h.page).ok();
        cell.bbox = (h.bbox != [0.0; 4]).then_some(h.bbox);
        cell.col_header = Some(h.key.clone());
        out.push(cell);
    }
    let label_key = table.headers.first().map(|h| h.key.clone());
    for (i, row) in table.rows.iter().enumerate() {
        let r = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let mut l = base(r, 0);
        l.text.clone_from(&row.label);
        l.page = i32::try_from(row.label_page).ok();
        l.bbox = Some(row.label_bbox);
        l.row_header = Some(row.label.clone());
        l.col_header.clone_from(&label_key);
        out.push(l);
        let mut cols: Vec<(&String, &template::TCell)> = row.cells.iter().collect();
        cols.sort_by_key(|(k, _)| col_of.get(k.as_str()).copied().unwrap_or(u32::MAX));
        for (k, tc) in cols {
            let mut cell = base(r, col_of.get(k.as_str()).copied().unwrap_or(u32::MAX));
            cell.text.clone_from(&tc.text);
            cell.vtype = if matches!(tc.value, Value::Number(_)) {
                CellType::Number
            } else {
                CellType::String
            };
            cell.page = i32::try_from(tc.page).ok();
            cell.bbox = Some(tc.bbox);
            cell.row_header = Some(row.label.clone());
            cell.col_header = Some(k.clone());
            cell.conf = conf.or(Some(1.0));
            out.push(cell);
        }
    }
    out
}

/// A table as read back from extract cells.
#[derive(Debug, Default)]
pub struct CellTable<'a> {
    /// Column key → (header text, cell index).
    pub headers: BTreeMap<String, (&'a str, u32)>,
    /// Rows in order: label, label cell index, column key → cell index.
    pub rows: Vec<(&'a str, u32, BTreeMap<&'a str, u32>)>,
    pub artifact_idx: u32,
}

/// Groups the cells of one table (by `table_id`).
pub fn cell_table<'a>(cells: &'a [Cell], table_id: &str) -> Result<CellTable<'a>> {
    let mut t = CellTable::default();
    let mut rows: BTreeMap<u32, (&str, u32, BTreeMap<&str, u32>)> = BTreeMap::new();
    let mut seen = false;
    for (i, c) in cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.table_id == table_id)
    {
        let i = u32::try_from(i)?;
        seen = true;
        t.artifact_idx = c.artifact_idx;
        let key = c
            .col_header
            .as_deref()
            .context("cell without a column key")?;
        if c.r == 0 {
            t.headers.insert(key.to_owned(), (c.text.as_str(), i));
        } else if c.c == 0 {
            rows.entry(c.r)
                .or_insert_with(|| ("", 0, BTreeMap::new()))
                .0 = c.text.as_str();
            rows.entry(c.r).or_default().1 = i;
        } else {
            rows.entry(c.r)
                .or_insert_with(|| ("", 0, BTreeMap::new()))
                .2
                .insert(key, i);
        }
    }
    ensure!(seen, "table {table_id} is not in the extract");
    t.rows = rows.into_values().collect();
    Ok(t)
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Period code from printed text.
pub fn parse_period(how: PeriodParse, text: &str) -> Result<String> {
    if how == PeriodParse::Month {
        return parse_month(text, None);
    }
    let year = text
        .split(|c: char| !c.is_ascii_digit())
        .find(|t| t.len() == 4 && (t.starts_with("19") || t.starts_with("20")))
        .with_context(|| format!("no year in {text:?}"))?;
    match how {
        PeriodParse::Year => Ok(year.to_owned()),
        PeriodParse::Month => parse_month(text, None),
        PeriodParse::QuarterEnd => {
            let lower = text.to_ascii_lowercase();
            let m = lower
                .split(|c: char| !c.is_ascii_alphabetic())
                .find_map(|w| MONTHS.iter().position(|m| w.len() >= 3 && w.starts_with(m)))
                .with_context(|| format!("no month in {text:?}"))?;
            Ok(format!("{year}-Q{}", m / 3 + 1))
        }
    }
}

fn year_in(text: &str) -> Option<&str> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|t| t.len() == 4 && (t.starts_with("19") || t.starts_with("20")))
}

/// `YYYY-MM` from a month name and a year in the text, or `carried`.
fn parse_month(text: &str, carried: Option<&str>) -> Result<String> {
    let year = year_in(text)
        .or(carried)
        .with_context(|| format!("no year in {text:?} (and none above)"))?;
    let lower = text.to_ascii_lowercase();
    let m = lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .find_map(|w| MONTHS.iter().position(|m| w.len() >= 3 && w.starts_with(m)))
        .with_context(|| format!("no month in {text:?}"))?;
    Ok(format!("{year}-{:02}", m + 1))
}

#[async_trait]
impl Connector for PdfTable {
    fn source_id(&self) -> &str {
        &self.spec.source
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    fn extractor(&self) -> &str {
        "pdf_table"
    }

    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let mut candidates = Vec::new();
        for f in &self.spec.discover.files {
            let local_path = ctx
                .opts
                .from_dir
                .as_ref()
                .map(|d| d.join(format!("{}.pdf", f.key)));
            if let Some(p) = &local_path {
                ensure!(p.is_file(), "--from: {} is missing", p.display());
            }
            candidates.push(Candidate {
                key: f.key.clone(),
                url: f.url.clone(),
                local_path,
                media_type: Some("application/pdf".to_owned()),
                edition: f.edition.clone(),
                ..Candidate::default()
            });
        }
        ensure!(!candidates.is_empty(), "pdf_table: no files");
        Ok(Discovery {
            candidates,
            fingerprint: None,
        })
    }

    async fn extract(&self, ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        let mut cells = Vec::new();
        for t in &self.spec.extract.tables {
            let (ai, f) = fetched
                .iter()
                .enumerate()
                .find(|(_, f)| f.key == t.file)
                .with_context(|| format!("table {}: file {} was not fetched", t.id, t.file))?;
            let tpl = &self.templates[&t.template];
            let edition = self.file(&t.file).and_then(|f| f.edition.as_deref());
            let pdf = f.read()?;
            let (tc, results) =
                read_table(ctx.sandbox, &pdf, tpl, edition, &t.id, u32::try_from(ai)?)
                    .await
                    .with_context(|| format!("table {} ({})", t.id, t.template))?;
            if let Some(s) = failed_summary(&results) {
                bail!(
                    "table {} ({}): template check failed: {s}",
                    t.id,
                    t.template
                );
            }
            cells.extend(tc);
        }
        Ok(vec![ExtractOut::Cells(cells)])
    }

    #[allow(clippy::too_many_lines)]
    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        let s = &self.spec.standardise;
        let ds_of = |o: &OutSpec| o.dataset.clone().unwrap_or_else(|| s.dataset.clone());
        for o in &s.outputs {
            let id = ds_of(o);
            let ds = ctx
                .registry
                .dataset(&id)
                .with_context(|| format!("registry has no dataset {id}"))?;
            ensure!(
                ds.indicators.iter().any(|i| i.id == o.indicator),
                "{id} does not declare {}",
                o.indicator
            );
        }
        let need_names = s.outputs.iter().any(|o| o.geo.from_label.is_some());
        let alias_path = ctx.registry.root.join("crosswalks/ds-name-aliases.csv");
        let level_names = if need_names {
            let aliases = std::fs::read_to_string(&alias_path).ok();
            Some(crate::drivers::table::Names::new(
                &ctx.lookup.units,
                aliases.as_deref(),
            )?)
        } else {
            None
        };
        let names = if need_names {
            let units: Vec<UnitRef> = ctx
                .lookup
                .units
                .iter()
                .filter(|u| u.level >= 1)
                .cloned()
                .collect();
            ensure!(
                !units.is_empty(),
                "no admin units in geo.units: run `lanka ingest run hdx-cod-ab-lka` first"
            );
            let aliases = std::fs::read_to_string(&alias_path).unwrap_or_default();
            Some(NameIndex::new(&units, &aliases)?)
        } else {
            None
        };
        let root = ctx.profile.admin.root().to_owned();
        // Dataset → (editions, rows), in the order of first appearance.
        let mut by_ds: Vec<(String, Vec<String>, Vec<ObsRow>)> = Vec::new();
        for (ei, x) in extracts.iter().enumerate() {
            let cells = x.data.cells();
            for o in &s.outputs {
                if !cells.iter().any(|c| c.table_id == o.table) {
                    continue;
                }
                let (tspec, tpl) = self
                    .template_of(&o.table)
                    .with_context(|| format!("unknown table {}", o.table))?;
                let edition = self.file(&tspec.file).and_then(|f| f.edition.clone());
                let ds_id = ds_of(o);
                let di = if let Some(i) = by_ds.iter().position(|d| d.0 == ds_id) {
                    i
                } else {
                    by_ds.push((ds_id, Vec::new(), Vec::new()));
                    by_ds.len() - 1
                };
                if let Some(e) = &edition
                    && !by_ds[di].1.contains(e)
                {
                    by_ds[di].1.push(e.clone());
                }
                let (_, ed) = tpl.edition(edition.as_deref())?;
                let t = cell_table(cells, &o.table)?;
                // Row selection: every row, or the mapped ones (by label + occurrence).
                let mut picked: Vec<(usize, DimMap)> = Vec::new();
                match &o.rows {
                    None => picked.extend((0..t.rows.len()).map(|i| (i, DimMap::new()))),
                    Some(map) => {
                        for (reference, dims) in map {
                            let (label, nth) = match reference.rsplit_once('#') {
                                Some((l, n)) if n.parse::<usize>().is_ok() => {
                                    (l, n.parse().unwrap_or(1))
                                }
                                _ => (reference.as_str(), 1),
                            };
                            let want = norm(label);
                            let i = t
                                .rows
                                .iter()
                                .enumerate()
                                .filter(|(_, r)| norm(r.0) == want)
                                .nth(nth - 1)
                                .map(|(i, _)| i)
                                .with_context(|| {
                                    format!("{}: row {reference:?} not in the table", o.table)
                                })?;
                            picked.push((i, dims.clone()));
                        }
                    }
                }
                // Year carried down the rows for month labels without one.
                let mut years: Vec<Option<String>> = Vec::with_capacity(t.rows.len());
                let mut last: Option<String> = None;
                for r in &t.rows {
                    if let Some(y) = year_in(r.0) {
                        last = Some(y.to_owned());
                    }
                    years.push(last.clone());
                }
                for (ri, row_dims) in picked {
                    let (label, label_cell, by_col) = &t.rows[ri];
                    let geo = match (&o.geo.fixed, &o.geo.from_label) {
                        (Some(f), _) if f == "root" => (root.clone(), None),
                        (Some(f), _) => (f.clone(), None),
                        (None, Some(lg)) => {
                            if lg.root.iter().any(|r| norm(r) == norm(label)) {
                                (root.clone(), Some("root_label"))
                            } else if lg.skip.iter().any(|r| norm(r) == norm(label)) {
                                continue;
                            } else if lg.level != 2 {
                                let idx = level_names.as_ref().context("name index")?;
                                let (code, method) =
                                    idx.resolve(lg.level, label).with_context(|| {
                                        format!(
                                            "{}: unmatched name {label:?} (level {})",
                                            o.table, lg.level
                                        )
                                    })?;
                                (code, Some(method))
                            } else {
                                let idx = names.as_ref().context("name index")?;
                                let m = idx.district(label).with_context(|| {
                                    format!(
                                        "{}: unmatched name {label:?} (level {})",
                                        o.table, lg.level
                                    )
                                })?;
                                let method = match m {
                                    crate::connectors::dcs_census::Match::Exact(_) => "name_exact",
                                    crate::connectors::dcs_census::Match::Alias(_) => "name_alias",
                                };
                                let code = match m {
                                    crate::connectors::dcs_census::Match::Exact(p)
                                    | crate::connectors::dcs_census::Match::Alias(p) => p,
                                };
                                (code, Some(method))
                            }
                        }
                        (None, None) => bail!("geo"),
                    };
                    for (col, col_dims) in &o.columns {
                        let Some(&ci) = by_col.get(col.as_str()) else {
                            continue;
                        };
                        let cell = &cells[ci as usize];
                        let value = match ed.number.parse(&cell.text) {
                            Value::Number(v) => v * o.scale,
                            Value::Missing => continue,
                            Value::Text => bail!(
                                "{}: {label:?}/{col}: {:?} is not a number",
                                o.table,
                                cell.text
                            ),
                        };
                        let header = t.headers.get(col.as_str()).map_or("", |h| h.0);
                        let period =
                            match (&o.period.from_header, &o.period.from_label, &o.period.fixed) {
                                (Some(p), _, _) => parse_period(*p, header)?,
                                (_, Some(PeriodParse::Month), _) => {
                                    parse_month(label, years[ri].as_deref())?
                                }
                                (_, Some(p), _) => parse_period(*p, label)?,
                                (_, _, Some(f)) => f.clone(),
                                _ => bail!("period"),
                            };
                        let mut status = o.status.default.unwrap_or(Status::Normal);
                        for (m, st) in &o.status.header_markers {
                            if header.contains(m.as_str()) {
                                status = *st;
                            }
                        }
                        for (m, st) in &o.status.label_markers {
                            if label.contains(m.as_str()) {
                                status = *st;
                            }
                        }
                        let mut dims: Vec<(&str, &str)> = col_dims
                            .iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect();
                        dims.extend(row_dims.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                        let art = x.artifact(cell.artifact_idx)?;
                        by_ds[di].2.push(ObsRow {
                            indicator_id: o.indicator.clone(),
                            geo_code: geo.0.clone(),
                            period,
                            dims: dims_key(&dims),
                            value: Some(value),
                            lo: None,
                            hi: None,
                            basis: Basis::Reported,
                            status,
                            flags: Flags::EMPTY,
                            artifact: cell.artifact_idx as usize,
                            prov: Some(CellProv { extract: ei, row: ci }),
                            locator: serde_json::json!({
                                "url": art.url, "page": cell.page, "bbox": cell.bbox,
                                "table": o.table, "template": tspec.template, "edition": edition,
                                "row": label, "col": col, "header": header,
                                "label_cell": label_cell,
                                "geo_match": geo.1.map(|m| serde_json::json!({ "name": label, "method": m })),
                            }),
                            verbatim: Some(cell.text.clone()),
                            method: if cell.reader == READER { Method::PdfText } else { Method::LlmDual },
                            confidence: cell.conf,
                        });
                    }
                }
            }
        }
        ensure!(
            by_ds.iter().any(|d| !d.2.is_empty()),
            "pdf_table: no observations from the extract"
        );
        for (dataset_id, editions, rows) in by_ds {
            let vintage = s
                .vintage
                .as_ref()
                .map(|v| v.replace("{edition}", &editions.join(", ")));
            sink.begin(OutputHeader {
                dataset_id,
                kind: OutputKind::Observations,
                vintage,
            })?;
            for r in rows {
                sink.obs(r)?;
            }
            sink.end()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_from_printed_headers() {
        assert_eq!(
            parse_period(PeriodParse::Year, "2020(1) Total;").expect("y"),
            "2020"
        );
        assert_eq!(
            parse_period(PeriodParse::Year, "2024 *").expect("y"),
            "2024"
        );
        assert_eq!(
            parse_period(PeriodParse::QuarterEnd, "End June 2026 USD million").expect("q"),
            "2026-Q2"
        );
        assert_eq!(
            parse_period(PeriodParse::QuarterEnd, "End March 2026 LKR billion").expect("q"),
            "2026-Q1"
        );
        assert!(parse_period(PeriodParse::Year, "Total").is_err());
        assert_eq!(
            parse_period(PeriodParse::Month, "2022 January").expect("m"),
            "2022-01"
        );
        assert_eq!(parse_month("February", Some("2023")).expect("m"), "2023-02");
        assert!(parse_month("March", None).is_err());
    }
}
