//! Declarative expectations (03 §5.2): `expect:` in
//! `registry/datasets/<id>.yaml` and in `registry/connectors/<source>.yaml`
//! (there optionally scoped with `dataset:`). Each entry is one check plus
//! `severity: error | warn` (default: error for class A, which nobody
//! reviews; warn otherwise) and an optional `note`.
//!
//! | check | evaluated |
//! |---|---|
//! | `row_count {min, max}`, `row_count_change {pct}` | on the delivered rows, vs the published version |
//! | `completeness {indicator, level, min_units}` | per period, units of `level` present |
//! | `range {indicator, min, max}` | every value |
//! | `sum_to_parent {indicator, tol_pct}` | children (from the unit hierarchy) vs parent, when all children are present |
//! | `sum_dims {indicator, dim, tol, tol_pct}` | breakdown members vs the codelist's total member |
//! | `ratio_consistency {indicator, numerator, denominator, scale, tol_pct}` | value vs numerator / denominator × scale |
//! | `monotonic {indicator, direction}` | each series ordered by period |
//! | `max_step_change {indicator, pct}` | consecutive periods of each series |
//! | `revision_limit {pct_changed, pct_removed}` | the class-A gate thresholds (staging, vs the released version) |
//! | `no_future`, `period_continuity {indicator}` | period starts after today (forecasts exempt); gaps in a series |
//! | `geometry {valid, within_bbox, nesting_tol, overlap_tol}` | features and admin units |
//! | `sql {query, expect: empty}` | at staging, against the temp view `stage_obs`, read-only, 10 s |
//!
//! `indicator` is optional where it scopes a check (absent = every
//! indicator of the dataset). Results are stored in the version's
//! `validation` summary and written to the run log. An error-severity
//! failure fails a class-A run before staging; for B/C it sends the version
//! to review with the failures attached.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail, ensure};
use chrono::{Datelike, Months, NaiveDate};
use lanka_core::period::{Freq, Period};
use lanka_core::{Basis, Calendars, Dims, Profile};
use serde::{Deserialize, Serialize};

use crate::connector::{FeatureIn, Lookup, ObsRow};
use crate::registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Increasing,
    Decreasing,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Bbox {
    /// `true`: the profile's grid extent.
    Profile(bool),
    Explicit([f64; 4]),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlExpect {
    /// The query returns no rows (each row is a violation).
    #[default]
    Empty,
}

const fn half() -> f64 {
    0.5
}

const fn unit() -> f64 {
    1.0
}

/// One declarative check (the `check:` tag of an `expect:` entry).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    RowCount {
        #[serde(default)]
        min: Option<u64>,
        #[serde(default)]
        max: Option<u64>,
    },
    RowCountChange {
        pct: f64,
    },
    Completeness {
        #[serde(default)]
        indicator: Option<String>,
        level: i16,
        min_units: usize,
    },
    Range {
        #[serde(default)]
        indicator: Option<String>,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    SumToParent {
        #[serde(default)]
        indicator: Option<String>,
        #[serde(default = "half")]
        tol_pct: f64,
    },
    SumDims {
        #[serde(default)]
        indicator: Option<String>,
        dim: String,
        #[serde(default)]
        tol: f64,
        #[serde(default)]
        tol_pct: f64,
    },
    RatioConsistency {
        indicator: String,
        #[serde(default)]
        numerator: Option<String>,
        #[serde(default)]
        denominator: Option<String>,
        #[serde(default = "unit")]
        scale: f64,
        #[serde(default = "half")]
        tol_pct: f64,
    },
    Monotonic {
        #[serde(default)]
        indicator: Option<String>,
        direction: Direction,
    },
    MaxStepChange {
        #[serde(default)]
        indicator: Option<String>,
        pct: f64,
    },
    RevisionLimit {
        pct_changed: f64,
        pct_removed: f64,
    },
    NoFuture {},
    PeriodContinuity {
        #[serde(default)]
        indicator: Option<String>,
    },
    Geometry {
        #[serde(default)]
        valid: bool,
        #[serde(default)]
        within_bbox: Option<Bbox>,
        /// Max share of units whose representative point is outside their parent.
        #[serde(default)]
        nesting_tol: Option<f64>,
        /// Max share of polygons whose representative point falls in another
        /// polygon of the same level.
        #[serde(default)]
        overlap_tol: Option<f64>,
    },
    Sql {
        query: String,
        #[serde(default)]
        expect: SqlExpect,
    },
}

