//! `lanka connector test <source> [--from DIR]` (03 §2.2): runs discover
//! (unless `--from`), fetch, extract and standardise of one connector
//! without the database or the blob store, and evaluates the expectations
//! of every output. Nothing is archived or published.
//!
//! With `--from DIR` every file in `DIR` is one artifact whose key is the
//! file name without its last extension (`SP.POP.TOTL.json` →
//! `SP.POP.TOTL`, `production_maha.xlsx` → `production_maha`), so fixtures
//! are named after the keys the spec's tables select.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use lanka_core::Profile;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::connector::{
    CollectSink, DiscoverCtx, ExtractCtx, Fetched, Lookup, Output, RunOptions, StdCtx,
    connector_for,
};
use crate::expect::{self, CheckResult, ObsCtx};
use crate::extract::{ExtractArtifact, ExtractRef, from_parquet, to_parquet};
use crate::registry::Registry;

#[derive(Debug, Serialize)]
pub struct OutputSummary {
    pub dataset: String,
    pub kind: &'static str,
    pub vintage: Option<String>,
    pub rows: usize,
    pub first: Vec<Value>,
    pub expectations: Vec<CheckResult>,
}

#[derive(Debug, Serialize)]
pub struct TestReport {
    pub source: String,
    pub driver: String,
    pub artifacts: Vec<Value>,
    pub extracts: Vec<Value>,
    pub outputs: Vec<OutputSummary>,
    pub warnings: Vec<String>,
}

impl TestReport {
    /// Any error-severity expectation failed.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.outputs
            .iter()
            .any(|o| o.expectations.iter().any(CheckResult::is_error))
    }
}

