//! Source/dataset registry (`registry/sources/*.yaml`, `registry/datasets/*.yaml`,
//! `registry/codelists/*.yaml`, `registry/units.yaml`).
//!
//! Every file is validated against `registry/schema/*.schema.json` (JSON
//! Schema 2020-12) before it is deserialised, the file stem must equal `id`,
//! indicator ids must be globally unique, and cross-references are checked
//! (units, codelists, numerator/denominator/weight, class rules).
//! `sync` mirrors the registry and the country profile into `catalog.*` and
//! `geo.schemes` in one transaction, then backfills version hashes.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use lanka_core::units::{Aggregation, MeasureType, UnitKind, check_unit_code};
use lanka_core::{Freq, IndicatorClass, Profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

// ───────────────────────────── types ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub id: String,
    pub publisher: String,
    pub title: String,
    pub homepage: String,
    pub access: Access,
    pub licence: Licence,
    pub cadence: Cadence,
    pub geo: GeoSpec,
    pub themes: Vec<String>,
    pub datasets: Vec<String>,
    pub priority: String,
    pub effort: String,
    pub status: String,
    #[serde(default)]
    pub verified_at: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub dc_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    pub method: String,
    pub endpoint: String,
    pub auth: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Licence {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
    /// SPDX id; derived from `id` for the common licences when omitted.
    #[serde(default)]
    pub spdx: Option<String>,
    /// bulk | api | link | internal; derived from `status` when omitted.
    #[serde(default)]
    pub redistribution: Option<String>,
}

impl Licence {
    /// The SPDX identifier (explicit, or by licence id).
    #[must_use]
    pub fn spdx(&self) -> Option<String> {
        if self.spdx.is_some() {
            return self.spdx.clone();
        }
        let id = self.id.to_ascii_lowercase();
        let known = [
            ("cc0-1.0", "CC0-1.0"),
            ("cc-by-4.0", "CC-BY-4.0"),
            ("cc-by-3.0-igo", "CC-BY-3.0-IGO"),
            ("cc-by-sa-4.0", "CC-BY-SA-4.0"),
            ("cc-by-nc-4.0", "CC-BY-NC-4.0"),
            ("cc-by-nc-sa-4.0", "CC-BY-NC-SA-4.0"),
            ("odbl-1.0", "ODbL-1.0"),
            ("cdla-permissive-2.0", "CDLA-Permissive-2.0"),
            ("pddl-1.0", "PDDL-1.0"),
        ];
        known
            .iter()
            .find(|(k, _)| *k == id)
            .map(|(_, v)| (*v).to_owned())
    }

    /// Redistribution tier (08 §2): explicit, or by licence status.
    #[must_use]
    pub fn redistribution(&self) -> &str {
        match (&self.redistribution, self.status.as_str()) {
            (Some(r), _) => r,
            (None, "open" | "attribution") => "bulk",
            (None, "permission_unclear") => "api",
            (None, _) => "link",
        }
    }

    #[must_use]
    pub fn share_alike(&self) -> bool {
        let id = self.id.to_ascii_lowercase();
        id.contains("-sa") || id.starts_with("odbl")
    }

    #[must_use]
    pub fn non_commercial(&self) -> bool {
        self.id.to_ascii_lowercase().contains("-nc")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cadence {
    pub frequency: String,
    #[serde(default)]
    pub release_calendar_url: Option<String>,
    #[serde(default)]
    pub expected_lag_days: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoSpec {
    pub levels: Vec<String>,
    pub join: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetSpec {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source_ids: Vec<String>,
    pub indicators: Vec<IndicatorSpec>,
    pub time_coverage: TimeCoverage,
    pub method: String,
    pub limits: Vec<String>,
    #[serde(default)]
    pub enrichment: Option<serde_json::Value>,
    pub publication_class: String,
    /// indicators | features | boundaries | grid | crosswalk | codelist
    #[serde(default)]
    pub kind: Option<String>,
    /// Boundary set the geographies are keyed on (default: the reference set).
    #[serde(default)]
    pub boundary_set: Option<String>,
    /// Intraday release cuts allowed (ADR 0012).
    #[serde(default)]
    pub live: bool,
}

impl DatasetSpec {
    #[must_use]
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("indicators")
    }
}

/// A label: English text, or `{en, si, ta}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LabelSpec {
    Text(String),
    Langs {
        en: String,
        #[serde(default)]
        si: Option<String>,
        #[serde(default)]
        ta: Option<String>,
    },
}

impl LabelSpec {
    #[must_use]
    pub fn en(&self) -> &str {
        match self {
            Self::Text(s) | Self::Langs { en: s, .. } => s,
        }
    }
    #[must_use]
    pub fn si(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Langs { si, .. } => si.as_deref(),
        }
    }
    #[must_use]
    pub fn ta(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Langs { ta, .. } => ta.as_deref(),
        }
    }
}

/// A dimension: a bare id (codelist of the same name), or `{id, codelist}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DimSpec {
    Id(String),
    Ref { id: String, codelist: String },
}