impl Check {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::RowCount { .. } => "row_count",
            Self::RowCountChange { .. } => "row_count_change",
            Self::Completeness { .. } => "completeness",
            Self::Range { .. } => "range",
            Self::SumToParent { .. } => "sum_to_parent",
            Self::SumDims { .. } => "sum_dims",
            Self::RatioConsistency { .. } => "ratio_consistency",
            Self::Monotonic { .. } => "monotonic",
            Self::MaxStepChange { .. } => "max_step_change",
            Self::RevisionLimit { .. } => "revision_limit",
            Self::NoFuture {} => "no_future",
            Self::PeriodContinuity { .. } => "period_continuity",
            Self::Geometry { .. } => "geometry",
            Self::Sql { .. } => "sql",
        }
    }
}

/// An `expect:` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    pub check: Check,
    pub severity: Option<Severity>,
    /// Connector-level entries: the dataset they apply to (default: all).
    pub dataset: Option<String>,
    pub note: Option<String>,
}

impl Expectation {
    /// Parses one entry (`check` + parameters + `severity`/`dataset`/`note`).
    pub fn parse(v: &serde_json::Value) -> Result<Self> {
        let mut m = v
            .as_object()
            .context("an expectation is a mapping with `check:`")?
            .clone();
        let severity = m
            .remove("severity")
            .map(serde_json::from_value)
            .transpose()
            .context("severity: error | warn")?;
        let dataset = m
            .remove("dataset")
            .map(|d| d.as_str().map(str::to_owned).context("dataset: a string"))
            .transpose()?;
        let note = m
            .remove("note")
            .map(|d| d.as_str().map(str::to_owned).context("note: a string"))
            .transpose()?;
        let check: Check = serde_json::from_value(serde_json::Value::Object(m))
            .with_context(|| format!("invalid expectation {v}"))?;
        if let Check::Sql { query, .. } = &check {
            ensure!(
                query
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("select")
                    || query.trim_start().to_ascii_lowercase().starts_with("with"),
                "sql expectation must be a SELECT"
            );
        }
        Ok(Self {
            check,
            severity,
            dataset,
            note,
        })
    }

    /// Severity with the class default (error for class A).
    #[must_use]
    pub fn severity_for(&self, class: &str) -> Severity {
        self.severity.unwrap_or(if class == "A" {
            Severity::Error
        } else {
            Severity::Warn
        })
    }
}

/// Parses a list of entries.
pub fn parse_all(list: &[serde_json::Value]) -> Result<Vec<Expectation>> {
    list.iter()
        .enumerate()
        .map(|(i, v)| Expectation::parse(v).with_context(|| format!("expect[{i}]")))
        .collect()
}

