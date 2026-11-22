//! The connector contract, SDK v2 (03-ingestion.md §2).
//!
//! A connector knows one upstream source. It *discovers* what to fetch (and a
//! fingerprint of upstream state), *extracts* archived bytes into an L1
//! extract (no network, no database) and *standardises* extracts into
//! standard-model outputs (pure: extract + registry + a read-only lookup
//! snapshot). Fetching, archiving, persisting extracts, validation, staging,
//! the review gate and publishing are done uniformly by the pipeline.
//!
//! Stages: discover → fetch → archive (L0) → extract (L1) → standardise →
//! validate → stage → gate → publish/review.
//!
//! Two kinds of connector implement the trait: **drivers** configured by
//! `registry/connectors/<source>.yaml` (`drivers/`), and **code connectors**
//! for irregular sources (`connectors/`). [`connector_for`] picks the driver
//! when the registry has a spec for the source, else the code connector.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use bytes::Bytes;
use lanka_core::{Basis, Flags, Method, Profile, Status};
use serde_json::value::RawValue;

use crate::extract::{ExtractOut, ExtractRef};
use crate::registry::Registry;
use crate::run::RunTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
}

/// What to fetch: a URL, or a local file standing in for one.
#[derive(Debug, Clone, Default)]
pub struct Candidate {
    /// Connector-specific key (`A5`, `SP.POP.TOTL`, `adm3` …).
    pub key: String,
    /// Canonical upstream URL (recorded on the artifact even for local files).
    pub url: String,
    /// Read this file instead of fetching `url`.
    pub local_path: Option<PathBuf>,
    pub media_type: Option<String>,
    /// Edition label if known before fetching.
    pub edition: Option<String>,
    pub method: HttpMethod,
    pub body: Option<Bytes>,
    pub headers: Vec<(String, String)>,
}

/// The result of `discover`.
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub candidates: Vec<Candidate>,
    /// Hash of everything that would change if upstream changed (URLs,
    /// `lastupdated` stamps, resource ids). `None` = always fetch. When every
    /// candidate is a local file the pipeline derives one from file metadata.
    pub fingerprint: Option<[u8; 32]>,
}

/// An archived artifact handed to `extract`: a path, not bytes — large
/// files are never held in RAM by the pipeline.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub key: String,
    pub url: String,
    pub artifact_id: i64,
    pub sha256: [u8; 32],
    pub path: PathBuf,
    pub bytes: u64,
    pub media_type: Option<String>,
}

impl Fetched {
    /// Reads the archived bytes (for parsers that need them in memory).
    pub fn read(&self) -> Result<Vec<u8>> {
        std::fs::read(&self.path).with_context(|| format!("reading {}", self.path.display()))
    }
}

/// Where an observation came from inside the L1 extracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellProv {
    /// Index into the `extracts` slice given to `standardise`.
    pub extract: usize,
    /// Row index inside that extract.
    pub row: u32,
}

/// One observation with row-level provenance, as a connector reads it:
/// codes, not keys (the pipeline resolves surrogate keys and the admin
/// level from the catalogue and `geo.units`, never from the connector).
#[derive(Debug, Clone)]
pub struct ObsRow {
    pub indicator_id: String,
    /// p-code or `scheme:code`.
    pub geo_code: String,
    /// Canonical period code of the indicator's frequency.
    pub period: String,
    /// Canonical `k=v;k=v` with keys sorted; empty when undimensioned.
    pub dims: String,
    pub value: Option<f64>,
    /// Interval bounds (mandatory for `estimated` indicators).
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub basis: Basis,
    /// As the source states it. `revised` is assigned by the pipeline, never
    /// by a connector (see `publish`).
    pub status: Status,
    pub flags: Flags,
    /// Index into the run's artifact list (`Fetched` slice / the extracts'
    /// `artifact_idx`) the row came from.
    pub artifact: usize,
    /// Extract row the value was read from.
    pub prov: Option<CellProv>,
    pub locator: serde_json::Value,
    pub verbatim: Option<String>,
    pub method: Method,
    pub confidence: Option<f32>,
}