impl DimSpec {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Id(s) | Self::Ref { id: s, .. } => s,
        }
    }
    #[must_use]
    pub fn codelist(&self) -> &str {
        match self {
            Self::Id(s) | Self::Ref { codelist: s, .. } => s,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorSpec {
    pub id: String,
    pub label: LabelSpec,
    #[serde(default)]
    pub definition: Option<String>,
    /// UCUM code (ISO 4217 for money), listed in `registry/units.yaml`.
    pub unit: String,
    /// Power of ten of the stored values (3 = thousands).
    #[serde(default)]
    pub unit_mult: i16,
    #[serde(default)]
    pub decimals: Option<i16>,
    pub measure_type: MeasureType,
    pub aggregation: Aggregation,
    pub class: IndicatorClass,
    #[serde(default)]
    pub numerator: Option<String>,
    #[serde(default)]
    pub denominator: Option<String>,
    #[serde(default)]
    pub weight: Option<String>,
    #[serde(default)]
    pub ci_level: Option<f32>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub sdg: Option<String>,
    #[serde(default)]
    pub base_period: Option<String>,
    #[serde(default)]
    pub dims: Vec<DimSpec>,
    pub geo_levels: Vec<String>,
    pub period_type: String,
    #[serde(default)]
    pub upstream_id: Option<String>,
}

impl IndicatorSpec {
    /// Declared dimension ids, in order.
    pub fn dim_ids(&self) -> impl Iterator<Item = &str> {
        self.dims.iter().map(DimSpec::id)
    }

    #[must_use]
    pub fn freq(&self) -> Option<Freq> {
        Freq::from_period_type(&self.period_type)
    }
}

/// `registry/codelists/<id>.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodelistSpec {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub sdmx_ref: Option<String>,
    pub codes: Vec<CodeSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeSpec {
    pub code: String,
    pub label: LabelSpec,
    #[serde(default)]
    pub parent: Option<String>,
    /// The total member (SDMX `_T`); at most one per codelist.
    #[serde(default)]
    pub total: bool,
    /// SDMX cross-domain code when ours differs (`F`, `M`, `_T`, `Y15T19`).
    #[serde(default)]
    pub sdmx: Option<String>,
}

/// `registry/units.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitsFile {
    pub units: Vec<UnitSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitSpec {
    /// Case-sensitive UCUM code (ISO 4217 for money).
    pub code: String,
    pub label: LabelSpec,
    pub kind: UnitKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeCoverage {
    #[serde(default)]
    pub from: Option<Period>,
    #[serde(default)]
    pub to: Option<Period>,
}

/// A period that YAML may give as a bare year (`2024`) or a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Period {
    Year(i64),
    Text(String),
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Year(y) => write!(f, "{y}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Loaded<T> {
    pub spec: T,
    pub path: PathBuf,
    /// The file as JSON (stored verbatim in `catalog.*.spec`).
    pub json: serde_json::Value,
    pub sha256: [u8; 32],
}

/// `registry/connectors/<source>.yaml`: a declarative driver configuration
/// (03 §2.2). Only the envelope is typed here; each driver deserialises
/// `Loaded::json` into its own spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorSpec {
    pub source: String,
    pub driver: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub root: PathBuf,
    /// Driver specs by source id.
    pub connectors: BTreeMap<String, Loaded<ConnectorSpec>>,
    pub sources: BTreeMap<String, Loaded<SourceSpec>>,
    pub datasets: BTreeMap<String, Loaded<DatasetSpec>>,
    pub codelists: BTreeMap<String, Loaded<CodelistSpec>>,
    pub units: BTreeMap<String, UnitSpec>,
    /// Non-fatal findings (dangling cross-references, licence conflicts).
    pub warnings: Vec<String>,
}

/// All problems found while loading; fatal as a group.
#[derive(Debug)]
pub struct RegistryErrors(pub Vec<String>);

impl fmt::Display for RegistryErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "registry has {} error(s):", self.0.len())?;
        for e in &self.0 {
            writeln!(f, "  - {e}")?;
        }
        Ok(())
    }
}
impl std::error::Error for RegistryErrors {}

// ───────────────────────────── loading ─────────────────────────────────────

struct Schemas {
    source: jsonschema::Validator,
    dataset: jsonschema::Validator,
    codelist: jsonschema::Validator,
    units: jsonschema::Validator,
}

fn compile_schema(path: &Path) -> Result<jsonschema::Validator> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let schema: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    jsonschema::draft202012::new(&schema).map_err(|e| anyhow!("compiling {}: {e}", path.display()))
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    files.sort();
    Ok(files)
}

fn load_one<T: for<'de> Deserialize<'de>>(
    path: &Path,
    validator: &jsonschema::Validator,
    errors: &mut Vec<String>,
) -> Option<Loaded<T>> {
    let rel = path.display();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            errors.push(format!("{rel}: {e}"));
            return None;
        }
    };
    let json: serde_json::Value = match serde_yaml_ng::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("{rel}: invalid YAML: {e}"));
            return None;
        }
    };
    let schema_errors: Vec<String> = validator
        .iter_errors(&json)
        .map(|e| format!("{rel}: {} (at {})", e, e.instance_path()))
        .collect();
    if !schema_errors.is_empty() {
        errors.extend(schema_errors);
        return None;
    }
    let spec: T = match serde_json::from_value(json.clone()) {
        Ok(s) => s,
        Err(e) => {
            errors.push(format!("{rel}: {e}"));
            return None;
        }
    };
    let sha256: [u8; 32] = Sha256::digest(&bytes).into();
    Some(Loaded {
        spec,
        path: path.to_owned(),
        json,
        sha256,
    })
}