/// The expectations that apply to a dataset: the dataset's own, plus those of
/// the connector specs of its sources (unscoped or scoped to it).
pub fn for_dataset(registry: &Registry, dataset_id: &str) -> Result<Vec<Expectation>> {
    let mut out = Vec::new();
    if let Some(ds) = registry.dataset(dataset_id) {
        out.extend(parse_all(&ds.expect).with_context(|| format!("dataset {dataset_id}"))?);
        for s in &ds.source_ids {
            if let Some(c) = registry.connectors.get(s)
                && let Some(list) = c.json.get("expect").and_then(serde_json::Value::as_array)
            {
                for e in parse_all(list).with_context(|| format!("connector {s}"))? {
                    if e.dataset.as_deref().is_none_or(|d| d == dataset_id) {
                        out.push(e);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The outcome of one check.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckResult {
    pub check: &'static str,
    pub severity: Severity,
    pub passed: bool,
    /// Not evaluable here (no previous version, no unit lookup, …).
    pub skipped: bool,
    pub failures: usize,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl CheckResult {
    fn new(e: &Expectation, sev: Severity, failures: usize, examples: &[String], ok: &str) -> Self {
        Self {
            check: e.check.name(),
            severity: sev,
            passed: failures == 0,
            skipped: false,
            failures,
            detail: if failures == 0 {
                ok.to_owned()
            } else {
                format!(
                    "{failures} failure(s): {}",
                    examples
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            },
            note: e.note.clone(),
        }
    }

    fn skipped(e: &Expectation, sev: Severity, why: &str) -> Self {
        Self {
            check: e.check.name(),
            severity: sev,
            passed: true,
            skipped: true,
            failures: 0,
            detail: format!("skipped: {why}"),
            note: e.note.clone(),
        }
    }

    /// A failed error-severity check.
    #[must_use]
    pub fn is_error(&self) -> bool {
        !self.passed && self.severity == Severity::Error
    }

    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "{} {} [{}]: {}",
            if self.passed { "PASS" } else { "FAIL" },
            self.check,
            match self.severity {
                Severity::Error => "error",
                Severity::Warn => "warn",
            },
            self.detail
        )
    }
}

/// What observation checks may read besides the rows.
pub struct ObsCtx<'a> {
    pub registry: &'a Registry,
    pub profile: &'a Profile,
    pub lookup: &'a Lookup,
    pub class: &'a str,
    /// Rows of the currently published version, if any.
    pub prev_rows: Option<i64>,
    pub today: NaiveDate,
}

fn wants(ind: Option<&String>, id: &str) -> bool {
    ind.is_none_or(|i| i == id)
}

/// The start date of a row's period (`None` when it does not parse).
fn start(registry: &Registry, cal: &Calendars, r: &ObsRow) -> Option<(Freq, NaiveDate)> {
    let freq = registry.indicator(&r.indicator_id)?.freq()?;
    Period::parse(freq, &r.period, cal)
        .ok()
        .map(|p| (freq, p.start))
}

/// The start of the period after `(freq, s)`.
fn next_start(freq: Freq, s: NaiveDate, cal: &Calendars) -> Option<NaiveDate> {
    match freq {
        Freq::A | Freq::F => s.checked_add_months(Months::new(12)),
        Freq::H => s.checked_add_months(Months::new(6)),
        Freq::Q => s.checked_add_months(Months::new(3)),
        Freq::M => s.checked_add_months(Months::new(1)),
        Freq::W => s.checked_add_days(chrono::Days::new(7)),
        Freq::D => s.checked_add_days(chrono::Days::new(1)),
        Freq::S => {
            let mut starts: Vec<NaiveDate> = (s.year()..=s.year() + 1)
                .flat_map(|y| {
                    cal.seasons
                        .iter()
                        .filter_map(move |(_, m, d)| NaiveDate::from_ymd_opt(y, *m, *d))
                })
                .filter(|d| *d > s)
                .collect();
            starts.sort_unstable();
            starts.first().copied()
        }
        Freq::P => None,
    }
}

type SeriesKey<'a> = (&'a str, &'a str, &'a str);

/// Rows grouped per series (indicator, geo, dims), ordered by period start.
fn series<'a>(
    rows: &'a [ObsRow],
    ctx: &ObsCtx<'_>,
    ind: Option<&String>,
) -> BTreeMap<SeriesKey<'a>, Vec<(Freq, NaiveDate, &'a ObsRow)>> {
    let mut m: BTreeMap<SeriesKey<'a>, Vec<(Freq, NaiveDate, &'a ObsRow)>> = BTreeMap::new();
    for r in rows.iter().filter(|r| wants(ind, &r.indicator_id)) {
        if let Some((f, s)) = start(ctx.registry, &ctx.profile.calendars, r) {
            m.entry((
                r.indicator_id.as_str(),
                r.geo_code.as_str(),
                r.dims.as_str(),
            ))
            .or_default()
            .push((f, s, r));
        }
    }
    for v in m.values_mut() {
        v.sort_by_key(|(_, s, _)| *s);
    }
    m
}

fn rel_diff(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        if a == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        ((a - b) / b).abs() * 100.0
    }
}