fn files_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && !p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'))
        })
        .collect();
    v.sort();
    Ok(v)
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Runs one connector end to end in memory (see the module docs).
#[allow(clippy::too_many_lines)]
pub async fn run(
    registry: &Registry,
    profile: &Profile,
    source: &str,
    from: Option<&Path>,
    work_dir: &Path,
    lookup: &Lookup,
    first: usize,
) -> Result<TestReport> {
    let c = connector_for(registry, source)?
        .with_context(|| format!("no connector for source {source:?}"))?;
    let mut fetched = Vec::new();
    match from {
        Some(dir) => {
            for (i, p) in files_in(dir)?.into_iter().enumerate() {
                let bytes = std::fs::read(&p)?;
                let key = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("file name")?
                    .to_owned();
                fetched.push(Fetched {
                    key,
                    url: format!("file://{}", p.display()),
                    artifact_id: i64::try_from(i)? + 1,
                    sha256: sha(&bytes),
                    path: p,
                    bytes: bytes.len() as u64,
                    media_type: None,
                });
            }
            ensure!(!fetched.is_empty(), "{} has no files", dir.display());
        }
        None => {
            let http = crate::http::Http::for_source(profile, registry.source(source))?;
            let opts = RunOptions {
                trigger: "test".into(),
                ..RunOptions::default()
            };
            let d = c
                .discover(&DiscoverCtx {
                    registry,
                    profile,
                    http: &http,
                    opts: &opts,
                    run: None,
                })
                .await
                .context("discover")?;
            std::fs::create_dir_all(work_dir)?;
            for (i, cand) in d.candidates.iter().enumerate() {
                let bytes = match &cand.local_path {
                    Some(p) => std::fs::read(p)?,
                    None => http
                        .get(&cand.url)
                        .send()
                        .await
                        .and_then(reqwest::Response::error_for_status)
                        .with_context(|| format!("GET {}", cand.url))?
                        .bytes()
                        .await?
                        .to_vec(),
                };
                let path = work_dir.join(format!("{i:04}"));
                std::fs::write(&path, &bytes)?;
                fetched.push(Fetched {
                    key: cand.key.clone(),
                    url: cand.url.clone(),
                    artifact_id: i64::try_from(i)? + 1,
                    sha256: sha(&bytes),
                    path,
                    bytes: bytes.len() as u64,
                    media_type: cand.media_type.clone(),
                });
            }
        }
    }
    let sandbox = lanka_extractd::Sandbox::from_env()?;
    let outs = c
        .extract(
            &ExtractCtx {
                registry,
                profile,
                sandbox: &sandbox,
            },
            &fetched,
        )
        .await
        .context("extract")?;
    let artifacts: Vec<ExtractArtifact> = fetched
        .iter()
        .map(|f| ExtractArtifact {
            artifact_id: f.artifact_id,
            key: f.key.clone(),
            url: f.url.clone(),
            sha256: f.sha256,
        })
        .collect();
    let mut refs = Vec::new();
    let mut extracts = Vec::new();
    for (i, o) in outs.iter().enumerate() {
        // Round-trip through Parquet: standardise reads what the store holds.
        let bytes = to_parquet(o, &[("extractor", c.extractor())])?;
        let data = from_parquet(bytes.clone().into())?;
        extracts.push(json!({
            "shape": data.shape().as_str(), "rows": data.len(),
            "sha256": hex::encode(&sha(&bytes)[..8]),
        }));
        refs.push(ExtractRef {
            id: i64::try_from(i)? + 1,
            sha256: sha(&bytes),
            extractor: c.extractor().to_owned(),
            extractor_ver: c.version().to_owned(),
            artifacts: artifacts.clone(),
            data: Arc::new(data),
        });
    }
    let sctx = StdCtx::new(registry, profile, lookup);
    let mut sink = CollectSink::default();
    c.standardise(&sctx, &refs, &mut sink)
        .context("standardise")?;
    let mut warnings = sctx.take_warnings();
    if lookup.units.is_empty() {
        warnings.push(
            "no unit lookup (database not reachable): name matching and unit checks are skipped"
                .into(),
        );
    }
    let today = chrono::Utc::now().date_naive();
    let mut outputs = Vec::new();
    for o in sink.finish()? {
        match o {
            Output::Observations {
                dataset_id,
                vintage,
                rows,
            } => {
                let exps = expect::for_dataset(registry, &dataset_id)?;
                let class = registry
                    .dataset(&dataset_id)
                    .map_or_else(|| "A".to_owned(), |d| d.publication_class.clone());
                let mut results = expect::check_observations(
                    &exps,
                    &rows,
                    &ObsCtx {
                        registry,
                        profile,
                        lookup,
                        class: &class,
                        prev_rows: None,
                        today,
                    },
                );
                for e in &exps {
                    if matches!(e.check, expect::Check::Sql { .. }) {
                        results.push(CheckResult {
                            check: "sql",
                            severity: e.severity_for(&class),
                            passed: true,
                            skipped: true,
                            failures: 0,
                            detail: "skipped: runs at staging".into(),
                            note: e.note.clone(),
                        });
                    }
                }
                outputs.push(OutputSummary {
                    dataset: dataset_id,
                    kind: "observations",
                    vintage,
                    rows: rows.len(),
                    first: rows
                        .iter()
                        .take(first)
                        .map(|r| {
                            json!({
                                "indicator": r.indicator_id, "geo": r.geo_code, "period": r.period,
                                "dims": r.dims, "value": r.value, "lo": r.lo, "hi": r.hi,
                                "basis": r.basis.as_str(), "status": r.status.as_str(),
                                "verbatim": r.verbatim, "locator": r.locator,
                            })
                        })
                        .collect(),
                    expectations: results,
                });
            }
            Output::Features {
                dataset_id,
                vintage,
                rows,
            } => {
                let exps = expect::for_dataset(registry, &dataset_id)?;
                let class = registry
                    .dataset(&dataset_id)
                    .map_or_else(|| "A".to_owned(), |d| d.publication_class.clone());
                let results =
                    expect::check_geometry(&exps, &expect::feature_items(&rows), profile, &class);
                outputs.push(OutputSummary {
                    dataset: dataset_id,
                    kind: "features",
                    vintage,
                    rows: rows.len(),
                    first: rows
                        .iter()
                        .take(first)
                        .map(|f| {
                            json!({
                                "key": f.key, "kind": f.kind, "name": f.name_en,
                                "geometry": f.geometry, "attrs": f.attrs,
                            })
                        })
                        .collect(),
                    expectations: results,
                });
            }
            Output::AdminUnits {
                dataset_id, units, ..
            } => outputs.push(OutputSummary {
                dataset: dataset_id,
                kind: "admin_units",
                vintage: None,
                rows: units.len(),
                first: Vec::new(),
                expectations: Vec::new(),
            }),
            Output::Crosswalks { source_id, rows } => outputs.push(OutputSummary {
                dataset: source_id,
                kind: "crosswalks",
                vintage: None,
                rows: rows.len(),
                first: Vec::new(),
                expectations: Vec::new(),
            }),
        }
    }
    Ok(TestReport {
        source: source.to_owned(),
        driver: c.extractor().to_owned(),
        artifacts: fetched
            .iter()
            .map(|f| json!({ "key": f.key, "url": f.url, "bytes": f.bytes }))
            .collect(),
        extracts,
        outputs,
        warnings,
    })
}