fn file_stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

impl Registry {
    /// Loads and validates `root/{sources,datasets}/*.yaml` against
    /// `root/schema/*.schema.json`. Returns every error at once.
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_owned();
        let schemas = Schemas {
            source: compile_schema(&root.join("schema/source.schema.json"))?,
            dataset: compile_schema(&root.join("schema/dataset.schema.json"))?,
            codelist: compile_schema(&root.join("schema/codelist.schema.json"))?,
            units: compile_schema(&root.join("schema/units.schema.json"))?,
        };
        let mut errors = Vec::new();
        let mut reg = Self {
            root: root.clone(),
            ..Self::default()
        };

        for path in yaml_files(&root.join("sources"))? {
            if let Some(l) = load_one::<SourceSpec>(&path, &schemas.source, &mut errors) {
                if l.spec.id != file_stem(&path) {
                    errors.push(format!(
                        "{}: id {:?} does not match file name",
                        path.display(),
                        l.spec.id
                    ));
                }
                reg.sources.insert(l.spec.id.clone(), l);
            }
        }
        for path in yaml_files(&root.join("datasets"))? {
            if let Some(l) = load_one::<DatasetSpec>(&path, &schemas.dataset, &mut errors) {
                if l.spec.id != file_stem(&path) {
                    errors.push(format!(
                        "{}: id {:?} does not match file name",
                        path.display(),
                        l.spec.id
                    ));
                }
                reg.datasets.insert(l.spec.id.clone(), l);
            }
        }

        for path in yaml_files(&root.join("codelists"))? {
            if let Some(l) = load_one::<CodelistSpec>(&path, &schemas.codelist, &mut errors) {
                if l.spec.id != file_stem(&path) {
                    errors.push(format!(
                        "{}: id {:?} does not match file name",
                        path.display(),
                        l.spec.id
                    ));
                }
                reg.codelists.insert(l.spec.id.clone(), l);
            }
        }
        let units_path = root.join("units.yaml");
        if let Some(u) = load_one::<UnitsFile>(&units_path, &schemas.units, &mut errors) {
            for unit in u.spec.units {
                if let Err(e) = check_unit_code(&unit.code) {
                    errors.push(format!("{}: {e}", units_path.display()));
                }
                if let Some(prev) = reg.units.insert(unit.code.clone(), unit) {
                    errors.push(format!(
                        "{}: unit {:?} listed twice",
                        units_path.display(),
                        prev.code
                    ));
                }
            }
        }

        let connector_schema = root.join("schema/connector.schema.json");
        let connector_files = yaml_files(&root.join("connectors"))?;
        if !connector_files.is_empty() {
            let v = compile_schema(&connector_schema)?;
            for path in connector_files {
                if let Some(l) = load_one::<ConnectorSpec>(&path, &v, &mut errors) {
                    if l.spec.source != file_stem(&path) {
                        errors.push(format!(
                            "{}: source {:?} does not match file name",
                            path.display(),
                            l.spec.source
                        ));
                    }
                    if !reg.sources.contains_key(&l.spec.source) {
                        errors.push(format!(
                            "{}: unknown source {:?}",
                            path.display(),
                            l.spec.source
                        ));
                    }
                    reg.connectors.insert(l.spec.source.clone(), l);
                }
            }
        }