impl ObsRow {
    /// A reported, normal, unflagged observation (the common case).
    #[must_use]
    pub fn reported(
        indicator_id: impl Into<String>,
        geo_code: impl Into<String>,
        period: impl Into<String>,
        dims: String,
        value: Option<f64>,
    ) -> Self {
        Self {
            indicator_id: indicator_id.into(),
            geo_code: geo_code.into(),
            period: period.into(),
            dims,
            value,
            lo: None,
            hi: None,
            basis: Basis::Reported,
            status: Status::Normal,
            flags: Flags::EMPTY,
            artifact: 0,
            prov: None,
            locator: serde_json::Value::Null,
            verbatim: None,
            method: Method::Api,
            confidence: Some(1.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminUnitIn {
    pub pcode: String,
    pub level: i16,
    pub name_en: String,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    pub parent_pcode: Option<String>,
    /// GeoJSON geometry, passed through untouched to PostGIS.
    pub geometry: Box<RawValue>,
    pub artifact: usize,
}

#[derive(Debug, Clone)]
pub struct BoundarySet {
    pub id: String,
    pub title: String,
    pub vintage: Option<String>,
    pub notes: Option<String>,
}

/// One feature (point, line or polygon) as a connector delivers it: codes
/// and text, never keys. The pipeline derives `cell_id` (from `rep`) and
/// `unit_key` (deepest admin unit containing `rep`) at staging.
#[derive(Debug, Clone)]
pub struct FeatureIn {
    /// Upstream key, stable while upstream keeps it (`way/123456789`).
    pub key: String,
    pub kind: String,
    pub name_en: Option<String>,
    pub name_si: Option<String>,
    pub name_ta: Option<String>,
    /// GeoJSON geometry, coordinates with at most 6 decimals
    /// ([`crate::geom::Geometry::to_geojson6`]).
    pub geometry: String,
    /// Representative point `[lon, lat]` (on the geometry).
    pub rep: crate::geom::Coord,
    /// Typed attributes (declared `fields` of the dataset).
    pub attrs: serde_json::Map<String, serde_json::Value>,
    pub flags: Flags,
    /// Conflated entity (`lke_…`), when known.
    pub entity_id: Option<String>,
    /// Index into the run's artifact list.
    pub artifact: usize,
    pub prov: Option<CellProv>,
}

#[derive(Debug, Clone)]
pub struct CrosswalkRow {
    pub from_set: String,
    pub from_code: String,
    pub to_set: String,
    pub to_code: String,
    pub weight: f64,
    pub method: &'static str,
}

/// What a run publishes. One run may produce several outputs.
#[derive(Debug)]
pub enum Output {
    Observations {
        dataset_id: String,
        vintage: Option<String>,
        rows: Vec<ObsRow>,
    },
    AdminUnits {
        dataset_id: String,
        boundary_set: BoundarySet,
        units: Vec<AdminUnitIn>,
    },
    Crosswalks {
        source_id: String,
        rows: Vec<CrosswalkRow>,
    },
    Features {
        dataset_id: String,
        vintage: Option<String>,
        rows: Vec<FeatureIn>,
    },
}

#[derive(Debug, Clone)]
pub enum OutputKind {
    Observations,
    AdminUnits(BoundarySet),
    Crosswalks,
    Features,
}

/// Opens one output in the sink.
#[derive(Debug, Clone)]
pub struct OutputHeader {
    /// Dataset id (observations, units) or source id (crosswalks).
    pub dataset_id: String,
    pub kind: OutputKind,
    pub vintage: Option<String>,
}

/// Streaming sink: `standardise` writes rows as it produces them.
pub trait OutputSink {
    fn begin(&mut self, out: OutputHeader) -> Result<()>;
    fn obs(&mut self, row: ObsRow) -> Result<()>;
    fn unit(&mut self, row: AdminUnitIn) -> Result<()>;
    fn crosswalk(&mut self, row: CrosswalkRow) -> Result<()>;
    fn feature(&mut self, row: FeatureIn) -> Result<()> {
        bail!("this sink takes no features ({})", row.key)
    }
    fn end(&mut self) -> Result<()>;
}

/// The pipeline's sink: collects outputs for validation and staging (both
/// need the whole set of a dataset version).
#[derive(Debug, Default)]
pub struct CollectSink {
    pub outputs: Vec<Output>,
    open: Option<Output>,
}

impl CollectSink {
    pub fn finish(mut self) -> Result<Vec<Output>> {
        if self.open.is_some() {
            bail!("standardise did not end its last output");
        }
        Ok(std::mem::take(&mut self.outputs))
    }
}

impl OutputSink for CollectSink {
    fn begin(&mut self, h: OutputHeader) -> Result<()> {
        if self.open.is_some() {
            bail!(
                "output {} begun before the previous one ended",
                h.dataset_id
            );
        }
        self.open = Some(match h.kind {
            OutputKind::Observations => Output::Observations {
                dataset_id: h.dataset_id,
                vintage: h.vintage,
                rows: Vec::new(),
            },
            OutputKind::AdminUnits(set) => Output::AdminUnits {
                dataset_id: h.dataset_id,
                boundary_set: set,
                units: Vec::new(),
            },
            OutputKind::Crosswalks => Output::Crosswalks {
                source_id: h.dataset_id,
                rows: Vec::new(),
            },
            OutputKind::Features => Output::Features {
                dataset_id: h.dataset_id,
                vintage: h.vintage,
                rows: Vec::new(),
            },
        });
        Ok(())
    }

    fn obs(&mut self, row: ObsRow) -> Result<()> {
        match &mut self.open {
            Some(Output::Observations { rows, .. }) => {
                rows.push(row);
                Ok(())
            }
            _ => bail!("obs() outside an observations output"),
        }
    }

    fn unit(&mut self, row: AdminUnitIn) -> Result<()> {
        match &mut self.open {
            Some(Output::AdminUnits { units, .. }) => {
                units.push(row);
                Ok(())
            }
            _ => bail!("unit() outside an admin-units output"),
        }
    }

    fn crosswalk(&mut self, row: CrosswalkRow) -> Result<()> {
        match &mut self.open {
            Some(Output::Crosswalks { rows, .. }) => {
                rows.push(row);
                Ok(())
            }
            _ => bail!("crosswalk() outside a crosswalk output"),
        }
    }

    fn feature(&mut self, row: FeatureIn) -> Result<()> {
        match &mut self.open {
            Some(Output::Features { rows, .. }) => {
                rows.push(row);
                Ok(())
            }
            _ => bail!("feature() outside a features output"),
        }
    }

    fn end(&mut self) -> Result<()> {
        let o = self.open.take().context("end() without begin()")?;
        self.outputs.push(o);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Read inputs from this directory instead of the network (where the
    /// connector supports it).
    pub from_dir: Option<PathBuf>,
    pub trigger: String,
    /// Skip the fingerprint and artifact-set short-circuits.
    pub force: bool,
    /// The worker job executing this run (`ops.jobs.id`), if any.
    pub job_id: Option<i64>,
}

/// `discover` may use the network (cheap listing endpoints, HEAD requests).
pub struct DiscoverCtx<'a> {
    pub registry: &'a Registry,
    pub profile: &'a Profile,
    pub http: &'a reqwest::Client,
    pub opts: &'a RunOptions,
    pub run: Option<&'a RunTracker>,
}

/// `extract` reads archived files only: no network, no database.
pub struct ExtractCtx<'a> {
    pub registry: &'a Registry,
    pub profile: &'a Profile,
}

/// A coded geography in the lookup snapshot.
#[derive(Debug, Clone)]
pub struct UnitRef {
    pub code: String,
    pub level: i16,
    pub name_en: String,
    pub parent: Option<String>,
}

/// Read-only snapshot loaded once per run for `standardise` (which gets no
/// database handle).
#[derive(Debug, Clone, Default)]
pub struct Lookup {
    /// Current (not retired) units of the profile's admin scheme.
    pub units: Vec<UnitRef>,
    /// Land area of the country, buffered by the snap distance (features
    /// whose representative point falls outside are not the country's).
    pub coverage: Option<Arc<crate::geom::Coverage>>,
}

/// `standardise` is pure: extract + registry + lookup.
pub struct StdCtx<'a> {
    pub registry: &'a Registry,
    pub profile: &'a Profile,
    pub lookup: &'a Lookup,
    warnings: Mutex<Vec<String>>,
}

impl<'a> StdCtx<'a> {
    #[must_use]
    pub fn new(registry: &'a Registry, profile: &'a Profile, lookup: &'a Lookup) -> Self {
        Self {
            registry,
            profile,
            lookup,
            warnings: Mutex::new(Vec::new()),
        }
    }

    /// Records a non-fatal finding; the pipeline writes it to the run log.
    pub fn warn(&self, msg: impl Into<String>) {
        if let Ok(mut w) = self.warnings.lock() {
            w.push(msg.into());
        }
    }

    #[must_use]
    pub fn take_warnings(&self) -> Vec<String> {
        self.warnings
            .lock()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default()
    }
}

// async_trait marks the boxed futures #[must_use]; clippy then flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Connector: Send + Sync {
    fn source_id(&self) -> &str;
    /// Semver; part of every extract and run record.
    fn version(&self) -> &str;
    /// Extractor name recorded on extracts (driver id or connector id).
    fn extractor(&self) -> &str {
        self.source_id()
    }
    /// sha256 of the connector's spec file, if it has one (drivers).
    fn spec_sha256(&self) -> Option<[u8; 32]> {
        None
    }
    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery>;
    /// Bytes → extract tables. No network, no database.
    async fn extract(&self, ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>>;
    /// Extract → standard-model outputs. Pure.
    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()>;
}

/// Code connectors, by source id (the registry's `connectors/` specs take
/// precedence: a source with a driver spec uses the driver).
#[must_use]
fn code_connector(source_id: &str) -> Option<Arc<dyn Connector>> {
    use crate::connectors::{cod_ab::CodAb, dcs_census::DcsCensus2024};
    match source_id {
        "dcs-census-2024" => Some(Arc::new(DcsCensus2024)),
        "hdx-cod-ab-lka" => Some(Arc::new(CodAb)),
        _ => None,
    }
}

/// Source ids with a code connector.
pub const CODE_CONNECTORS: &[&str] = &["dcs-census-2024", "hdx-cod-ab-lka"];

/// The connector for a source: the declarative driver named by
/// `registry/connectors/<source>.yaml`, else a code connector.
pub fn connector_for(registry: &Registry, source_id: &str) -> Result<Option<Arc<dyn Connector>>> {
    if let Some(spec) = registry.connectors.get(source_id) {
        return crate::drivers::driver_for(spec).map(Some);
    }
    Ok(code_connector(source_id))
}

/// Every source id that has a connector (driver spec or code).
#[must_use]
pub fn implemented(registry: &Registry) -> Vec<String> {
    let mut v: Vec<String> = registry.connectors.keys().cloned().collect();
    v.extend(CODE_CONNECTORS.iter().map(|s| (*s).to_owned()));
    v.sort();
    v.dedup();
    v
}

/// Canonical dims key from pairs (keys sorted, `k=v` joined by `;`).
pub use lanka_core::dims::dims_key;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_are_canonical() {
        assert_eq!(dims_key(&[]), "");
        assert_eq!(dims_key(&[("sex", "f"), ("age", "0-14")]), "age=0-14;sex=f");
    }

    #[test]
    fn sink_requires_begin_and_end() {
        let mut s = CollectSink::default();
        assert!(
            s.obs(ObsRow::reported("x", "LK", "2020", String::new(), None))
                .is_err()
        );
        s.begin(OutputHeader {
            dataset_id: "d".into(),
            kind: OutputKind::Observations,
            vintage: None,
        })
        .expect("begin");
        s.obs(ObsRow::reported(
            "x",
            "LK",
            "2020",
            String::new(),
            Some(1.0),
        ))
        .expect("obs");
        assert!(s.unit_is_rejected());
        s.end().expect("end");
        let out = s.finish().expect("finish");
        assert!(matches!(&out[0], Output::Observations { rows, .. } if rows.len() == 1));
    }

    impl CollectSink {
        fn unit_is_rejected(&mut self) -> bool {
            self.crosswalk(CrosswalkRow {
                from_set: String::new(),
                from_code: String::new(),
                to_set: String::new(),
                to_code: String::new(),
                weight: 1.0,
                method: "x",
            })
            .is_err()
        }
    }
}