/// Evaluates every observation-level expectation of a dataset (all but
/// `revision_limit`, `sql` and `geometry`, which run elsewhere).
#[allow(clippy::too_many_lines)]
pub fn check_observations(
    exps: &[Expectation],
    rows: &[ObsRow],
    ctx: &ObsCtx<'_>,
) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let levels: HashMap<&str, i16> = ctx
        .lookup
        .units
        .iter()
        .map(|u| (u.code.as_str(), u.level))
        .collect();
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for u in &ctx.lookup.units {
        if let Some(p) = &u.parent {
            children
                .entry(p.as_str())
                .or_default()
                .push(u.code.as_str());
        }
    }
    for e in exps {
        let sev = e.severity_for(ctx.class);
        let mut bad: Vec<String> = Vec::new();
        let r = match &e.check {
            Check::RowCount { min, max } => {
                let n = rows.len() as u64;
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    bad.push(format!("{n} rows, expected {min:?}..{max:?}"));
                }
                CheckResult::new(e, sev, bad.len(), &bad, &format!("{n} rows"))
            }
            Check::RowCountChange { pct } => match ctx.prev_rows {
                None | Some(0) => CheckResult::skipped(e, sev, "no published version"),
                Some(prev) => {
                    #[allow(clippy::cast_precision_loss)]
                    let d = rel_diff(rows.len() as f64, prev as f64);
                    if d > *pct {
                        bad.push(format!("{} rows vs {prev} ({d:.1}% > {pct}%)", rows.len()));
                    }
                    CheckResult::new(e, sev, bad.len(), &bad, &format!("{d:.2}% change"))
                }
            },
            Check::Completeness {
                indicator,
                level,
                min_units,
            } => {
                if levels.is_empty() {
                    CheckResult::skipped(e, sev, "no unit lookup")
                } else {
                    let mut per: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
                    for r in rows
                        .iter()
                        .filter(|r| wants(indicator.as_ref(), &r.indicator_id))
                    {
                        if levels.get(r.geo_code.as_str()) == Some(level) {
                            per.entry((&r.indicator_id, &r.period))
                                .or_default()
                                .insert(&r.geo_code);
                        }
                    }
                    if per.is_empty() {
                        bad.push(format!("no rows at level {level}"));
                    }
                    for ((i, p), units) in &per {
                        if units.len() < *min_units {
                            bad.push(format!("{i} {p}: {} of {min_units} units", units.len()));
                        }
                    }
                    CheckResult::new(
                        e,
                        sev,
                        bad.len(),
                        &bad,
                        &format!("{} period(s) complete", per.len()),
                    )
                }
            }
            Check::Range {
                indicator,
                min,
                max,
            } => {
                for r in rows
                    .iter()
                    .filter(|r| wants(indicator.as_ref(), &r.indicator_id))
                {
                    if let Some(v) = r.value
                        && (min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m))
                    {
                        bad.push(format!(
                            "{} {} {} = {v}",
                            r.indicator_id, r.geo_code, r.period
                        ));
                    }
                }
                CheckResult::new(e, sev, bad.len(), &bad, "all values in range")
            }
            Check::SumToParent { indicator, tol_pct } => {
                if children.is_empty() {
                    CheckResult::skipped(e, sev, "no unit lookup")
                } else {
                    let mut groups: BTreeMap<(&str, &str, &str), HashMap<&str, f64>> =
                        BTreeMap::new();
                    for r in rows
                        .iter()
                        .filter(|r| wants(indicator.as_ref(), &r.indicator_id))
                    {
                        if let Some(v) = r.value {
                            groups
                                .entry((&r.indicator_id, &r.period, &r.dims))
                                .or_default()
                                .insert(&r.geo_code, v);
                        }
                    }
                    let mut compared = 0;
                    for ((i, p, d), vals) in &groups {
                        for (parent, pv) in vals {
                            let Some(kids) = children.get(parent) else {
                                continue;
                            };
                            if !kids.iter().all(|k| vals.contains_key(k)) {
                                continue;
                            }
                            compared += 1;
                            let sum: f64 = kids.iter().map(|k| vals[k]).sum();
                            let diff = rel_diff(sum, *pv);
                            if diff > *tol_pct {
                                bad.push(format!(
                                    "{i} {p} {d} {parent}: children sum {sum} vs {pv} ({diff:.2}%)"
                                ));
                            }
                        }
                    }
                    CheckResult::new(
                        e,
                        sev,
                        bad.len(),
                        &bad,
                        &format!("{compared} parent(s) agree"),
                    )
                }
            }
            Check::SumDims {
                indicator,
                dim,
                tol,
                tol_pct,
            } => match sum_dims(rows, ctx.registry, indicator.as_ref(), dim, *tol, *tol_pct) {
                Ok((n, b)) => CheckResult::new(e, sev, b.len(), &b, &format!("{n} total(s) agree")),
                Err(err) => {
                    bad.push(format!("{err:#}"));
                    CheckResult::new(e, sev, 1, &bad, "")
                }
            },
            Check::RatioConsistency {
                indicator,
                numerator,
                denominator,
                scale,
                tol_pct,
            } => {
                let spec = ctx.registry.indicator(indicator);
                let num = numerator
                    .clone()
                    .or_else(|| spec.and_then(|s| s.numerator.clone()));
                let den = denominator
                    .clone()
                    .or_else(|| spec.and_then(|s| s.denominator.clone()));
                match (num, den) {
                    (Some(num), Some(den)) => {
                        let key =
                            |r: &ObsRow| (r.geo_code.clone(), r.period.clone(), r.dims.clone());
                        let of = |id: &str| -> HashMap<(String, String, String), f64> {
                            rows.iter()
                                .filter(|r| r.indicator_id == id)
                                .filter_map(|r| r.value.map(|v| (key(r), v)))
                                .collect()
                        };
                        let (n, d) = (of(&num), of(&den));
                        let mut compared = 0;
                        for r in rows.iter().filter(|r| &r.indicator_id == indicator) {
                            let (Some(v), Some(a), Some(b)) =
                                (r.value, n.get(&key(r)), d.get(&key(r)))
                            else {
                                continue;
                            };
                            if *b == 0.0 {
                                continue;
                            }
                            compared += 1;
                            let want = a / b * scale;
                            let diff = rel_diff(v, want);
                            if diff > *tol_pct {
                                bad.push(format!(
                                    "{} {}: {v} vs {a}/{b}×{scale} = {want:.4} ({diff:.2}%)",
                                    r.geo_code, r.period
                                ));
                            }
                        }
                        CheckResult::new(
                            e,
                            sev,
                            bad.len(),
                            &bad,
                            &format!("{compared} ratio(s) agree"),
                        )
                    }
                    _ => {
                        bad.push(format!("{indicator}: no numerator/denominator declared"));
                        CheckResult::new(e, sev, 1, &bad, "")
                    }
                }
            }
            Check::Monotonic {
                indicator,
                direction,
            } => {
                for ((i, g, d), s) in series(rows, ctx, indicator.as_ref()) {
                    for w in s.windows(2) {
                        let (Some(a), Some(b)) = (w[0].2.value, w[1].2.value) else {
                            continue;
                        };
                        let ok = match direction {
                            Direction::Increasing => b >= a,
                            Direction::Decreasing => b <= a,
                        };
                        if !ok {
                            bad.push(format!(
                                "{i} {g} {d}: {} {a} → {} {b}",
                                w[0].2.period, w[1].2.period
                            ));
                        }
                    }
                }
                CheckResult::new(e, sev, bad.len(), &bad, "all series monotonic")
            }
            Check::MaxStepChange { indicator, pct } => {
                for ((i, g, d), s) in series(rows, ctx, indicator.as_ref()) {
                    for w in s.windows(2) {
                        let (Some(a), Some(b)) = (w[0].2.value, w[1].2.value) else {
                            continue;
                        };
                        if a == 0.0 {
                            continue;
                        }
                        let diff = rel_diff(b, a);
                        if diff > *pct {
                            bad.push(format!(
                                "{i} {g} {d}: {} {a} → {} {b} ({diff:.1}%)",
                                w[0].2.period, w[1].2.period
                            ));
                        }
                    }
                }
                CheckResult::new(e, sev, bad.len(), &bad, "no step above the limit")
            }
            Check::NoFuture {} => {
                for r in rows {
                    if r.basis == Basis::Forecast {
                        continue;
                    }
                    if let Some((_, s)) = start(ctx.registry, &ctx.profile.calendars, r)
                        && s > ctx.today
                    {
                        bad.push(format!("{} {} {}", r.indicator_id, r.geo_code, r.period));
                    }
                }
                CheckResult::new(e, sev, bad.len(), &bad, "no future periods")
            }
            Check::PeriodContinuity { indicator } => {
                for ((i, g, d), s) in series(rows, ctx, indicator.as_ref()) {
                    for w in s.windows(2) {
                        let (f, a, _) = w[0];
                        if let Some(n) = next_start(f, a, &ctx.profile.calendars)
                            && n < w[1].1
                        {
                            bad.push(format!("{i} {g} {d}: gap after {}", w[0].2.period));
                        }
                    }
                }
                CheckResult::new(e, sev, bad.len(), &bad, "no gaps")
            }
            Check::RevisionLimit { .. } | Check::Sql { .. } | Check::Geometry { .. } => continue,
        };
        out.push(r);
    }
    out
}