        reg.check_references(&mut errors);
        if errors.is_empty() {
            Ok(reg)
        } else {
            Err(RegistryErrors(errors).into())
        }
    }

    fn check_references(&mut self, errors: &mut Vec<String>) {
        self.check_semantics(errors);
        // Indicator ids are global.
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for d in self.datasets.values() {
            for i in &d.spec.indicators {
                if let Some(prev) = seen.insert(&i.id, &d.spec.id) {
                    errors.push(format!(
                        "indicator {:?} defined in both {prev:?} and {:?}",
                        i.id, d.spec.id
                    ));
                }
            }
            for s in &d.spec.source_ids {
                if !self.sources.contains_key(s) {
                    self.warnings.push(format!(
                        "dataset {:?} references unknown source {s:?}",
                        d.spec.id
                    ));
                }
            }
        }
        for s in self.sources.values() {
            for d in &s.spec.datasets {
                match self.datasets.get(d) {
                    None => self.warnings.push(format!("source {:?} lists dataset {d:?} with no datasets/{d}.yaml", s.spec.id)),
                    Some(ds) if !ds.spec.source_ids.contains(&s.spec.id) => self.warnings.push(format!(
                        "source {:?} lists dataset {d:?} but the dataset does not list the source back",
                        s.spec.id
                    )),
                    Some(_) => {}
                }
            }
        }
        // One licence id must mean one status everywhere.
        let mut lic: HashMap<&str, (&str, &Licence)> = HashMap::new();
        for s in self.sources.values() {
            let l = &s.spec.licence;
            if let Some((other, prev)) = lic.get(l.id.as_str()) {
                if prev.status != l.status {
                    errors.push(format!(
                        "licence {:?} has status {:?} in {other:?} but {:?} in {:?}",
                        l.id, prev.status, l.status, s.spec.id
                    ));
                }
            } else {
                lic.insert(&l.id, (&s.spec.id, l));
            }
        }
    }

    /// Units, codelists, measure semantics and class rules (02 §3).
    fn check_semantics(&mut self, errors: &mut Vec<String>) {
        let all: HashSet<&str> = self
            .datasets
            .values()
            .flat_map(|d| d.spec.indicators.iter().map(|i| i.id.as_str()))
            .collect();
        let mut pending: BTreeMap<String, usize> = BTreeMap::new();
        for cl in self.codelists.values() {
            let mut codes = HashSet::new();
            for c in &cl.spec.codes {
                if !codes.insert(c.code.as_str()) {
                    errors.push(format!(
                        "codelist {:?}: code {:?} twice",
                        cl.spec.id, c.code
                    ));
                }
            }
            for c in &cl.spec.codes {
                if let Some(p) = &c.parent
                    && !codes.contains(p.as_str())
                {
                    errors.push(format!(
                        "codelist {:?}: {:?} has unknown parent {p:?}",
                        cl.spec.id, c.code
                    ));
                }
            }
            if cl.spec.codes.iter().filter(|c| c.total).count() > 1 {
                errors.push(format!(
                    "codelist {:?}: more than one total member",
                    cl.spec.id
                ));
            }
        }
        for d in self.datasets.values() {
            let ds = &d.spec;
            for i in &ds.indicators {
                let at = format!("{} / {}", ds.id, i.id);
                if !self.units.contains_key(&i.unit) {
                    errors.push(format!(
                        "{at}: unit {:?} is not in registry/units.yaml",
                        i.unit
                    ));
                }
                if i.freq().is_none() {
                    errors.push(format!(
                        "{at}: period_type {:?} has no frequency",
                        i.period_type
                    ));
                }
                for (name, r) in [
                    ("numerator", &i.numerator),
                    ("denominator", &i.denominator),
                    ("weight", &i.weight),
                ] {
                    if let Some(r) = r
                        && !all.contains(r.as_str())
                    {
                        errors.push(format!("{at}: {name} {r:?} is not a declared indicator"));
                    }
                }
                let (needs, _) = i.aggregation.needs();
                for n in needs {
                    let present = match *n {
                        "numerator" => i.numerator.is_some(),
                        "denominator" => i.denominator.is_some(),
                        _ => i.weight.is_some(),
                    };
                    if !present {
                        errors.push(format!("{at}: aggregation {} needs a {n}", i.aggregation));
                    }
                }
                let est_ns = i.id.starts_with("est.");
                if est_ns != (i.class == IndicatorClass::Estimated) {
                    errors.push(format!(
                        "{at}: the est. namespace is reserved for (and required by) class estimated"
                    ));
                }
                if ds.id.starts_with("derived-")
                    && !matches!(i.class, IndicatorClass::Derived | IndicatorClass::Estimated)
                {
                    errors.push(format!(
                        "{at}: a derived- dataset publishes derived or estimated indicators"
                    ));
                }
                let mut dims = HashSet::new();
                for dim in &i.dims {
                    if !dims.insert(dim.id()) {
                        errors.push(format!("{at}: dimension {:?} declared twice", dim.id()));
                    }
                    if !self.codelists.contains_key(dim.codelist()) {
                        *pending.entry(dim.codelist().to_owned()).or_default() += 1;
                    }
                }
            }
        }
        if !pending.is_empty() {
            self.warnings.push(format!(
                "{} codelist(s) referenced but not written yet (needed before those datasets are ingested): {}",
                pending.len(),
                pending
                    .iter()
                    .map(|(c, n)| format!("{c} ({n})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    /// Validates a country profile file against `schema/profile.schema.json`.
    pub fn validate_profile(root: &Path, profile: &Profile) -> Result<()> {
        let v = compile_schema(&root.join("schema/profile.schema.json"))?;
        let errors: Vec<String> = v
            .iter_errors(&profile.json)
            .map(|e| format!("profile: {e} (at {})", e.instance_path()))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(RegistryErrors(errors).into())
        }
    }

    pub fn source(&self, id: &str) -> Option<&SourceSpec> {
        self.sources.get(id).map(|l| &l.spec)
    }

    pub fn dataset(&self, id: &str) -> Option<&DatasetSpec> {
        self.datasets.get(id).map(|l| &l.spec)
    }

    /// Which dataset owns an indicator id.
    pub fn indicator_owner(&self, indicator_id: &str) -> Option<&str> {
        self.datasets
            .values()
            .find(|d| d.spec.indicators.iter().any(|i| i.id == indicator_id))
            .map(|d| d.spec.id.as_str())
    }

    /// Counts for logs and `registry validate`.
    pub fn summary(&self) -> String {
        let indicators: usize = self
            .datasets
            .values()
            .map(|d| d.spec.indicators.len())
            .sum();
        format!(
            "{} sources, {} datasets, {} indicators",
            self.sources.len(),
            self.datasets.len(),
            indicators
        )
    }

    /// Mirrors the country profile and the registry into `catalog.*` and
    /// `geo.schemes` in one transaction and makes sure every dataset has its
    /// observation partitions. Rows for ids that disappeared from the
    /// registry are kept (history must stay joinable); sources are marked
    /// `status = 'retired'` instead. Afterwards, dataset versions without a
    /// multiset hash (migrated from 0001) get one ([`crate::publish::backfill_hashes`]).
    pub async fn sync(&self, pool: &PgPool, profile: &Profile) -> Result<SyncReport> {
        let mut tx = pool.begin().await?;
        let mut report = SyncReport::default();
        sync_profile(&mut tx, profile).await?;

        for u in self.units.values() {
            sqlx::query(
                "INSERT INTO catalog.units_of_measure (code, label_en, label_si, label_ta, kind)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (code) DO UPDATE SET label_en = EXCLUDED.label_en,
                    label_si = EXCLUDED.label_si, label_ta = EXCLUDED.label_ta, kind = EXCLUDED.kind",
            )
            .bind(&u.code)
            .bind(u.label.en())
            .bind(u.label.si())
            .bind(u.label.ta())
            .bind(u.kind.as_str())
            .execute(&mut *tx)
            .await?;
        }
        report.units = self.units.len();

        for l in self.codelists.values() {
            let c = &l.spec;
            sqlx::query(
                "INSERT INTO catalog.codelists (id, title, sdmx_ref, spec, spec_sha256, synced_at)
                 VALUES ($1, $2, $3, $4, $5, now())
                 ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, sdmx_ref = EXCLUDED.sdmx_ref,
                    spec = EXCLUDED.spec, spec_sha256 = EXCLUDED.spec_sha256, synced_at = now()",
            )
            .bind(&c.id)
            .bind(&c.title)
            .bind(&c.sdmx_ref)
            .bind(&l.json)
            .bind(l.sha256.as_slice())
            .execute(&mut *tx)
            .await?;
            let codes: Vec<&str> = c.codes.iter().map(|x| x.code.as_str()).collect();
            sqlx::query("DELETE FROM catalog.codes WHERE codelist_id = $1 AND code <> ALL($2)")
                .bind(&c.id)
                .bind(&codes)
                .execute(&mut *tx)
                .await?;
            // Clear the total flag first so moving it never trips codes_one_total.
            sqlx::query("UPDATE catalog.codes SET is_total = false WHERE codelist_id = $1")
                .bind(&c.id)
                .execute(&mut *tx)
                .await?;
            for (sort, code) in c.codes.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO catalog.codes (codelist_id, code, label_en, label_si, label_ta, parent_code,
                        sort, is_total, sdmx_code)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                     ON CONFLICT (codelist_id, code) DO UPDATE SET label_en = EXCLUDED.label_en,
                        label_si = EXCLUDED.label_si, label_ta = EXCLUDED.label_ta,
                        parent_code = EXCLUDED.parent_code, sort = EXCLUDED.sort,
                        is_total = EXCLUDED.is_total, sdmx_code = EXCLUDED.sdmx_code",
                )
                .bind(&c.id)
                .bind(&code.code)
                .bind(code.label.en())
                .bind(code.label.si())
                .bind(code.label.ta())
                .bind(&code.parent)
                .bind(i32::try_from(sort).unwrap_or(i32::MAX))
                .bind(code.total)
                .bind(&code.sdmx)
                .execute(&mut *tx)
                .await?;
            }
        }
        report.codelists = self.codelists.len();

        let mut licences: BTreeMap<&str, &Licence> = BTreeMap::new();
        for s in self.sources.values() {
            licences
                .entry(&s.spec.licence.id)
                .or_insert(&s.spec.licence);
        }
        for l in licences.values() {
            sqlx::query(
                "INSERT INTO catalog.licences (id, status, url, attribution, spdx, share_alike, non_commercial,
                    redistribution)
                 VALUES ($1, $2::catalog.licence_status, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status, url = EXCLUDED.url,
                    attribution = EXCLUDED.attribution, spdx = EXCLUDED.spdx,
                    share_alike = EXCLUDED.share_alike, non_commercial = EXCLUDED.non_commercial,
                    redistribution = EXCLUDED.redistribution",
            )
            .bind(&l.id)
            .bind(&l.status)
            .bind(&l.url)
            .bind(&l.attribution)
            .bind(l.spdx())
            .bind(l.share_alike())
            .bind(l.non_commercial())
            .bind(l.redistribution())
            .execute(&mut *tx)
            .await?;
        }
        report.licences = licences.len();

        for l in self.sources.values() {
            let s = &l.spec;
            let verified: Option<chrono::NaiveDate> = s
                .verified_at
                .as_deref()
                .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
            sqlx::query(
                "INSERT INTO catalog.sources (id, publisher, title, homepage, access_method, endpoint, auth,
                    licence_id, cadence, release_calendar_url, expected_lag_days, geo_levels, geo_join, themes,
                    priority, effort, status, verified_at, notes, spec, spec_sha256, synced_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21, now())
                 ON CONFLICT (id) DO UPDATE SET publisher = EXCLUDED.publisher, title = EXCLUDED.title,
                    homepage = EXCLUDED.homepage, access_method = EXCLUDED.access_method,
                    endpoint = EXCLUDED.endpoint, auth = EXCLUDED.auth, licence_id = EXCLUDED.licence_id,
                    cadence = EXCLUDED.cadence, release_calendar_url = EXCLUDED.release_calendar_url,
                    expected_lag_days = EXCLUDED.expected_lag_days, geo_levels = EXCLUDED.geo_levels,
                    geo_join = EXCLUDED.geo_join, themes = EXCLUDED.themes, priority = EXCLUDED.priority,
                    effort = EXCLUDED.effort, status = EXCLUDED.status, verified_at = EXCLUDED.verified_at,
                    notes = EXCLUDED.notes, spec = EXCLUDED.spec, spec_sha256 = EXCLUDED.spec_sha256,
                    synced_at = now()",
            )
            .bind(&s.id)
            .bind(&s.publisher)
            .bind(&s.title)
            .bind(&s.homepage)
            .bind(&s.access.method)
            .bind(&s.access.endpoint)
            .bind(&s.access.auth)
            .bind(&s.licence.id)
            .bind(&s.cadence.frequency)
            .bind(&s.cadence.release_calendar_url)
            .bind(s.cadence.expected_lag_days)
            .bind(&s.geo.levels)
            .bind(&s.geo.join)
            .bind(&s.themes)
            .bind(&s.priority)
            .bind(&s.effort)
            .bind(&s.status)
            .bind(verified)
            .bind(&s.notes)
            .bind(&l.json)
            .bind(l.sha256.as_slice())
            .execute(&mut *tx)
            .await?;
        }
        let ids: Vec<&str> = self.sources.keys().map(String::as_str).collect();
        report.retired_sources = sqlx::query(
            "UPDATE catalog.sources SET status = 'retired' WHERE id <> ALL($1) AND status <> 'retired'",
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        report.sources = self.sources.len();

        let reference = profile.reference_boundary_set();
        for l in self.datasets.values() {
            let d = &l.spec;
            let boundary_set = d
                .boundary_set
                .as_deref()
                .or((d.kind() == "indicators").then_some(reference));
            // UPDATE first, INSERT only when missing: an ON CONFLICT upsert would
            // burn an identity value (the surrogate key) on every sync.
            sqlx::query(
                "WITH upd AS (UPDATE catalog.datasets SET title = $2, description = $3, method = $4, limits = $5, publication_class = $6::catalog.publication_class, time_from = $7, time_to = $8, enrichment = $9, spec = $10, spec_sha256 = $11, kind = $12, live = $13, boundary_set = $14, synced_at = now()
                              WHERE id = $1 RETURNING 1)
                 INSERT INTO catalog.datasets (id, title, description, method, limits, publication_class, time_from, time_to, enrichment, spec, spec_sha256, kind, live, boundary_set, synced_at)
                 SELECT $1, $2, $3, $4, $5, $6::catalog.publication_class, $7, $8, $9, $10, $11, $12, $13, $14, now() WHERE NOT EXISTS (SELECT 1 FROM upd)",
            )
            .bind(&d.id)
            .bind(&d.title)
            .bind(&d.description)
            .bind(&d.method)
            .bind(&d.limits)
            .bind(&d.publication_class)
            .bind(d.time_coverage.from.as_ref().map(ToString::to_string))
            .bind(d.time_coverage.to.as_ref().map(ToString::to_string))
            .bind(&d.enrichment)
            .bind(&l.json)
            .bind(l.sha256.as_slice())
            .bind(d.kind())
            .bind(d.live)
            .bind(boundary_set)
            .execute(&mut *tx)
            .await?;

            sqlx::query("DELETE FROM catalog.dataset_sources WHERE dataset_id = $1")
                .bind(&d.id)
                .execute(&mut *tx)
                .await?;
            let known: Vec<&str> = d
                .source_ids
                .iter()
                .map(String::as_str)
                .filter(|s| self.sources.contains_key(*s))
                .collect();
            sqlx::query(
                "INSERT INTO catalog.dataset_sources (dataset_id, source_id) SELECT $1, unnest($2::text[])",
            )
            .bind(&d.id)
            .bind(&known)
            .execute(&mut *tx)
            .await?;

            for (sort, i) in d.indicators.iter().enumerate() {
                let freq = i
                    .freq()
                    .with_context(|| format!("{}: period_type {:?}", i.id, i.period_type))?;
                let dims: Vec<&str> = i.dim_ids().collect();
                let dim_defs = serde_json::Value::Array(
                    i.dims
                        .iter()
                        .map(|x| serde_json::json!({"id": x.id(), "codelist": x.codelist()}))
                        .collect(),
                );
                // UPDATE first, INSERT only when missing: an ON CONFLICT upsert would
                // burn an identity value (the surrogate key) on every sync.
                sqlx::query(
                    "WITH upd AS (UPDATE catalog.indicators SET dataset_id = $2, label = $3, label_si = $4, label_ta = $5, definition = $6, unit_code = $7, unit_mult = $8, decimals = $9, measure_type = $10, aggregation = $11, numerator_id = $12, denominator_id = $13, weight_id = $14, class = $15, ci_level = $16, direction = $17, sdg = $18, base_period = $19, dims = $20, dim_defs = $21, geo_levels = $22, period_type = $23, freq = $24::text::\"char\", sort = $25
                              WHERE id = $1 RETURNING 1)
                 INSERT INTO catalog.indicators (id, dataset_id, label, label_si, label_ta, definition, unit_code, unit_mult, decimals, measure_type, aggregation, numerator_id, denominator_id, weight_id, class, ci_level, direction, sdg, base_period, dims, dim_defs, geo_levels, period_type, freq, sort)
                 SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24::text::\"char\", $25 WHERE NOT EXISTS (SELECT 1 FROM upd)",
                )
                .bind(&i.id)
                .bind(&d.id)
                .bind(i.label.en())
                .bind(i.label.si())
                .bind(i.label.ta())
                .bind(&i.definition)
                .bind(&i.unit)
                .bind(i.unit_mult)
                .bind(i.decimals)
                .bind(i.measure_type.as_str())
                .bind(i.aggregation.as_str())
                .bind(&i.numerator)
                .bind(&i.denominator)
                .bind(&i.weight)
                .bind(i.class.as_str())
                .bind(i.ci_level)
                .bind(&i.direction)
                .bind(&i.sdg)
                .bind(&i.base_period)
                .bind(&dims)
                .bind(&dim_defs)
                .bind(&i.geo_levels)
                .bind(&i.period_type)
                .bind(freq.as_char().to_string())
                .bind(i32::try_from(sort).unwrap_or(i32::MAX))
                .execute(&mut *tx)
                .await?;
                report.indicators += 1;
            }
            sqlx::query("SELECT data.ensure_obs_partition($1)")
                .bind(&d.id)
                .execute(&mut *tx)
                .await?;
        }
        report.datasets = self.datasets.len();
        tx.commit().await?;
        report.hashes_backfilled = crate::publish::backfill_hashes(pool, profile).await?;
        Ok(report)
    }
}

