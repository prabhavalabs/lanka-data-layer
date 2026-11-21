//! Source/dataset registry (`registry/sources/*.yaml`, `registry/datasets/*.yaml`).
//!
//! Every file is validated against `registry/schema/*.schema.json` (JSON
//! Schema 2020-12) before it is deserialised, the file stem must equal `id`,
//! indicator ids must be globally unique, and cross-references are checked.
//! `sync` mirrors the registry into `catalog.*` in one transaction.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorSpec {
    pub id: String,
    pub label: String,
    pub unit: String,
    #[serde(default)]
    pub dims: Vec<String>,
    pub geo_levels: Vec<String>,
    pub period_type: String,
    #[serde(default)]
    pub upstream_id: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub root: PathBuf,
    pub sources: BTreeMap<String, Loaded<SourceSpec>>,
    pub datasets: BTreeMap<String, Loaded<DatasetSpec>>,
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

        reg.check_references(&mut errors);
        if errors.is_empty() {
            Ok(reg)
        } else {
            Err(RegistryErrors(errors).into())
        }
    }

    fn check_references(&mut self, errors: &mut Vec<String>) {
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

    /// Mirrors the registry into `catalog.*` in one transaction and makes
    /// sure every dataset has its observation partition. Rows for ids that
    /// disappeared from the registry are kept (history must stay joinable);
    /// sources are marked `status = 'retired'` instead.
    pub async fn sync(&self, pool: &PgPool) -> Result<SyncReport> {
        let mut tx = pool.begin().await?;
        let mut report = SyncReport::default();

        let mut licences: BTreeMap<&str, &Licence> = BTreeMap::new();
        for s in self.sources.values() {
            licences
                .entry(&s.spec.licence.id)
                .or_insert(&s.spec.licence);
        }
        for l in licences.values() {
            sqlx::query(
                "INSERT INTO catalog.licences (id, status, url, attribution)
                 VALUES ($1, $2::catalog.licence_status, $3, $4)
                 ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status, url = EXCLUDED.url,
                   attribution = EXCLUDED.attribution",
            )
            .bind(&l.id)
            .bind(&l.status)
            .bind(&l.url)
            .bind(&l.attribution)
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

        for l in self.datasets.values() {
            let d = &l.spec;
            sqlx::query(
                "INSERT INTO catalog.datasets (id, title, description, method, limits, publication_class,
                    time_from, time_to, enrichment, spec, spec_sha256, synced_at)
                 VALUES ($1,$2,$3,$4,$5,$6::catalog.publication_class,$7,$8,$9,$10,$11, now())
                 ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title, description = EXCLUDED.description,
                    method = EXCLUDED.method, limits = EXCLUDED.limits,
                    publication_class = EXCLUDED.publication_class, time_from = EXCLUDED.time_from,
                    time_to = EXCLUDED.time_to, enrichment = EXCLUDED.enrichment, spec = EXCLUDED.spec,
                    spec_sha256 = EXCLUDED.spec_sha256, synced_at = now()",
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
                sqlx::query(
                    "INSERT INTO catalog.indicators (id, dataset_id, label, unit, dims, geo_levels, period_type, sort)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                     ON CONFLICT (id) DO UPDATE SET dataset_id = EXCLUDED.dataset_id, label = EXCLUDED.label,
                        unit = EXCLUDED.unit, dims = EXCLUDED.dims, geo_levels = EXCLUDED.geo_levels,
                        period_type = EXCLUDED.period_type, sort = EXCLUDED.sort",
                )
                .bind(&i.id)
                .bind(&d.id)
                .bind(&i.label)
                .bind(&i.unit)
                .bind(&i.dims)
                .bind(&i.geo_levels)
                .bind(&i.period_type)
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
        Ok(report)
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncReport {
    pub licences: usize,
    pub sources: usize,
    pub retired_sources: u64,
    pub datasets: usize,
    pub indicators: usize,
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
        for f in ["source.schema.json", "dataset.schema.json"] {
            std::fs::copy(
                repo_root().join("registry/schema").join(f),
                dir.join("schema").join(f),
            )
            .expect("copy schema");
        }
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
  - { id: test.value, label: Value, unit: count, geo_levels: [national], period_type: year }
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
    fn repository_registry_is_valid() {
        let reg = Registry::load(repo_root().join("registry"))
            .expect("repository registry must validate");
        for id in ["worldbank-wdi", "dcs-census-2024", "hdx-cod-ab-lka"] {
            assert!(reg.source(id).is_some(), "missing source {id}");
        }
    }
}