fn sum_dims(
    rows: &[ObsRow],
    registry: &Registry,
    ind: Option<&String>,
    dim: &str,
    tol: f64,
    tol_pct: f64,
) -> Result<(usize, Vec<String>)> {
    let mut bad = Vec::new();
    let mut compared = 0;
    // indicator → (total code, member codes)
    let mut members: HashMap<&str, (String, BTreeSet<String>)> = HashMap::new();
    let mut groups: BTreeMap<(String, String, String, String), HashMap<String, f64>> =
        BTreeMap::new();
    for r in rows.iter().filter(|r| wants(ind, &r.indicator_id)) {
        let Some(v) = r.value else { continue };
        let Ok(d) = Dims::parse(&r.dims) else {
            continue;
        };
        let Some(code) = d.iter().find(|(k, _)| *k == dim).map(|(_, v)| v.to_owned()) else {
            continue;
        };
        if !members.contains_key(r.indicator_id.as_str()) {
            let spec = registry
                .indicator(&r.indicator_id)
                .with_context(|| format!("unknown indicator {}", r.indicator_id))?;
            let ds = spec
                .dims
                .iter()
                .find(|x| x.id() == dim)
                .with_context(|| format!("{} has no dimension {dim}", r.indicator_id))?;
            let cl = registry
                .codelists
                .get(ds.codelist())
                .with_context(|| format!("no codelist {}", ds.codelist()))?;
            let total = cl
                .spec
                .codes
                .iter()
                .find(|c| c.total)
                .with_context(|| format!("codelist {} has no total member", cl.spec.id))?
                .code
                .clone();
            let direct: BTreeSet<String> = cl
                .spec
                .codes
                .iter()
                .filter(|c| c.parent.as_deref() == Some(total.as_str()))
                .map(|c| c.code.clone())
                .collect();
            let kids = if direct.is_empty() {
                cl.spec
                    .codes
                    .iter()
                    .filter(|c| !c.total)
                    .map(|c| c.code.clone())
                    .collect()
            } else {
                direct
            };
            members.insert(&r.indicator_id, (total, kids));
        }
        let rest: Vec<String> = d
            .iter()
            .filter(|(k, _)| *k != dim)
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        groups
            .entry((
                r.indicator_id.clone(),
                r.geo_code.clone(),
                r.period.clone(),
                rest.join(";"),
            ))
            .or_default()
            .insert(code, v);
    }
    for ((i, g, p, rest), vals) in &groups {
        let (total, kids) = &members[i.as_str()];
        let Some(t) = vals.get(total) else { continue };
        let present: Vec<f64> = kids.iter().filter_map(|k| vals.get(k).copied()).collect();
        if present.is_empty() {
            continue;
        }
        compared += 1;
        let sum: f64 = present.iter().sum();
        let abs = (sum - t).abs();
        let ok = abs <= tol || (tol_pct > 0.0 && rel_diff(sum, *t) <= tol_pct);
        if !ok {
            bad.push(format!(
                "{i} {g} {p} {rest}: members sum {sum} vs {total} {t}"
            ));
        }
    }
    Ok((compared, bad))
}