/// Profile → `catalog.profile`, `geo.schemes`, the reference boundary set.
async fn sync_profile(tx: &mut sqlx::PgConnection, profile: &Profile) -> Result<()> {
    let installed: Option<String> = sqlx::query_scalar("SELECT id FROM catalog.profile")
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(id) = installed.filter(|id| *id != profile.spec.id) {
        anyhow::bail!(
            "this database holds country {id:?}; profile {:?} needs its own deployment",
            profile.spec.id
        );
    }
    sqlx::query(
        "INSERT INTO catalog.profile (id, spec, spec_sha256, synced_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (id) DO UPDATE SET spec = EXCLUDED.spec, spec_sha256 = EXCLUDED.spec_sha256,
            synced_at = now()",
    )
    .bind(&profile.spec.id)
    .bind(&profile.json)
    .bind(profile.sha256.as_slice())
    .execute(&mut *tx)
    .await?;
    let admin = profile.admin_scheme();
    sqlx::query(
        "INSERT INTO geo.schemes (id, title, prefix, hierarchical) VALUES ($1, $2, NULL, true)
         ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, prefix = NULL, hierarchical = true",
    )
    .bind(admin)
    .bind(format!(
        "Administrative units of {} (p-codes)",
        profile.spec.name.en
    ))
    .execute(&mut *tx)
    .await?;
    for s in &profile.spec.schemes {
        sqlx::query(
            "INSERT INTO geo.schemes (id, title, prefix, hierarchical) VALUES ($1, $2, $1 || ':', false)
             ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, prefix = EXCLUDED.prefix",
        )
        .bind(&s.id)
        .bind(&s.title)
        .execute(&mut *tx)
        .await?;
    }
    for s in &profile.spec.schemes {
        sqlx::query(
            "UPDATE geo.schemes SET nests_in = $2, hierarchical = $2 IS NOT NULL WHERE id = $1",
        )
        .bind(&s.id)
        .bind(&s.nests_in)
        .execute(&mut *tx)
        .await?;
    }
    let reference = profile.reference_boundary_set();
    sqlx::query(
        "INSERT INTO geo.boundary_sets (id, title, scheme_id) VALUES ($1, $1, $2) ON CONFLICT (id) DO NOTHING",
    )
    .bind(reference)
    .bind(admin)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE geo.boundary_sets SET is_reference = false WHERE scheme_id = $1 AND id <> $2 AND is_reference")
        .bind(admin)
        .bind(reference)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE geo.boundary_sets SET is_reference = true, scheme_id = $1 WHERE id = $2 AND NOT is_reference")
        .bind(admin)
        .bind(reference)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncReport {
    pub licences: usize,
    pub units: usize,
    pub codelists: usize,
    pub sources: usize,
    pub retired_sources: u64,
    pub datasets: usize,
    pub indicators: usize,
    /// Dataset versions given a multiset hash and a new dsv (migrated rows).
    pub hashes_backfilled: usize,
}

/// Unique-ish set helper used by validators.
pub fn duplicates<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut dup = BTreeSet::new();
    for i in items {
        if !seen.insert(i) {
            dup.insert(i);
        }
    }
    dup
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn fixture_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lanka-registry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("schema")).expect("mkdir");
        std::fs::create_dir_all(dir.join("sources")).expect("mkdir");
        std::fs::create_dir_all(dir.join("datasets")).expect("mkdir");
        for f in [
            "source.schema.json",
            "dataset.schema.json",
            "codelist.schema.json",
            "units.schema.json",
        ] {
            std::fs::copy(
                repo_root().join("registry/schema").join(f),
                dir.join("schema").join(f),
            )
            .expect("copy schema");
        }
        std::fs::write(
            dir.join("units.yaml"),
            "units:\n  - { code: '{count}', label: count, kind: count }\n",
        )
        .expect("units");
        dir
    }

    const SOURCE: &str = r"
