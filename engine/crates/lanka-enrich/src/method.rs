//! The method contract (04 §2).
//!
//! A [`Method`] is a versioned, deterministic algorithm with declared ports,
//! a JSON-Schema parameter contract, an output basis floor, an uncertainty
//! kind and mandatory flags. It reads only through [`Inputs`] (pinned input
//! versions, or in-memory tables when used as a library by `lanka-analysis`)
//! and writes only through an [`OutputSink`]. Every output row lists the
//! input rows it was computed from ([`Contrib`]); the runner uses that list
//! to enforce basis/status/flag propagation (04 §5) independently of what
//! the method claims, and `explain` turns it into contributors (04 §6).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use lanka_core::{Basis, Calendars, Flags, Freq, Period, Status};
use serde::Serialize;

// ───────────────────────────── versions ────────────────────────────────────

/// `MAJOR.MINOR.PATCH`. MAJOR changes when outputs change for the same inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let mut it = s.split('.');
        let mut next = || -> Result<u32> {
            Ok(match it.next() {
                Some(p) => p
                    .parse()
                    .with_context(|| format!("version {s:?}: {p:?} is not a number"))?,
                None => 0,
            })
        };
        let v = Self::new(next()?, next()?, next()?);
        if s.split('.').count() > 3 || s.is_empty() {
            bail!("version {s:?} is not MAJOR[.MINOR[.PATCH]]");
        }
        Ok(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A version requirement: `^1`, `^1.2`, `=1.2.3`, `1` (= `^1`), `*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionReq {
    Any,
    Caret(Version),
    Exact(Version),
}

impl VersionReq {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        Ok(if s == "*" {
            Self::Any
        } else if let Some(v) = s.strip_prefix('=') {
            Self::Exact(Version::parse(v)?)
        } else {
            Self::Caret(Version::parse(s.strip_prefix('^').unwrap_or(s))?)
        })
    }

    #[must_use]
    pub fn matches(&self, v: Version) -> bool {
        match *self {
            Self::Any => true,
            Self::Exact(e) => e == v,
            Self::Caret(c) => v.major == c.major && v >= c,
        }
    }
}

/// `rollup@^1` → (`rollup`, `^1`); a bare id means any version.
pub fn parse_method_ref(s: &str) -> Result<(&str, VersionReq)> {
    match s.split_once('@') {
        Some((id, req)) => Ok((id, VersionReq::parse(req)?)),
        None => Ok((s, VersionReq::Any)),
    }
}

// ───────────────────────────── info ────────────────────────────────────────

/// What a port carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    Indicators,
    Grid,
    Features,
    Boundaries,
    Crosswalk,
}

impl PortKind {
    /// The `catalog.datasets.kind` a dataset bound to this port has.
    #[must_use]
    pub const fn dataset_kind(self) -> &'static str {
        match self {
            Self::Indicators => "indicators",
            Self::Grid => "grid",
            Self::Features => "features",
            Self::Boundaries => "boundaries",
            Self::Crosswalk => "crosswalk",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PortSpec {
    /// Role name, e.g. `input`, `terms`, `features`.
    pub name: &'static str,
    pub kind: PortKind,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyKind {
    /// Computed values: no interval.
    None,
    Interval,
    QualityClass,
    ModelSpread,
}

/// Static description of a method (mirrored in `catalog.methods.info`).
#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub id: &'static str,
    pub version: Version,
    pub summary: &'static str,
    pub citation: &'static str,
    pub inputs: &'static [PortSpec],
    /// JSON Schema (draft 2020-12) of the `with:` parameters.
    pub params: &'static str,
    /// The *minimum* basis of every output row (04 §5).
    pub output_basis: Basis,
    pub uncertainty: UncertaintyKind,
    /// Flags every output row carries.
    pub mandatory_flags: Flags,
    /// External tools whose versions enter the fingerprint.
    pub tools: &'static [&'static str],
    /// Accepts a `changed_keys` hint and recomputes only those keys.
    pub partial_recompute: bool,
}

impl MethodInfo {
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// `catalog.methods.info`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "version": self.version.to_string(),
            "summary": self.summary,
            "citation": self.citation,
            "inputs": self.inputs,
            "params": serde_json::from_str::<serde_json::Value>(self.params).unwrap_or_default(),
            "output_basis": self.output_basis.as_str(),
            "uncertainty": self.uncertainty,
            "mandatory_flags": self.mandatory_flags.to_string(),
            "tools": self.tools,
            "partial_recompute": self.partial_recompute,
        })
    }
}

// ───────────────────────────── rows ────────────────────────────────────────

/// One observation as methods see it: codes, not keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub indicator: String,
    /// p-code or `scheme:code`.
    pub geo: String,
    pub level: i16,
    pub freq: Freq,
    pub start: NaiveDate,
    /// Canonical `k=v;k=v`, empty when undimensioned.
    pub dims: String,
    pub value: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub basis: Basis,
    pub status: Status,
    pub flags: Flags,
    /// Set for rows read from a published dataset version.
    pub obs_id: Option<i64>,
}

impl Row {
    /// A computed, normal row with no interval (the common output).
    #[must_use]
    pub fn computed(
        indicator: impl Into<String>,
        geo: impl Into<String>,
        level: i16,
        freq: Freq,
        start: NaiveDate,
        dims: impl Into<String>,
        value: Option<f64>,
    ) -> Self {
        Self {
            indicator: indicator.into(),
            geo: geo.into(),
            level,
            freq,
            start,
            dims: dims.into(),
            value,
            lo: None,
            hi: None,
            basis: Basis::Computed,
            status: Status::Normal,
            flags: Flags::EMPTY,
            obs_id: None,
        }
    }

    #[must_use]
    pub fn key(&self) -> RowKey {
        RowKey {
            indicator: self.indicator.clone(),
            geo: self.geo.clone(),
            freq: self.freq,
            start: self.start,
            dims: self.dims.clone(),
        }
    }

    #[must_use]
    pub fn period(&self, cal: &Calendars) -> String {
        Period {
            freq: self.freq,
            start: self.start,
        }
        .render(cal)
    }
}

/// Identity of an observation inside a run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct RowKey {
    pub indicator: String,
    pub geo: String,
    #[serde(serialize_with = "ser_freq")]
    pub freq: Freq,
    pub start: NaiveDate,
    pub dims: String,
}

fn ser_freq<S: serde::Serializer>(f: &Freq, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.collect_str(f)
}

/// One feature of a features dataset (05 §4 shape, WP-04).
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub key: String,
    pub kind: String,
    /// Deepest admin unit (by point-on-surface for lines and polygons).
    pub unit: Option<String>,
    pub attrs: serde_json::Value,
    pub flags: Flags,
    pub length_km: Option<f64>,
    pub area_km2: Option<f64>,
}

/// One unit of the reference boundary set.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub code: String,
    pub level: i16,
    pub parent: Option<String>,
    pub name: String,
    pub area_km2: Option<f64>,
}

/// The unit hierarchy of the reference boundary set.
#[derive(Debug, Clone, Default)]
pub struct Units {
    pub list: Vec<Unit>,
    by_code: HashMap<String, usize>,
    children: HashMap<String, Vec<usize>>,
}

impl Units {
    #[must_use]
    pub fn new(mut list: Vec<Unit>) -> Self {
        list.sort_by(|a, b| (a.level, &a.code).cmp(&(b.level, &b.code)));
        let by_code = list
            .iter()
            .enumerate()
            .map(|(i, u)| (u.code.clone(), i))
            .collect();
        let mut children: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, u) in list.iter().enumerate() {
            if let Some(p) = &u.parent {
                children.entry(p.clone()).or_default().push(i);
            }
        }
        Self {
            list,
            by_code,
            children,
        }
    }

    #[must_use]
    pub fn get(&self, code: &str) -> Option<&Unit> {
        self.by_code.get(code).map(|&i| &self.list[i])
    }

    pub fn children(&self, code: &str) -> impl Iterator<Item = &Unit> {
        self.children
            .get(code)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|&i| &self.list[i])
    }

    #[must_use]
    pub fn child_count(&self, code: &str) -> usize {
        self.children.get(code).map_or(0, Vec::len)
    }

    /// Ancestor of `code` at `level` (itself when it is at that level).
    #[must_use]
    pub fn ancestor_at(&self, code: &str, level: i16) -> Option<&Unit> {
        let mut u = self.get(code)?;
        while u.level > level {
            u = self.get(u.parent.as_deref()?)?;
        }
        (u.level == level).then_some(u)
    }

    pub fn at_level(&self, level: i16) -> impl Iterator<Item = &Unit> {
        self.list.iter().filter(move |u| u.level == level)
    }
}

/// A grid layer aligned to the canonical grid with its unit index
/// (`gnd_idx`), as WP-05's `lanka-grid` provides it.
#[derive(Debug, Clone)]
pub struct GridInput {
    pub layer: String,
    /// Cell values; NaN = nodata.
    pub values: Arc<[f32]>,
    /// Per cell, the index into `units` of the deepest unit; `u32::MAX` = none.
    pub unit_idx: Arc<[u32]>,
    /// Unit codes by index (deepest level).
    pub units: Arc<[String]>,
    pub basis: Basis,
    pub flags: Flags,
}

/// Indicator semantics a method needs (from `catalog.indicators`).
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorMeta {
    pub id: String,
    pub dataset_id: String,
    pub freq: Freq,
    pub measure_type: String,
    pub aggregation: String,
    pub class: String,
    pub numerator: Option<String>,
    pub denominator: Option<String>,
    pub weight: Option<String>,
    pub ci_level: Option<f32>,
    pub dims: Vec<String>,
}

// ───────────────────────────── inputs / outputs ────────────────────────────

/// Read access to a method's inputs. Implemented over pinned Postgres
/// versions (`pg::PgInputs`), over earlier steps of a recipe (`runner`) and
/// in memory ([`MemInputs`], for tests and `lanka-analysis`).
// async-trait's boxed futures are already must_use.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Inputs: Send + Sync {
    /// Observation rows of an indicators port, in a deterministic order.
    async fn obs(&self, port: &str) -> Result<Arc<Vec<Row>>>;
    /// Features of a features port, ordered by key.
    async fn features(&self, port: &str) -> Result<Arc<Vec<Feature>>>;
    /// The reference unit hierarchy (with areas).
    async fn units(&self) -> Result<Arc<Units>>;
    /// A grid layer with its unit index (WP-05 implements the Postgres side).
    async fn grid(&self, port: &str) -> Result<Arc<GridInput>> {
        bail!("port {port:?}: grid inputs are provided by lanka-grid (WP-05)")
    }
    fn indicator(&self, id: &str) -> Option<IndicatorMeta>;
    fn calendars(&self) -> &Calendars;
}

/// An in-memory [`Inputs`]: tables by port, plus units and indicator metadata.
#[derive(Debug, Clone, Default)]
pub struct MemInputs {
    pub obs: HashMap<String, Arc<Vec<Row>>>,
    pub features: HashMap<String, Arc<Vec<Feature>>>,
    pub grids: HashMap<String, Arc<GridInput>>,
    pub units: Arc<Units>,
    pub indicators: HashMap<String, IndicatorMeta>,
    pub calendars: Calendars,
}

#[async_trait]
impl Inputs for MemInputs {
    async fn obs(&self, port: &str) -> Result<Arc<Vec<Row>>> {
        self.obs
            .get(port)
            .cloned()
            .with_context(|| format!("no indicators input {port:?}"))
    }
    async fn features(&self, port: &str) -> Result<Arc<Vec<Feature>>> {
        self.features
            .get(port)
            .cloned()
            .with_context(|| format!("no features input {port:?}"))
    }
    async fn units(&self) -> Result<Arc<Units>> {
        Ok(self.units.clone())
    }
    async fn grid(&self, port: &str) -> Result<Arc<GridInput>> {
        self.grids
            .get(port)
            .cloned()
            .with_context(|| format!("no grid input {port:?}"))
    }
    fn indicator(&self, id: &str) -> Option<IndicatorMeta> {
        self.indicators.get(id).cloned()
    }
    fn calendars(&self) -> &Calendars {
        &self.calendars
    }
}

/// What a contributor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContribKind {
    /// Row `idx` of `inputs.obs(port)`.
    Obs,
    /// Feature `idx` of `inputs.features(port)`.
    Feature,
    /// Unit `idx` of `inputs.units().list` (port is the boundaries port).
    Unit,
    /// A summary of grid cells (idx = cell count).
    Cells,
}

/// The role a contributor played (04 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Numerator,
    Denominator,
    Term,
    SourceUnit,
    Anchor,
    CellSum,
    Feature,
    Geometry,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Contrib {
    pub port: Arc<str>,
    pub kind: ContribKind,
    pub idx: u32,
    pub role: Role,
    pub weight: f64,
}

/// One output row and the input rows it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct OutRow {
    pub row: Row,
    pub contrib: Vec<Contrib>,
}

/// Streaming sink: methods write rows as they produce them.
pub trait OutputSink: Send {
    fn row(&mut self, row: OutRow) -> Result<()>;
}

/// The in-memory sink (library use, tests, recipe steps).
#[derive(Debug, Default)]
pub struct CollectSink {
    pub rows: Vec<OutRow>,
}

impl OutputSink for CollectSink {
    fn row(&mut self, row: OutRow) -> Result<()> {
        self.rows.push(row);
        Ok(())
    }
}

/// Keys whose inputs changed since the previous derivation (partial
/// recompute hint, 04 §7). `None` everywhere = recompute fully.
#[derive(Debug, Clone, Default)]
pub struct ChangedKeys {
    pub geos: std::collections::BTreeSet<String>,
}

pub struct MethodCtx<'a> {
    pub inputs: &'a dyn Inputs,
    /// First 8 bytes of the fingerprint.
    pub rng_seed: u64,
    /// The run's reference date; never the wall clock.
    pub as_of: NaiveDate,
    pub changed_keys: Option<&'a ChangedKeys>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MethodReport {
    pub rows: u64,
    pub diagnostics: serde_json::Value,
}

/// A port reference found in a step's parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortUse {
    /// The recipe input or earlier step it names.
    pub reference: String,
    pub kind: PortKind,
    /// Indicator ids read from it, when the parameters name them.
    pub indicators: Vec<String>,
}

/// One input row behind an output row (04 §6).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Contributor {
    pub port: String,
    pub kind: ContribKind,
    pub role: Role,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obs_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indicator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dims: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_key: Option<String>,
    /// Cell count for grid summaries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cells: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Explanation {
    pub method: String,
    pub key: RowKey,
    pub value: Option<f64>,
    pub formula: String,
    pub contributors: Vec<Contributor>,
    /// Contributors beyond the listing limit (features, cells).
    pub truncated: usize,
}

/// Listing limit for contributors of one row.
pub const MAX_CONTRIBUTORS: usize = 500;

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Method: Send + Sync {
    fn info(&self) -> &MethodInfo;

    /// The ports a step's parameters reference (checked against the
    /// recipe's inputs and earlier steps; edges of the step DAG).
    fn ports(&self, params: &serde_json::Value) -> Result<Vec<PortUse>>;

    /// Deterministic. No network. Reads only through `ctx.inputs`, writes
    /// only through `out`.
    async fn run(
        &self,
        ctx: &MethodCtx<'_>,
        params: &serde_json::Value,
        out: &mut dyn OutputSink,
    ) -> Result<MethodReport>;

    /// A one-line formula for an output row.
    fn formula(&self, _params: &serde_json::Value, out: &OutRow) -> String {
        format!(
            "{}[{}, {}] = {}({} inputs)",
            out.row.indicator,
            out.row.geo,
            out.row.start,
            self.info().id,
            out.contrib.len()
        )
    }

    /// For one output key, the input rows and weights that produced it.
    /// The default re-runs the method and reads the row's contributors;
    /// methods with a cheaper path override it.
    async fn explain(
        &self,
        ctx: &MethodCtx<'_>,
        params: &serde_json::Value,
        key: &RowKey,
    ) -> Result<Explanation> {
        let mut sink = CollectSink::default();
        self.run(ctx, params, &mut sink).await?;
        let out = sink
            .rows
            .into_iter()
            .find(|r| r.row.key() == *key)
            .with_context(|| format!("{} produced no row for {key:?}", self.info().id))?;
        let (contributors, truncated) = contributors(ctx.inputs, &out).await?;
        Ok(Explanation {
            method: self.info().reference(),
            key: key.clone(),
            value: out.row.value,
            formula: self.formula(params, &out),
            contributors,
            truncated,
        })
    }
}

/// Resolves an output row's [`Contrib`] list against the inputs.
pub async fn contributors(inputs: &dyn Inputs, out: &OutRow) -> Result<(Vec<Contributor>, usize)> {
    let cal = inputs.calendars().clone();
    let mut list = Vec::new();
    for c in out.contrib.iter().take(MAX_CONTRIBUTORS) {
        let base = Contributor {
            port: c.port.to_string(),
            kind: c.kind,
            role: c.role,
            weight: c.weight,
            obs_id: None,
            indicator: None,
            geo: None,
            period: None,
            dims: None,
            value: None,
            basis: None,
            feature_key: None,
            cells: None,
        };
        list.push(match c.kind {
            ContribKind::Obs => {
                let rows = inputs.obs(&c.port).await?;
                let r = rows
                    .get(c.idx as usize)
                    .with_context(|| format!("contributor {} of {} out of range", c.idx, c.port))?;
                Contributor {
                    obs_id: r.obs_id,
                    indicator: Some(r.indicator.clone()),
                    geo: Some(r.geo.clone()),
                    period: Some(r.period(&cal)),
                    dims: Some(r.dims.clone()),
                    value: r.value,
                    basis: Some(r.basis.as_str().to_owned()),
                    ..base
                }
            }
            ContribKind::Feature => {
                let fs = inputs.features(&c.port).await?;
                let f = fs
                    .get(c.idx as usize)
                    .with_context(|| format!("feature {} of {} out of range", c.idx, c.port))?;
                Contributor {
                    feature_key: Some(f.key.clone()),
                    geo: f.unit.clone(),
                    dims: Some(f.kind.clone()),
                    ..base
                }
            }
            ContribKind::Unit => {
                let units = inputs.units().await?;
                let u = units
                    .list
                    .get(c.idx as usize)
                    .with_context(|| format!("unit {} out of range", c.idx))?;
                Contributor {
                    geo: Some(u.code.clone()),
                    value: u.area_km2,
                    ..base
                }
            }
            ContribKind::Cells => Contributor {
                cells: Some(c.idx),
                ..base
            },
        });
    }
    Ok((list, out.contrib.len().saturating_sub(MAX_CONTRIBUTORS)))
}

/// Reads a required string parameter.
pub fn param_str<'a>(params: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    params
        .get(name)
        .and_then(serde_json::Value::as_str)
        .with_context(|| format!("parameter {name:?} (string) is required"))
}

/// A parameter that is a string or a list of strings.
#[must_use]
pub fn param_strs(params: &serde_json::Value, name: &str) -> Vec<String> {
    match params.get(name) {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// A parameter that is a list of admin levels.
#[must_use]
pub fn param_levels(params: &serde_json::Value, name: &str) -> Vec<i16> {
    params
        .get(name)
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_i64().and_then(|l| i16::try_from(l).ok()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_requirements() {
        let v = Version::parse("1.2.3").expect("v");
        assert_eq!(v, Version::new(1, 2, 3));
        assert_eq!(Version::parse("2").expect("v"), Version::new(2, 0, 0));
        assert!(Version::parse("1.x").is_err());
        assert!(VersionReq::parse("^1").expect("r").matches(v));
        assert!(VersionReq::parse("1.2").expect("r").matches(v));
        assert!(!VersionReq::parse("^1.3").expect("r").matches(v));
        assert!(!VersionReq::parse("^2").expect("r").matches(v));
        assert!(VersionReq::parse("=1.2.3").expect("r").matches(v));
        assert!(!VersionReq::parse("=1.2.4").expect("r").matches(v));
        let (id, req) = parse_method_ref("rollup@^1").expect("ref");
        assert_eq!(id, "rollup");
        assert!(req.matches(Version::new(1, 9, 0)));
    }

    #[test]
    fn unit_hierarchy() {
        let u = |c: &str, l: i16, p: Option<&str>| Unit {
            code: c.into(),
            level: l,
            parent: p.map(Into::into),
            name: c.into(),
            area_km2: Some(1.0),
        };
        let units = Units::new(vec![
            u("ZZ11", 2, Some("ZZ1")),
            u("ZZ", 0, None),
            u("ZZ1", 1, Some("ZZ")),
            u("ZZ12", 2, Some("ZZ1")),
        ]);
        assert_eq!(units.child_count("ZZ1"), 2);
        assert_eq!(
            units.ancestor_at("ZZ12", 0).map(|u| u.code.as_str()),
            Some("ZZ")
        );
        assert_eq!(units.ancestor_at("ZZ1", 2), None);
        assert_eq!(units.at_level(2).count(), 2);
    }
}