/// The class-A gate thresholds (`revision_limit`), as shares: the tightest
/// declared, else the defaults of `publish`.
#[must_use]
pub fn revision_limits(exps: &[Expectation], default: (f64, f64)) -> (f64, f64) {
    exps.iter().fold(default, |(c, r), e| match e.check {
        Check::RevisionLimit {
            pct_changed,
            pct_removed,
        } => (pct_changed / 100.0, pct_removed / 100.0),
        _ => (c, r),
    })
}

/// `revision_limit` as a result, from the staging diff.
#[must_use]
pub fn check_revision(
    e: &Expectation,
    class: &str,
    prev_rows: i64,
    changed: i64,
    removed: i64,
) -> Option<CheckResult> {
    let Check::RevisionLimit {
        pct_changed,
        pct_removed,
    } = e.check
    else {
        return None;
    };
    let sev = e.severity_for(class);
    if prev_rows <= 0 {
        return Some(CheckResult::skipped(e, sev, "no released version"));
    }
    #[allow(clippy::cast_precision_loss)]
    let (c, r) = (
        changed as f64 / prev_rows as f64 * 100.0,
        removed as f64 / prev_rows as f64 * 100.0,
    );
    let mut bad = Vec::new();
    if c > pct_changed {
        bad.push(format!(
            "{changed} values changed ({c:.2}% > {pct_changed}%)"
        ));
    }
    if r > pct_removed {
        bad.push(format!("{removed} rows removed ({r:.2}% > {pct_removed}%)"));
    }
    Some(CheckResult::new(
        e,
        sev,
        bad.len(),
        &bad,
        &format!("{c:.2}% changed, {r:.2}% removed"),
    ))
}

/// A geometry to check: code, level/parent (admin units) and GeoJSON.
pub struct GeomItem<'a> {
    pub key: &'a str,
    pub level: Option<i16>,
    pub parent: Option<&'a str>,
    pub geojson: &'a str,
}

/// The profile grid's extent `[lon_min, lat_min, lon_max, lat_max]`.
#[must_use]
pub fn profile_bbox(p: &Profile) -> [f64; 4] {
    let g = &p.spec.grid;
    #[allow(clippy::cast_precision_loss)]
    let (h, w) = (g.rows as f64 * g.step_deg, g.cols as f64 * g.step_deg);
    [g.lon_min, g.lat_max - h, g.lon_min + w, g.lat_max]
}

