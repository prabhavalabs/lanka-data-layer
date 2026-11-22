//! The selector language shared by the tabular drivers (03 §2.2): how an L1
//! extract (cells of a sheet or HTML table, records of a CSV or JSON API)
//! becomes tables with named columns, and how table rows become
//! observations or features.
//!
//! ```yaml
//! standardise:
//!   dataset: dcs-paddy-statistics
//!   tables:
//!     - id: production_maha
//!       artifact: "^production_maha$"          # regex on the artifact key (default: all)
//!       header: { anchor: "^DISTRICT$", rows: 1, fill_right: true }
//!       rows: { until_blank: 2, stop: ["^Source"] }
//!       rename: { "2000/2021": "2020/2021" }   # printed-header corrections, reviewed in the spec
//!       fill_down: [Year]
//!   outputs:
//!     - table: production_maha
//!       indicator: agri.paddy_production_t
//!       melt: { columns: "^\\d{4}/\\d{4}$" }    # wide → long: one row per matching column
//!       geo: { column: DISTRICT, match: name, level: 2, root_names: ["SRI LANKA"], skip: ["UDAWALAWE"] }
//!       period: { melt: true, regex: "^(\\d{4})/\\d{4}$", replace: "${1}-Maha", freq: S }
//!       value: { number: { thousands: ",", missing: ["-", ""] } }
//! ```
//!
//! A *source* (`Src`) names where a text comes from — a column (`column` or
//! a bare string), a column `index`, the melted header (`melt: true`), a
//! constant (`value`), a `template` over columns (`{Year} {Month}`,
//! `{melt}`, `{key}`), or the first row of another table (`table` +
//! `column`) — and how it is transformed, in this order: trim, `regex`
//! (first capture group, or `replace` with `$1`-style references), `date`
//! (`parse` with a chrono format, re-`format`), `map` (exact lookup;
//! `unmapped: error | drop | keep`), `case`. A source that yields nothing
//! drops the row (an empty cell is no observation).
//!
//! Every value keeps its provenance: the extract row of the cell it was
//! read from, the printed token as `verbatim`, and a locator (`sheet` +
//! `a1`, HTML `table`/`row`/`col`, CSV `row` + `column`, JSON `path`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use lanka_core::period::{Freq, Period};
use lanka_core::{Basis, Flags, Method, Status};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::connector::{
    CellProv, FeatureIn, ObsRow, OutputHeader, OutputKind, OutputSink, StdCtx, UnitRef,
};
use crate::extract::{CellType, ExtractRef};
use crate::registry::{FieldSpec, Registry};

// ───────────────────────────── table model ─────────────────────────────────

/// Where a table cell was printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loc {
    Sheet { sheet: String, a1: String },
    Html { table: u32, row: u32, col: u32 },
    Record { path: String, field: String },
}

/// One cell of an assembled table, with its provenance.
#[derive(Debug, Clone)]
pub struct TCell {
    pub text: String,
    pub vtype: CellType,
    /// Index into the extracts given to `standardise`.
    pub extract: usize,
    /// Row of that extract.
    pub row: u32,
    pub artifact: u32,
    pub loc: Loc,
}

/// A table with named columns.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub id: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<TCell>>>,
    /// Artifact key of each row (for `{key}`).
    pub keys: Vec<String>,
    /// Artifact URL of each row (locators).
    pub urls: Vec<String>,
}