id: test-src
publisher: Test Publisher
title: Test source
homepage: https://example.org
access: { method: api, endpoint: https://example.org/api, auth: none }
licence: { id: cc-by-4.0, status: open }
cadence: { frequency: annual }
geo: { levels: [national], join: none }
themes: [ECON]
datasets: [test-ds]
priority: P1
effort: S
status: planned
verified_at: 2026-11-21
";

    const DATASET: &str = r"
id: test-ds
title: Test dataset
description: A dataset used by unit tests only.
source_ids: [test-src]
indicators:
  - { id: test.value, label: Value, unit: '{count}', measure_type: count, aggregation: sum,
      class: official, geo_levels: [national], period_type: year }
time_coverage: { from: 2000, to: '2024-Q2' }
method: Made up.
limits: [Not real.]
publication_class: A
";

    #[test]
    fn loads_valid_fixture() {
        let dir = fixture_root("ok");
        std::fs::write(dir.join("sources/test-src.yaml"), SOURCE).expect("write");
        std::fs::write(dir.join("datasets/test-ds.yaml"), DATASET).expect("write");
        let reg = Registry::load(&dir).expect("valid registry");
        assert_eq!(reg.sources.len(), 1);
        let ds = reg.dataset("test-ds").expect("dataset");
        assert_eq!(ds.time_coverage.from, Some(Period::Year(2000)));
        assert_eq!(
            ds.time_coverage
                .to
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("2024-Q2")
        );
        assert_eq!(reg.indicator_owner("test.value"), Some("test-ds"));
        assert!(reg.warnings.is_empty(), "{:?}", reg.warnings);
    }

    #[test]
    fn rejects_schema_violations_and_id_mismatch() {
        let dir = fixture_root("bad");
        let bad = SOURCE
            .replace("method: api", "method: carrier_pigeon")
            .replace("P1", "P9");
        std::fs::write(dir.join("sources/other-name.yaml"), bad).expect("write");
        std::fs::write(
            dir.join("datasets/test-ds.yaml"),
            DATASET.replace("limits: [Not real.]", "limits: []"),
        )
        .expect("write");
        let err = Registry::load(&dir).expect_err("must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("carrier_pigeon"), "{msg}");
        assert!(msg.contains("P9"), "{msg}");
        assert!(msg.contains("limits"), "{msg}");
    }

    #[test]
    fn rejects_duplicate_indicator_ids() {
        let dir = fixture_root("dup");
        std::fs::write(dir.join("sources/test-src.yaml"), SOURCE).expect("write");
        std::fs::write(dir.join("datasets/test-ds.yaml"), DATASET).expect("write");
        std::fs::write(
            dir.join("datasets/test-ds2.yaml"),
            DATASET.replace("id: test-ds", "id: test-ds2"),
        )
        .expect("write");
        let msg = format!("{:#}", Registry::load(&dir).expect_err("must fail"));
        assert!(msg.contains("test.value"), "{msg}");
    }

    #[test]
    fn rejects_semantic_errors() {
        let dir = fixture_root("sem");
        std::fs::write(dir.join("sources/test-src.yaml"), SOURCE).expect("write");
        let bad = DATASET
            .replace("unit: '{count}'", "unit: persons")
            .replace("aggregation: sum", "aggregation: ratio_of_sums");
        std::fs::write(dir.join("datasets/test-ds.yaml"), bad).expect("write");
        let msg = format!("{:#}", Registry::load(&dir).expect_err("must fail"));
        assert!(msg.contains("units.yaml"), "{msg}");
        assert!(msg.contains("needs a numerator"), "{msg}");
        let est = DATASET.replace("id: test.value", "id: est.value");
        std::fs::write(dir.join("datasets/test-ds.yaml"), est).expect("write");
        let msg = format!("{:#}", Registry::load(&dir).expect_err("must fail"));
        assert!(msg.contains("est. namespace"), "{msg}");
    }

    #[test]
    fn repository_registry_is_valid() {
        let reg = Registry::load(repo_root().join("registry"))
            .expect("repository registry must validate");
        for id in ["worldbank-wdi", "dcs-census-2024", "hdx-cod-ab-lka"] {
            assert!(reg.source(id).is_some(), "missing source {id}");
        }
        // Every dimension of the datasets with a connector has its codelist.
        for ds in [
            "wdi-lka",
            "dcs-census-2024-population",
            "dcs-census-2024-ethnicity-religion",
        ] {
            for i in &reg.dataset(ds).expect("dataset").indicators {
                for d in &i.dims {
                    assert!(
                        reg.codelists.contains_key(d.codelist()),
                        "{ds}: {}",
                        d.codelist()
                    );
                }
            }
        }
        let root = repo_root().join("registry");
        let profile = Profile::load(Profile::discover(&root).expect("profile")).expect("loads");
        Registry::validate_profile(&root, &profile).expect("profile matches its schema");
    }
}