fn polygons(g: &crate::geom::Geometry) -> Vec<&Vec<Vec<crate::geom::Coord>>> {
    use crate::geom::Geometry;
    match g {
        Geometry::Polygon(r) => vec![r],
        Geometry::MultiPolygon(ps) => ps.iter().collect(),
        _ => Vec::new(),
    }
}

fn contains(g: &crate::geom::Geometry, p: crate::geom::Coord) -> bool {
    polygons(g)
        .iter()
        .any(|rings| crate::geom::point_in_rings(p, rings))
}

fn invalid(g: &crate::geom::Geometry) -> Option<&'static str> {
    use crate::geom::Geometry;
    let ring_ok = |r: &Vec<crate::geom::Coord>| r.len() >= 4 && r.first() == r.last();
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => None,
        Geometry::LineString(l) => (l.len() < 2).then_some("line with < 2 points"),
        Geometry::MultiLineString(ls) => ls
            .iter()
            .any(|l| l.len() < 2)
            .then_some("line with < 2 points"),
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => {
            if polygons(g)
                .iter()
                .any(|p| p.is_empty() || !p.iter().all(ring_ok))
            {
                Some("ring not closed or < 4 points")
            } else if g.area_deg2() <= 0.0 {
                Some("zero area")
            } else {
                None
            }
        }
    }
}