impl Table {
    #[must_use]
    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

/// Collapses whitespace runs to one space and trims.
#[must_use]
pub fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ───────────────────────────── table specs ─────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderSpec {
    /// Regex matched against cell text; the first matching cell (row-major)
    /// starts the header.
    pub anchor: String,
    /// Header rows (their texts are joined with a space per column).
    #[serde(default = "one")]
    pub rows: u32,
    /// Carry a header text right over empty cells (merged headers) on all
    /// but the last header row.
    #[serde(default)]
    pub fill_right: bool,
}

const fn one() -> u32 {
    1
}

const fn two() -> u32 {
    2
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowsSpec {
    /// The table ends after this many consecutive empty rows.
    #[serde(default = "two")]
    pub until_blank: u32,
    /// The table ends at a row whose first text matches one of these.
    #[serde(default)]
    pub stop: Vec<String>,
    /// Rows whose first text matches one of these are skipped.
    #[serde(default)]
    pub skip: Vec<String>,
}

impl Default for RowsSpec {
    fn default() -> Self {
        Self {
            until_blank: 2,
            stop: Vec::new(),
            skip: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    pub id: String,
    /// Regex on the artifact key (default: every artifact).
    #[serde(default)]
    pub artifact: Option<String>,
    /// Regex on the extract table id (`<key>!<sheet>`, `<key>#<n>`, a CSV
    /// key or a JSON table id).
    #[serde(default)]
    pub source: Option<String>,
    /// Cells only: where the header is.
    #[serde(default)]
    pub header: Option<HeaderSpec>,
    #[serde(default)]
    pub rows: RowsSpec,
    /// Header text corrections (printed typos).
    #[serde(default)]
    pub rename: BTreeMap<String, String>,
    /// Columns whose empty cells repeat the cell above (merged row labels).
    #[serde(default)]
    pub fill_down: Vec<String>,
}

// ───────────────────────────── sources ─────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unmapped {
    #[default]
    Error,
    Drop,
    Keep,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    Upper,
    Lower,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateConv {
    /// chrono format of the text (a missing day defaults to 1).
    pub parse: String,
    /// chrono format of the result.
    pub format: String,
}

/// Where a text comes from, and how it is transformed (module docs).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SrcFull {
    pub column: Option<String>,
    pub index: Option<usize>,
    pub melt: bool,
    pub value: Option<String>,
    pub template: Option<String>,
    /// With `column`: read the first row of this table instead.
    pub table: Option<String>,
    pub regex: Option<String>,
    pub replace: Option<String>,
    pub date: Option<DateConv>,
    pub map: BTreeMap<String, String>,
    pub unmapped: Unmapped,
    pub case: Option<Case>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Src {
    Column(String),
    Full(Box<SrcFull>),
}

impl Src {
    fn full(&self) -> SrcFull {
        match self {
            Self::Column(c) => SrcFull {
                column: Some(c.clone()),
                ..SrcFull::default()
            },
            Self::Full(f) => (**f).clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub column: String,
    #[serde(default)]
    pub eq: Option<String>,
    #[serde(default, rename = "in")]
    pub any_of: Vec<String>,
    #[serde(default)]
    pub not_in: Vec<String>,
    #[serde(default)]
    pub regex: Option<String>,
    /// Keep rows where the column is empty (true) or not empty (false).
    #[serde(default)]
    pub empty: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeltSpec {
    /// Regex on column names: each matching column becomes a row.
    pub columns: String,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeoMatch {
    /// The text is a geo code (p-code or `scheme:code`) after `map`.
    Code,
    /// The text is a printed name, matched to a unit of `level`.
    Name,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeoOut {
    /// The profile's root unit.
    #[serde(default)]
    pub root: bool,
    /// A fixed geo code.
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default, rename = "match")]
    pub matching: Option<GeoMatch>,
    #[serde(default)]
    pub level: Option<i16>,
    /// Names that mean the root unit ("SRI LANKA", "Total").
    #[serde(default)]
    pub root_names: Vec<String>,
    /// Names that are not units (irrigation systems, notes): rows dropped.
    #[serde(default)]
    pub skip: Vec<String>,
    #[serde(flatten)]
    pub src: SrcFull,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeriodParse {
    /// Already canonical for the frequency.
    AsIs,
    /// Four digits.
    Year,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PeriodOut {
    /// Frequency code (`A`, `M`, `S`, …) or registry period type (`year`, …).
    pub freq: String,
    /// `as_is` (default), `year`, or a chrono format (`%Y %B`; a missing
    /// day defaults to 1) whose date is the start of the period.
    #[serde(default)]
    pub parse: Option<String>,
    #[serde(flatten)]
    pub src: SrcFull,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberSpec {
    #[serde(default)]
    pub thousands: Option<String>,
    #[serde(default)]
    pub decimal: Option<String>,
    /// Tokens meaning "no value" (row skipped).
    #[serde(default)]
    pub missing: Vec<String>,
    /// Multiplier applied to the parsed value (the verbatim keeps the token).
    #[serde(default)]
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ValueOut {
    #[serde(default)]
    pub number: Option<NumberSpec>,
    #[serde(flatten)]
    pub src: SrcFull,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusOut {
    #[serde(default)]
    pub codes: BTreeMap<String, (Basis, Status)>,
    #[serde(default = "reported_normal")]
    pub default: (Basis, Status),
    #[serde(flatten)]
    pub src: SrcFull,
}

const fn reported_normal() -> (Basis, Status) {
    (Basis::Reported, Status::Normal)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureOut {
    pub key: Src,
    pub kind: Src,
    #[serde(default)]
    pub name: Option<Src>,
    pub lon: Src,
    pub lat: Src,
    #[serde(default)]
    pub attrs: BTreeMap<String, Src>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    pub table: String,
    /// Default: `standardise.dataset`.
    #[serde(default)]
    pub dataset: Option<String>,
    #[serde(default)]
    pub filter: Vec<Filter>,
    #[serde(default)]
    pub melt: Option<MeltSpec>,
    // observations
    #[serde(default)]
    pub indicator: Option<Src>,
    #[serde(default)]
    pub geo: Option<GeoOut>,
    #[serde(default)]
    pub period: Option<PeriodOut>,
    #[serde(default)]
    pub dims: BTreeMap<String, Src>,
    #[serde(default)]
    pub value: Option<ValueOut>,
    #[serde(default)]
    pub lo: Option<ValueOut>,
    #[serde(default)]
    pub hi: Option<ValueOut>,
    #[serde(default)]
    pub status: Option<StatusOut>,
    #[serde(default)]
    pub method: Option<Method>,
    // features
    #[serde(default)]
    pub feature: Option<FeatureOut>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VintageSpec {
    Fixed(String),
    From(Box<SrcFull>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdSpec {
    pub dataset: String,
    #[serde(default)]
    pub vintage: Option<VintageSpec>,
    #[serde(default)]
    pub tables: Vec<TableSpec>,
    pub outputs: Vec<OutputSpec>,
    /// Warn when an output delivers nothing (otherwise an error).
    #[serde(default)]
    pub allow_empty: bool,
}

// ───────────────────────────── compiled ────────────────────────────────────

fn re(s: &str) -> Result<Regex> {
    Regex::new(s).with_context(|| format!("bad regex {s:?}"))
}

/// A compiled source.
#[derive(Debug, Clone)]
struct CSrc {
    f: SrcFull,
    regex: Option<Regex>,
}

impl CSrc {
    fn new(f: SrcFull) -> Result<Self> {
        let n = usize::from(f.column.is_some())
            + usize::from(f.index.is_some())
            + usize::from(f.melt)
            + usize::from(f.value.is_some())
            + usize::from(f.template.is_some());
        ensure!(
            n <= 1,
            "a source takes one of column, index, melt, value, template"
        );
        ensure!(
            f.table.is_none() || f.column.is_some(),
            "`table` needs `column`"
        );
        ensure!(
            f.replace.is_none() || f.regex.is_some(),
            "`replace` needs `regex`"
        );
        let regex = f.regex.as_deref().map(re).transpose()?;
        Ok(Self { f, regex })
    }

    fn has_base(&self) -> bool {
        self.f.column.is_some()
            || self.f.index.is_some()
            || self.f.melt
            || self.f.value.is_some()
            || self.f.template.is_some()
    }
}

/// One row being mapped: the table row plus the melted column, if any.
#[derive(Clone, Copy)]
struct RowCtx<'a> {
    t: &'a Table,
    r: usize,
    melt: Option<usize>,
    tables: &'a HashMap<String, Table>,
}

impl<'a> RowCtx<'a> {
    fn cell(&self, c: usize) -> Option<&'a TCell> {
        self.t.rows[self.r].get(c).and_then(Option::as_ref)
    }

    fn named(&self, name: &str) -> Result<Option<&'a TCell>> {
        let c = self.t.col(name).with_context(|| {
            format!(
                "table {}: no column {name:?} (columns: {})",
                self.t.id,
                self.t.columns.join(" | ")
            )
        })?;
        Ok(self.cell(c))
    }

    /// The cell a source reads (for verbatim and provenance).
    fn source_cell(&self, s: &CSrc) -> Result<Option<&'a TCell>> {
        if s.f.table.is_some() {
            return Ok(None);
        }
        if let Some(c) = &s.f.column {
            return self.named(c);
        }
        if let Some(i) = s.f.index {
            return Ok(self.cell(i));
        }
        if !s.has_base() || s.f.melt {
            // a value with no base reads the melted cell
            return Ok(self.melt.and_then(|m| self.cell(m)));
        }
        Ok(None)
    }

    fn base(&self, s: &CSrc, value_default: bool) -> Result<Option<String>> {
        let f = &s.f;
        if let (Some(t), Some(c)) = (&f.table, &f.column) {
            let other = self
                .tables
                .get(t)
                .with_context(|| format!("no table {t:?}"))?;
            let ci = other
                .col(c)
                .with_context(|| format!("table {t}: no column {c:?}"))?;
            return Ok(other
                .rows
                .iter()
                .find_map(|r| r.get(ci).and_then(Option::as_ref))
                .map(|c| c.text.clone()));
        }
        if let Some(v) = &f.value {
            return Ok(Some(v.clone()));
        }
        if let Some(tpl) = &f.template {
            return self.template(tpl);
        }
        if f.melt && !value_default {
            let m = self.melt.context("`melt: true` without a `melt` section")?;
            return Ok(Some(self.t.columns[m].clone()));
        }
        if !s.has_base() && !value_default {
            bail!("source has no column, index, melt, value or template");
        }
        Ok(self.source_cell(s)?.map(|c| c.text.clone()))
    }

    fn template(&self, tpl: &str) -> Result<Option<String>> {
        let mut out = String::new();
        let mut rest = tpl;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let j = rest[i..]
                .find('}')
                .with_context(|| format!("unclosed placeholder in {tpl:?}"))?;
            let name = &rest[i + 1..i + j];
            let v = match name {
                "melt" => self.melt.map(|m| self.t.columns[m].clone()),
                "key" => self.t.keys.get(self.r).cloned(),
                _ => self.named(name)?.map(|c| c.text.clone()),
            };
            match v {
                Some(v) if !v.trim().is_empty() => out.push_str(v.trim()),
                _ => return Ok(None),
            }
            rest = &rest[i + j + 1..];
        }
        out.push_str(rest);
        Ok(Some(out))
    }

    /// Evaluates a source: `Ok(None)` drops the row.
    fn eval(&self, s: &CSrc, value_default: bool) -> Result<Option<String>> {
        let Some(raw) = self.base(s, value_default)? else {
            return Ok(None);
        };
        let mut v = clean(&raw);
        if v.is_empty() {
            return Ok(None);
        }
        if let Some(rx) = &s.regex {
            let Some(caps) = rx.captures(&v) else {
                return match s.f.unmapped {
                    Unmapped::Drop => Ok(None),
                    Unmapped::Keep => Ok(Some(v)),
                    Unmapped::Error => bail!("{v:?} does not match {}", rx.as_str()),
                };
            };
            v = match &s.f.replace {
                Some(r) => {
                    let mut o = String::new();
                    caps.expand(r, &mut o);
                    o
                }
                None => caps
                    .get(1)
                    .or_else(|| caps.get(0))
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_default(),
            };
        }
        if let Some(d) = &s.f.date {
            let date = parse_date(&v, &d.parse)
                .with_context(|| format!("{v:?} is not a date of format {:?}", d.parse))?;
            v = date.format(&d.format).to_string();
        }
        if !s.f.map.is_empty() {
            match s.f.map.get(&v) {
                Some(m) => v.clone_from(m),
                None => match s.f.unmapped {
                    Unmapped::Drop => return Ok(None),
                    Unmapped::Keep => {}
                    Unmapped::Error => bail!("{v:?} is not in the map"),
                },
            }
        }
        match s.f.case {
            Some(Case::Upper) => v = v.to_uppercase(),
            Some(Case::Lower) => v = v.to_lowercase(),
            None => {}
        }
        Ok((!v.is_empty()).then_some(v))
    }
}

/// Parses a date with a chrono format; a format without a day gets day 1.
pub fn parse_date(text: &str, fmt: &str) -> Option<chrono::NaiveDate> {
    use chrono::NaiveDate;
    NaiveDate::parse_from_str(text, fmt).ok().or_else(|| {
        if fmt.contains("%d") || fmt.contains("%e") {
            return None;
        }
        NaiveDate::parse_from_str(&format!("{text} 1"), &format!("{fmt} %d")).ok()
    })
}

// ───────────────────────────── assembly ────────────────────────────────────

/// Builds the declared tables from the extracts (pure).
pub fn assemble(specs: &[TableSpec], extracts: &[ExtractRef]) -> Result<HashMap<String, Table>> {
    let mut out = HashMap::new();
    for spec in specs {
        let art = spec.artifact.as_deref().map(re).transpose()?;
        let src = spec.source.as_deref().map(re).transpose()?;
        let mut table = Table {
            id: spec.id.clone(),
            ..Table::default()
        };
        let mut matched = 0usize;
        for (ei, x) in extracts.iter().enumerate() {
            // cells grouped by (table_id, artifact)
            let mut cell_groups: BTreeMap<(String, u32), Vec<u32>> = BTreeMap::new();
            for (i, c) in x.data.cells().iter().enumerate() {
                cell_groups
                    .entry((c.table_id.clone(), c.artifact_idx))
                    .or_default()
                    .push(u32::try_from(i)?);
            }
            let mut rec_groups: BTreeMap<(String, u32), Vec<u32>> = BTreeMap::new();
            for (i, r) in x.data.records().iter().enumerate() {
                rec_groups
                    .entry((r.table_id.clone(), r.artifact_idx))
                    .or_default()
                    .push(u32::try_from(i)?);
            }
            for ((tid, ai), idx) in cell_groups {
                let a = x.artifact(ai)?;
                if !art.as_ref().is_none_or(|r| r.is_match(&a.key))
                    || !src.as_ref().is_none_or(|r| r.is_match(&tid))
                {
                    continue;
                }
                let h = spec
                    .header
                    .as_ref()
                    .with_context(|| format!("table {}: cells need a `header`", spec.id))?;
                let part = from_cells(spec, h, ei, x, &idx)
                    .with_context(|| format!("table {} from {tid}", spec.id))?;
                merge(&mut table, part, &a.key, &a.url);
                matched += 1;
            }
            for ((tid, ai), idx) in rec_groups {
                let a = x.artifact(ai)?;
                if !art.as_ref().is_none_or(|r| r.is_match(&a.key))
                    || !src.as_ref().is_none_or(|r| r.is_match(&tid))
                {
                    continue;
                }
                let part = from_records(spec, ei, x, &idx);
                merge(&mut table, part, &a.key, &a.url);
                matched += 1;
            }
        }
        ensure!(
            matched > 0,
            "table {}: no extract table matches (artifact {:?}, source {:?})",
            spec.id,
            spec.artifact,
            spec.source
        );
        for name in &spec.fill_down {
            let c = table
                .col(name)
                .with_context(|| format!("table {}: fill_down column {name:?}", spec.id))?;
            let mut last: Option<TCell> = None;
            for row in &mut table.rows {
                match row.get(c).and_then(Option::as_ref) {
                    Some(cell) => last = Some(cell.clone()),
                    None => {
                        if row.len() <= c {
                            row.resize(c + 1, None);
                        }
                        row[c].clone_from(&last);
                    }
                }
            }
        }
        out.insert(spec.id.clone(), table);
    }
    Ok(out)
}

/// Appends `part` to `t`, aligning columns by name.
fn merge(t: &mut Table, part: Table, key: &str, url: &str) {
    let map: Vec<usize> = part
        .columns
        .iter()
        .map(|c| {
            t.col(c).unwrap_or_else(|| {
                t.columns.push(c.clone());
                t.columns.len() - 1
            })
        })
        .collect();
    for row in part.rows {
        let mut r = vec![None; t.columns.len()];
        for (i, c) in row.into_iter().enumerate() {
            if let Some(c) = c {
                r[map[i]] = Some(c);
            }
        }
        t.rows.push(r);
        t.keys.push(key.to_owned());
        t.urls.push(url.to_owned());
    }
    let n = t.columns.len();
    for r in &mut t.rows {
        r.resize(n, None);
    }
}

fn from_records(spec: &TableSpec, ei: usize, x: &ExtractRef, idx: &[u32]) -> Table {
    let recs = x.data.records();
    let mut cols: BTreeSet<&str> = BTreeSet::new();
    for &i in idx {
        cols.extend(recs[i as usize].fields.keys().map(String::as_str));
    }
    let columns: Vec<String> = cols
        .iter()
        .map(|c| {
            spec.rename
                .get(*c)
                .cloned()
                .unwrap_or_else(|| (*c).to_owned())
        })
        .collect();
    let pos: HashMap<&str, usize> = cols.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let mut rows = Vec::with_capacity(idx.len());
    for &i in idx {
        let r = &recs[i as usize];
        let mut row = vec![None; columns.len()];
        for (k, v) in &r.fields {
            if v.trim().is_empty() {
                continue;
            }
            row[pos[k.as_str()]] = Some(TCell {
                text: v.clone(),
                vtype: CellType::String,
                extract: ei,
                row: i,
                artifact: r.artifact_idx,
                loc: Loc::Record {
                    path: r.path.clone(),
                    field: k.clone(),
                },
            });
        }
        rows.push(row);
    }
    Table {
        id: spec.id.clone(),
        columns,
        rows,
        ..Table::default()
    }
}

fn from_cells(
    spec: &TableSpec,
    h: &HeaderSpec,
    ei: usize,
    x: &ExtractRef,
    idx: &[u32],
) -> Result<Table> {
    let cells = x.data.cells();
    let mut grid: BTreeMap<u32, BTreeMap<u32, u32>> = BTreeMap::new();
    for &i in idx {
        let c = &cells[i as usize];
        if !c.text.trim().is_empty() {
            grid.entry(c.r).or_default().insert(c.c, i);
        }
    }
    let anchor = re(&h.anchor)?;
    let (hr, hc) = grid
        .iter()
        .find_map(|(r, cols)| {
            cols.iter()
                .find(|(_, i)| anchor.is_match(&clean(&cells[**i as usize].text)))
                .map(|(c, _)| (*r, *c))
        })
        .with_context(|| format!("header anchor {:?} not found (template miss)", h.anchor))?;
    let header_rows: Vec<u32> = (hr..hr + h.rows).collect();
    let max_c = header_rows
        .iter()
        .filter_map(|r| grid.get(r))
        .filter_map(|cols| cols.keys().next_back().copied())
        .max()
        .unwrap_or(hc);
    let ncols = (max_c - hc + 1) as usize;
    let mut parts: Vec<Vec<String>> = vec![Vec::new(); ncols];
    for (k, r) in header_rows.iter().enumerate() {
        let last = k + 1 == header_rows.len();
        let mut carry: Option<String> = None;
        for (j, part) in parts.iter_mut().enumerate() {
            let c = hc + u32::try_from(j)?;
            let t = grid
                .get(r)
                .and_then(|cols| cols.get(&c))
                .map(|i| clean(&cells[*i as usize].text))
                .filter(|t| !t.is_empty());
            let t = match t {
                Some(t) => {
                    carry = Some(t.clone());
                    Some(t)
                }
                None if h.fill_right && !last => carry.clone(),
                None => None,
            };
            if let Some(t) = t {
                part.push(t);
            }
        }
    }
    let mut columns = Vec::with_capacity(ncols);
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (j, p) in parts.iter().enumerate() {
        let mut name = if p.is_empty() {
            format!(
                "#{}",
                crate::connectors::dcs_census::col_letters(hc + u32::try_from(j)?)
            )
        } else {
            p.join(" ")
        };
        if let Some(fix) = spec.rename.get(&name) {
            name.clone_from(fix);
        }
        let n = seen.entry(name.clone()).or_default();
        *n += 1;
        if *n > 1 {
            name = format!("{name} ({n})");
        }
        columns.push(name);
    }
    let stop: Vec<Regex> = spec
        .rows
        .stop
        .iter()
        .map(|s| re(s))
        .collect::<Result<_>>()?;
    let skip: Vec<Regex> = spec
        .rows
        .skip
        .iter()
        .map(|s| re(s))
        .collect::<Result<_>>()?;
    let last_header = hr + h.rows - 1;
    let max_row = grid.keys().next_back().copied().unwrap_or(last_header);
    let mut rows = Vec::new();
    let mut blanks = 0u32;
    let mut started = false;
    for r in last_header + 1..=max_row {
        let in_range: Vec<(u32, u32)> = grid
            .get(&r)
            .map(|cols| {
                cols.range(hc..=max_c)
                    .map(|(c, i)| (*c, *i))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if in_range.is_empty() {
            if started {
                blanks += 1;
                if blanks >= spec.rows.until_blank {
                    break;
                }
            }
            continue;
        }
        blanks = 0;
        let first = clean(&cells[in_range[0].1 as usize].text);
        if stop.iter().any(|s| s.is_match(&first)) {
            break;
        }
        started = true;
        if skip.iter().any(|s| s.is_match(&first)) {
            continue;
        }
        let mut row = vec![None; ncols];
        for (c, i) in in_range {
            let cell = &cells[i as usize];
            let loc = match (&cell.sheet, &cell.a1) {
                (Some(s), Some(a1)) => Loc::Sheet {
                    sheet: s.clone(),
                    a1: a1.clone(),
                },
                _ => Loc::Html {
                    table: cell
                        .table_id
                        .rsplit('#')
                        .next()
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0),
                    row: cell.r,
                    col: cell.c,
                },
            };
            row[(c - hc) as usize] = Some(TCell {
                text: cell.text.clone(),
                vtype: cell.vtype,
                extract: ei,
                row: i,
                artifact: cell.artifact_idx,
                loc,
            });
        }
        rows.push(row);
    }
    Ok(Table {
        id: spec.id.clone(),
        columns,
        rows,
        ..Table::default()
    })
}

// ───────────────────────────── names → codes ───────────────────────────────

/// Lowercase ASCII letters and digits only ("Nuwara-Eliya" == "NUWARA ELIYA").
#[must_use]
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Printed unit names → codes, per admin level, with the reviewed aliases
/// of `registry/crosswalks/ds-name-aliases.csv` (and level suffixes like
/// "District" stripped).
#[derive(Debug, Default)]
pub struct Names {
    by_level: HashMap<i16, HashMap<String, String>>,
    alias: HashMap<(i16, String), String>,
}

impl Names {
    pub fn new(units: &[UnitRef], aliases_csv: Option<&str>) -> Result<Self> {
        let mut by_level: HashMap<i16, HashMap<String, String>> = HashMap::new();
        for u in units {
            by_level
                .entry(u.level)
                .or_default()
                .insert(norm(strip_suffix(&u.name_en)), u.code.clone());
        }
        let mut alias = HashMap::new();
        for line in aliases_csv
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
        {
            let f: Vec<&str> = line.splitn(4, ',').collect();
            ensure!(f.len() >= 3, "bad alias line {line:?}");
            let level: i16 = f[0]
                .parse()
                .with_context(|| format!("alias level {line:?}"))?;
            alias.insert((level, f[1].to_owned()), f[2].to_owned());
        }
        Ok(Self { by_level, alias })
    }

    /// `None` when the name is not a unit of `level`.
    #[must_use]
    pub fn resolve(&self, level: i16, name: &str) -> Option<(String, &'static str)> {
        let m = self.by_level.get(&level)?;
        let n = norm(strip_suffix(name));
        if let Some(c) = m.get(&n) {
            return Some((c.clone(), "name_exact"));
        }
        let a = self.alias.get(&(level, n))?;
        m.get(a).map(|c| (c.clone(), "name_alias"))
    }
}

fn strip_suffix(s: &str) -> &str {
    let t = s.trim();
    for suf in [
        " District",
        " DISTRICT",
        " Province",
        " PROVINCE",
        " District Secretariat",
    ] {
        if let Some(x) = t.strip_suffix(suf) {
            return x;
        }
    }
    t
}

// ───────────────────────────── mapping ─────────────────────────────────────

fn parse_number(text: &str, spec: Option<&NumberSpec>) -> Result<Option<f64>> {
    let t = text.trim();
    if let Some(n) = spec
        && n.missing.iter().any(|m| m.trim() == t)
    {
        return Ok(None);
    }
    if t.is_empty() {
        return Ok(None);
    }
    let mut s = t.to_owned();
    if let Some(th) = spec.and_then(|n| n.thousands.as_deref()) {
        s = s.replace(th, "");
    }
    if let Some(d) = spec.and_then(|n| n.decimal.as_deref())
        && d != "."
    {
        s = s.replace(d, ".");
    }
    let s = s.replace(' ', "");
    let v: f64 = s
        .parse()
        .with_context(|| format!("non-numeric value {t:?}"))?;
    ensure!(v.is_finite(), "non-finite value {t:?}");
    Ok(Some(v * spec.and_then(|n| n.scale).unwrap_or(1.0)))
}

fn freq_of(s: &str) -> Result<Freq> {
    if s.len() == 1
        && let Some(f) = s.chars().next().and_then(Freq::from_char)
    {
        return Ok(f);
    }
    Freq::from_period_type(s).with_context(|| format!("unknown frequency {s:?}"))
}

fn period_code(
    text: &str,
    p: &PeriodOut,
    freq: Freq,
    cal: &lanka_core::Calendars,
) -> Result<String> {
    let period = match p.parse.as_deref() {
        None | Some("as_is") => Period::parse(freq, text, cal),
        Some("year") => {
            ensure!(
                text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()),
                "{text:?} is not a year"
            );
            Period::parse(freq, text, cal)
        }
        Some(fmt) => {
            let d = parse_date(text, fmt)
                .with_context(|| format!("{text:?} is not a date of format {fmt:?}"))?;
            Period::new(freq, d, cal)
        }
    }
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(period.render(cal))
}

fn locator(t: &Table, r: usize, cell: Option<&TCell>) -> Value {
    let url = t.urls.get(r).cloned().unwrap_or_default();
    match cell.map(|c| &c.loc) {
        Some(Loc::Sheet { sheet, a1 }) => json!({ "url": url, "sheet": sheet, "cell": a1 }),
        Some(Loc::Html { table, row, col }) => {
            json!({ "url": url, "table": table, "row": row, "col": col })
        }
        Some(Loc::Record { path, field }) => json!({ "url": url, "path": path, "field": field }),
        None => json!({ "url": url }),
    }
}

/// The compiled output of one `outputs[]` entry.
struct COut<'s> {
    filters: Vec<(usize, &'s Filter, Option<Regex>)>,
    melt: Vec<usize>,
    indicator: Option<CSrc>,
    geo: Option<(CSrc, &'s GeoOut)>,
    period: Option<(CSrc, Freq)>,
    dims: Vec<(String, CSrc)>,
    value: Option<CSrc>,
    lo: Option<CSrc>,
    hi: Option<CSrc>,
    status: Option<CSrc>,
}

impl<'s> COut<'s> {
    fn new(spec: &'s OutputSpec, t: &Table) -> Result<Self> {
        let mut filters = Vec::new();
        for f in &spec.filter {
            let c = t
                .col(&f.column)
                .with_context(|| format!("filter: no column {:?} in {}", f.column, t.id))?;
            filters.push((c, f, f.regex.as_deref().map(re).transpose()?));
        }
        let melt = match &spec.melt {
            None => Vec::new(),
            Some(m) => {
                let rx = re(&m.columns)?;
                let v: Vec<usize> = t
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| rx.is_match(c) && !m.exclude.contains(c))
                    .map(|(i, _)| i)
                    .collect();
                ensure!(
                    !v.is_empty(),
                    "melt: no column of {} matches {:?} (columns: {})",
                    t.id,
                    m.columns,
                    t.columns.join(" | ")
                );
                v
            }
        };
        let opt = |s: &Option<Src>| s.as_ref().map(|s| CSrc::new(s.full())).transpose();
        Ok(Self {
            filters,
            melt,
            indicator: opt(&spec.indicator)?,
            geo: spec
                .geo
                .as_ref()
                .map(|g| CSrc::new(g.src.clone()).map(|c| (c, g)))
                .transpose()?,
            period: spec
                .period
                .as_ref()
                .map(|p| -> Result<_> { Ok((CSrc::new(p.src.clone())?, freq_of(&p.freq)?)) })
                .transpose()?,
            dims: spec
                .dims
                .iter()
                .map(|(k, s)| CSrc::new(s.full()).map(|c| (k.clone(), c)))
                .collect::<Result<_>>()?,
            value: spec
                .value
                .as_ref()
                .map(|v| CSrc::new(v.src.clone()))
                .transpose()?,
            lo: spec
                .lo
                .as_ref()
                .map(|v| CSrc::new(v.src.clone()))
                .transpose()?,
            hi: spec
                .hi
                .as_ref()
                .map(|v| CSrc::new(v.src.clone()))
                .transpose()?,
            status: spec
                .status
                .as_ref()
                .map(|s| CSrc::new(s.src.clone()))
                .transpose()?,
        })
    }

    fn keep(&self, t: &Table, r: usize) -> bool {
        self.filters.iter().all(|(c, f, rx)| {
            let v = t.rows[r]
                .get(*c)
                .and_then(Option::as_ref)
                .map(|c| clean(&c.text))
                .unwrap_or_default();
            f.eq.as_ref().is_none_or(|e| &v == e)
                && (f.any_of.is_empty() || f.any_of.contains(&v))
                && !f.not_in.contains(&v)
                && rx.as_ref().is_none_or(|r| r.is_match(&v))
                && f.empty.is_none_or(|e| e == v.is_empty())
        })
    }
}

/// Outcome counters of one output (warnings).
#[derive(Debug, Default)]
struct Counts {
    rows: usize,
    /// Observation keys delivered so far in this dataset → where.
    keys: HashMap<(String, String, String, String), String>,
    skipped_names: BTreeMap<String, usize>,
    unmatched: BTreeSet<String>,
}

/// Maps assembled tables to outputs (the tabular drivers' `standardise`).
pub fn standardise(
    spec: &StdSpec,
    ctx: &StdCtx<'_>,
    extracts: &[ExtractRef],
    sink: &mut dyn OutputSink,
) -> Result<()> {
    let tables = assemble(&spec.tables, extracts)?;
    let tables = with_record_tables(spec, tables, extracts)?;
    let alias_path = ctx.registry.root.join("crosswalks/ds-name-aliases.csv");
    let aliases = std::fs::read_to_string(&alias_path).ok();
    let names = Names::new(&ctx.lookup.units, aliases.as_deref())?;
    // Outputs of the same dataset are written into one version.
    let mut order: Vec<String> = Vec::new();
    let mut by_ds: HashMap<String, Vec<&OutputSpec>> = HashMap::new();
    for o in &spec.outputs {
        let ds = o.dataset.clone().unwrap_or_else(|| spec.dataset.clone());
        if !by_ds.contains_key(&ds) {
            order.push(ds.clone());
        }
        by_ds.entry(ds).or_default().push(o);
    }
    let vintage = match &spec.vintage {
        None => None,
        Some(VintageSpec::Fixed(s)) => Some(s.clone()),
        Some(VintageSpec::From(f)) => {
            let s = CSrc::new((**f).clone())?;
            let t = Table::default();
            let ctx_row = RowCtx {
                t: &t,
                r: 0,
                melt: None,
                tables: &tables,
            };
            ensure!(f.table.is_some(), "vintage: give `table` and `column`");
            ctx_row.eval(&s, false)?
        }
    };
    for ds_id in order {
        let ds = ctx
            .registry
            .dataset(&ds_id)
            .with_context(|| format!("registry has no dataset {ds_id}"))?;
        let outs = &by_ds[&ds_id];
        let features = outs.iter().any(|o| o.feature.is_some());
        ensure!(
            outs.iter().all(|o| o.feature.is_some() == features),
            "{ds_id}: outputs mix observations and features"
        );
        sink.begin(OutputHeader {
            dataset_id: ds_id.clone(),
            kind: if features {
                OutputKind::Features
            } else {
                OutputKind::Observations
            },
            vintage: vintage.clone(),
        })?;
        let mut keys: HashMap<(String, String, String, String), String> = HashMap::new();
        for o in outs {
            let t = tables
                .get(&o.table)
                .with_context(|| format!("output: no table {:?}", o.table))?;
            let mut counts = Counts {
                keys: std::mem::take(&mut keys),
                ..Counts::default()
            };
            if features {
                map_features(ctx.registry, ds, o, t, &tables, sink, &mut counts)
                    .with_context(|| format!("{ds_id} from table {}", t.id))?;
            } else {
                map_obs(ctx, ds, o, t, &tables, &names, sink, &mut counts)
                    .with_context(|| format!("{ds_id} from table {}", t.id))?;
            }
            keys = std::mem::take(&mut counts.keys);
            for (n, k) in &counts.skipped_names {
                ctx.warn(format!(
                    "{ds_id}/{}: {k} row(s) for {n:?} skipped (not a unit)",
                    t.id
                ));
            }
            if counts.rows == 0 {
                let msg = format!("{ds_id}: table {} produced no rows", t.id);
                if spec.allow_empty {
                    ctx.warn(msg);
                } else {
                    bail!(msg);
                }
            }
        }
        sink.end()?;
    }
    Ok(())
}

/// Record extracts need no assembly spec: a table per `table_id` is made
/// available under that id when no declared table has it.
fn with_record_tables(
    spec: &StdSpec,
    mut tables: HashMap<String, Table>,
    extracts: &[ExtractRef],
) -> Result<HashMap<String, Table>> {
    let wanted: BTreeSet<&str> = spec
        .outputs
        .iter()
        .map(|o| o.table.as_str())
        .chain(
            spec.outputs
                .iter()
                .flat_map(|o| o.dims.values().chain(o.indicator.iter()))
                .filter_map(|s| match s {
                    Src::Full(f) => f.table.as_deref(),
                    Src::Column(_) => None,
                }),
        )
        .chain(match &spec.vintage {
            Some(VintageSpec::From(f)) => f.table.as_deref(),
            _ => None,
        })
        .filter(|t| !tables.contains_key(*t))
        .collect();
    for id in wanted {
        let auto = TableSpec {
            id: id.to_owned(),
            artifact: None,
            source: Some(format!("^{}$", regex::escape(id))),
            header: None,
            rows: RowsSpec::default(),
            rename: BTreeMap::new(),
            fill_down: Vec::new(),
        };
        let t = assemble(std::slice::from_ref(&auto), extracts)?;
        tables.extend(t);
    }
    Ok(tables)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn map_obs(
    ctx: &StdCtx<'_>,
    ds: &crate::registry::DatasetSpec,
    o: &OutputSpec,
    t: &Table,
    tables: &HashMap<String, Table>,
    names: &Names,
    sink: &mut dyn OutputSink,
    counts: &mut Counts,
) -> Result<()> {
    let c = COut::new(o, t)?;
    let ind_src = c.indicator.as_ref().context("output needs `indicator`")?;
    let geo = c.geo.as_ref().context("output needs `geo`")?;
    let (period_src, freq) = c.period.as_ref().context("output needs `period`")?;
    let value_src = c.value.as_ref().context("output needs `value`")?;
    let vspec = o.value.as_ref().and_then(|v| v.number.as_ref());
    let declared: BTreeSet<&str> = ds.indicators.iter().map(|i| i.id.as_str()).collect();
    let method = o.method.unwrap_or(Method::SheetCell);
    let root = ctx.profile.admin.root().to_owned();
    let melt: Vec<Option<usize>> = if c.melt.is_empty() {
        vec![None]
    } else {
        c.melt.iter().copied().map(Some).collect()
    };
    for r in 0..t.rows.len() {
        if !c.keep(t, r) {
            continue;
        }
        for m in &melt {
            let rc = RowCtx {
                t,
                r,
                melt: *m,
                tables,
            };
            let at = || {
                format!(
                    "table {} row {}{}",
                    t.id,
                    r + 1,
                    m.map(|m| format!(" column {:?}", t.columns[m]))
                        .unwrap_or_default()
                )
            };
            let vcell = rc.source_cell(value_src)?;
            let Some(vtext) = rc.eval(value_src, true).with_context(at)? else {
                continue;
            };
            let Some(value) = parse_number(&vtext, vspec).with_context(at)? else {
                continue;
            };
            let Some(indicator) = rc.eval(ind_src, false).with_context(at)? else {
                continue;
            };
            ensure!(
                declared.contains(indicator.as_str()),
                "{}: indicator {indicator:?} is not declared by {}",
                at(),
                ds.id
            );
            // geography
            let (gsrc, g) = geo;
            let geo_code = if g.root {
                root.clone()
            } else if let Some(code) = &g.code {
                code.clone()
            } else {
                let Some(text) = rc.eval(gsrc, false).with_context(at)? else {
                    continue;
                };
                if g.skip.iter().any(|s| norm(s) == norm(&text)) {
                    *counts.skipped_names.entry(text).or_default() += 1;
                    continue;
                }
                if g.root_names.iter().any(|s| norm(s) == norm(&text)) {
                    root.clone()
                } else {
                    match g.matching.unwrap_or(GeoMatch::Code) {
                        GeoMatch::Code => text,
                        GeoMatch::Name => {
                            let level = g.level.context("geo match: name needs `level`")?;
                            match names.resolve(level, &text) {
                                Some((code, _)) => code,
                                None => {
                                    counts.unmatched.insert(text);
                                    continue;
                                }
                            }
                        }
                    }
                }
            };
            let Some(ptext) = rc.eval(period_src, false).with_context(at)? else {
                continue;
            };
            let p = o.period.as_ref().context("period")?;
            let period = period_code(&ptext, p, *freq, &ctx.profile.calendars).with_context(at)?;
            let mut pairs: Vec<(String, String)> = Vec::new();
            let mut dropped = false;
            for (k, s) in &c.dims {
                match rc.eval(s, false).with_context(at)? {
                    Some(v) => pairs.push((k.clone(), v)),
                    None => {
                        dropped = true;
                        break;
                    }
                }
            }
            if dropped {
                continue;
            }
            let refs: Vec<(&str, &str)> = pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let bound = |s: &Option<CSrc>, spec: &Option<ValueOut>| -> Result<Option<f64>> {
                match s {
                    None => Ok(None),
                    Some(s) => match rc.eval(s, true)? {
                        None => Ok(None),
                        Some(t) => parse_number(
                            &t,
                            spec.as_ref().and_then(|v| v.number.as_ref()).or(vspec),
                        ),
                    },
                }
            };
            let lo = bound(&c.lo, &o.lo).with_context(at)?;
            let hi = bound(&c.hi, &o.hi).with_context(at)?;
            let (basis, status) = match (&c.status, &o.status) {
                (Some(s), Some(spec)) => match rc.eval(s, false)? {
                    Some(code) => spec.codes.get(&code).copied().unwrap_or(spec.default),
                    None => spec.default,
                },
                _ => (Basis::Reported, Status::Normal),
            };
            let dims = crate::connector::dims_key(&refs);
            let key = (
                indicator.clone(),
                geo_code.clone(),
                period.clone(),
                dims.clone(),
            );
            if let Some(prev) = counts.keys.insert(key, at()) {
                bail!(
                    "duplicate key {indicator} {geo_code} {period} {dims:?}: {} and {prev} (header typo? use `rename`)",
                    at()
                );
            }
            counts.rows += 1;
            sink.obs(ObsRow {
                indicator_id: indicator,
                geo_code,
                period,
                dims,
                value: Some(value),
                lo,
                hi,
                basis,
                status,
                flags: Flags::EMPTY,
                artifact: vcell.map_or(0, |c| c.artifact as usize),
                prov: vcell.map(|c| CellProv {
                    extract: c.extract,
                    row: c.row,
                }),
                locator: locator(t, r, vcell),
                verbatim: Some(vcell.map_or(vtext, |c| c.text.trim().to_owned())),
                method,
                confidence: Some(1.0),
            })?;
        }
    }
    if !counts.unmatched.is_empty() {
        bail!(
            "{} unmatched geography name(s) in table {} (add them to `skip`, `root_names` or registry/crosswalks/ds-name-aliases.csv): {}",
            counts.unmatched.len(),
            t.id,
            counts
                .unmatched
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

fn map_features(
    registry: &Registry,
    ds: &crate::registry::DatasetSpec,
    o: &OutputSpec,
    t: &Table,
    tables: &HashMap<String, Table>,
    sink: &mut dyn OutputSink,
    counts: &mut Counts,
) -> Result<()> {
    let _ = registry;
    let c = COut::new(o, t)?;
    let f = o.feature.as_ref().context("feature")?;
    let key = CSrc::new(f.key.full())?;
    let kind = CSrc::new(f.kind.full())?;
    let name = f.name.as_ref().map(|n| CSrc::new(n.full())).transpose()?;
    let lon = CSrc::new(f.lon.full())?;
    let lat = CSrc::new(f.lat.full())?;
    let fields: BTreeMap<&str, &FieldSpec> =
        ds.fields.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut attrs: Vec<(String, &FieldSpec, CSrc)> = Vec::new();
    for (k, s) in &f.attrs {
        let fs = fields
            .get(k.as_str())
            .with_context(|| format!("attribute {k:?} is not a declared field of {}", ds.id))?;
        attrs.push((k.clone(), fs, CSrc::new(s.full())?));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for r in 0..t.rows.len() {
        if !c.keep(t, r) {
            continue;
        }
        let rc = RowCtx {
            t,
            r,
            melt: None,
            tables,
        };
        let at = || format!("table {} row {}", t.id, r + 1);
        let (Some(k), Some(kd), Some(x), Some(y)) = (
            rc.eval(&key, false).with_context(at)?,
            rc.eval(&kind, false).with_context(at)?,
            rc.eval(&lon, false).with_context(at)?,
            rc.eval(&lat, false).with_context(at)?,
        ) else {
            continue;
        };
        let x: f64 = x.parse().with_context(|| format!("{}: lon {x:?}", at()))?;
        let y: f64 = y.parse().with_context(|| format!("{}: lat {y:?}", at()))?;
        ensure!(x.is_finite() && y.is_finite(), "{}: non-finite point", at());
        ensure!(
            seen.insert(k.clone()),
            "{}: duplicate feature key {k:?}",
            at()
        );
        let mut map = serde_json::Map::new();
        for (n, fs, s) in &attrs {
            if let Some(v) = rc.eval(s, false).with_context(at)?
                && let Some(tv) = crate::drivers::feature_rules::typed(fs, &v)
            {
                map.insert(n.clone(), tv);
            }
        }
        let g = crate::geom::Geometry::Point([x, y]);
        let kcell = rc.source_cell(&key)?;
        counts.rows += 1;
        sink.feature(FeatureIn {
            key: k,
            kind: kd,
            name_en: name
                .as_ref()
                .map(|n| rc.eval(n, false))
                .transpose()?
                .flatten(),
            name_si: None,
            name_ta: None,
            geometry: g.to_geojson6(),
            rep: [crate::geom::round6(x), crate::geom::round6(y)],
            attrs: map,
            flags: Flags::EMPTY,
            entity_id: None,
            artifact: kcell.map_or(0, |c| c.artifact as usize),
            prov: kcell.map(|c| CellProv {
                extract: c.extract,
                row: c.row,
            }),
        })?;
    }
    Ok(())
}

/// Shared handle so drivers can keep a parsed spec behind an `Arc`.
pub type SharedStd = Arc<StdSpec>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_dates() {
        let n = NumberSpec {
            thousands: Some(",".into()),
            decimal: None,
            missing: vec!["-".into(), "n.a.".into()],
            scale: None,
        };
        assert_eq!(parse_number("1,234.5", Some(&n)).expect("ok"), Some(1234.5));
        assert_eq!(parse_number(" - ", Some(&n)).expect("ok"), None);
        assert!(parse_number("abc", Some(&n)).is_err());
        assert_eq!(
            parse_date("1986 January", "%Y %B").map(|d| d.to_string()),
            Some("1986-01-01".into())
        );
        let cal = lanka_core::Calendars {
            fiscal_start_month: 1,
            seasons: vec![("Maha".into(), 9, 1), ("Yala".into(), 4, 1)],
        };
        let p = PeriodOut {
            freq: "M".into(),
            parse: Some("%Y %B".into()),
            src: SrcFull::default(),
        };
        assert_eq!(
            period_code("2024 March", &p, Freq::M, &cal).expect("ok"),
            "2024-03"
        );
        let s = PeriodOut {
            freq: "S".into(),
            parse: None,
            src: SrcFull::default(),
        };
        assert_eq!(
            period_code("1978-Maha", &s, Freq::S, &cal).expect("ok"),
            "1978-Maha"
        );
        assert_eq!(norm("Nuwara-Eliya"), norm("NUWARA ELIYA"));
        assert_eq!(strip_suffix("Colombo District"), "Colombo");
    }
}