/// `geometry` expectations over features or admin units, and `row_count`
/// on the delivered features (a features dataset has no observations, so
/// `check_observations` never sees it).
pub fn check_geometry(
    exps: &[Expectation],
    items: &[GeomItem<'_>],
    profile: &Profile,
    class: &str,
) -> Vec<CheckResult> {
    let mut out = Vec::new();
    for e in exps {
        if let Check::RowCount { min, max } = &e.check {
            let n = items.len() as u64;
            let bad: Vec<String> = if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                vec![format!("{n} features, expected {min:?}..{max:?}")]
            } else {
                Vec::new()
            };
            out.push(CheckResult::new(
                e,
                e.severity_for(class),
                bad.len(),
                &bad,
                &format!("{n} features"),
            ));
            continue;
        }
        let Check::Geometry {
            valid,
            within_bbox,
            nesting_tol,
            overlap_tol,
        } = &e.check
        else {
            continue;
        };
        let sev = e.severity_for(class);
        let mut bad = Vec::new();
        let parsed: Vec<Option<crate::geom::Geometry>> = items
            .iter()
            .map(|i| crate::geom::Geometry::parse(i.geojson).ok())
            .collect();
        if *valid {
            for (i, g) in items.iter().zip(&parsed) {
                match g {
                    None => bad.push(format!("{}: unparseable geometry", i.key)),
                    Some(g) => {
                        if let Some(why) = invalid(g) {
                            bad.push(format!("{}: {why}", i.key));
                        }
                    }
                }
            }
        }
        if let Some(b) = within_bbox {
            let bb = match b {
                Bbox::Profile(true) => Some(profile_bbox(profile)),
                Bbox::Profile(false) => None,
                Bbox::Explicit(x) => Some(*x),
            };
            if let Some([x0, y0, x1, y1]) = bb {
                for (i, g) in items.iter().zip(&parsed) {
                    if let Some(g) = g {
                        let [a, b, c, d] = g.bbox();
                        if a < x0 || b < y0 || c > x1 || d > y1 {
                            bad.push(format!("{}: outside {:?}", i.key, [x0, y0, x1, y1]));
                        }
                    }
                }
            }
        }
        let by_key: HashMap<&str, usize> =
            items.iter().enumerate().map(|(n, i)| (i.key, n)).collect();
        if let Some(tol) = nesting_tol {
            let (mut n, mut outside) = (0usize, Vec::new());
            for (i, g) in items.iter().zip(&parsed) {
                let (Some(p), Some(g)) = (i.parent, g) else {
                    continue;
                };
                let Some(pg) = by_key.get(p).and_then(|k| parsed[*k].as_ref()) else {
                    continue;
                };
                n += 1;
                if let Some(rp) = g.rep_point()
                    && !contains(pg, rp)
                {
                    outside.push(format!("{} not inside {p}", i.key));
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let share = if n == 0 {
                0.0
            } else {
                outside.len() as f64 / n as f64
            };
            if share > *tol {
                bad.push(format!(
                    "{:.2}% of units outside their parent: {}",
                    share * 100.0,
                    outside
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        if let Some(tol) = overlap_tol {
            let mut by_level: BTreeMap<Option<i16>, Vec<usize>> = BTreeMap::new();
            for (n, g) in parsed.iter().enumerate() {
                if g.as_ref().is_some_and(|g| !polygons(g).is_empty()) {
                    by_level.entry(items[n].level).or_default().push(n);
                }
            }
            let (mut n, mut over) = (0usize, Vec::new());
            for idx in by_level.values() {
                let boxes: Vec<[f64; 4]> = idx
                    .iter()
                    .map(|k| {
                        parsed[*k]
                            .as_ref()
                            .map_or([0.0; 4], crate::geom::Geometry::bbox)
                    })
                    .collect();
                for (a, ka) in idx.iter().enumerate() {
                    n += 1;
                    let Some(rp) = parsed[*ka]
                        .as_ref()
                        .and_then(crate::geom::Geometry::rep_point)
                    else {
                        continue;
                    };
                    for (b, kb) in idx.iter().enumerate() {
                        let bb = boxes[b];
                        if a == b
                            || rp[0] < bb[0]
                            || rp[0] > bb[2]
                            || rp[1] < bb[1]
                            || rp[1] > bb[3]
                        {
                            continue;
                        }
                        if parsed[*kb].as_ref().is_some_and(|g| contains(g, rp)) {
                            over.push(format!("{} overlaps {}", items[*ka].key, items[*kb].key));
                            break;
                        }
                    }
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let share = if n == 0 {
                0.0
            } else {
                over.len() as f64 / n as f64
            };
            if share > *tol {
                bad.push(format!(
                    "{:.2}% of polygons overlap another: {}",
                    share * 100.0,
                    over.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        out.push(CheckResult::new(
            e,
            sev,
            bad.len(),
            &bad,
            &format!("{} geometries ok", items.len()),
        ));
    }
    out
}

/// Geometry items of features.
#[must_use]
pub fn feature_items(rows: &[FeatureIn]) -> Vec<GeomItem<'_>> {
    rows.iter()
        .map(|f| GeomItem {
            key: &f.key,
            level: None,
            parent: None,
            geojson: &f.geometry,
        })
        .collect()
}

/// `sql` expectations, run at staging against the temp view `stage_obs`
/// (`indicator_id, geo_code, period_start, freq, dims, value, lo, hi`):
/// inside a savepoint that is always rolled back, read-only, 10 s timeout.
/// Each returned row is a violation.
pub async fn run_sql(
    conn: &mut sqlx::PgConnection,
    e: &Expectation,
    class: &str,
) -> Result<Option<CheckResult>> {
    let Check::Sql { query, .. } = &e.check else {
        return Ok(None);
    };
    let sev = e.severity_for(class);
    sqlx::query("SAVEPOINT lanka_expect")
        .execute(&mut *conn)
        .await?;
    let res: Result<Vec<String>> = async {
        sqlx::query(
            "CREATE TEMP VIEW stage_obs AS
             SELECT i.id AS indicator_id, u.code AS geo_code, s.time_start AS period_start,
                    s.freq::text AS freq, ds.dims, s.value, s.lo, s.hi
             FROM obs_stage s
             JOIN catalog.indicators i ON i.key = s.indicator_key
             JOIN geo.units u ON u.key = s.geo_key
             JOIN catalog.dim_sets ds ON ds.key = s.dims_key",
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query("SET LOCAL transaction_read_only = on")
            .execute(&mut *conn)
            .await?;
        sqlx::query("SET LOCAL statement_timeout = '10s'")
            .execute(&mut *conn)
            .await?;
        let q = format!(
            "SELECT row_to_json(q)::text FROM ({}) q LIMIT 20",
            query.trim().trim_end_matches(';')
        );
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(q))
            .fetch_all(&mut *conn)
            .await?;
        Ok(rows)
    }
    .await;
    sqlx::query("ROLLBACK TO SAVEPOINT lanka_expect")
        .execute(&mut *conn)
        .await?;
    sqlx::query("RELEASE SAVEPOINT lanka_expect")
        .execute(&mut *conn)
        .await?;
    Ok(Some(match res {
        Ok(rows) => CheckResult::new(e, sev, rows.len(), &rows, "query returned no rows"),
        Err(err) => CheckResult::new(e, sev, 1, &[format!("query failed: {err:#}")], ""),
    }))
}

/// Fails when any result is an error-severity failure.
pub fn ensure_passed(dataset_id: &str, results: &[CheckResult]) -> Result<()> {
    let errors: Vec<String> = results
        .iter()
        .filter(|r| r.is_error())
        .map(CheckResult::line)
        .collect();
    if !errors.is_empty() {
        bail!(
            "expectations failed for {dataset_id}:\n  - {}",
            errors.join("\n  - ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
