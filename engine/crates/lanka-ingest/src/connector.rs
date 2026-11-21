//! The connector contract.
//!
//! A connector knows one upstream source. It only *discovers* what to fetch
//! and *parses* archived bytes into typed outputs; fetching, archiving,
//! validation, staging, review gating and publishing are done uniformly by
//! the pipeline (`pipeline.rs`), so every source gets the same provenance,
//! monitoring and publication rules.
//!
//! Stages: discover → fetch → archive (sha256) → parse → validate → stage → publish.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use futures::future::BoxFuture;
use serde_json::value::RawValue;
use sqlx::PgPool;

use crate::archive::Archive;
use crate::registry::Registry;
use crate::run::RunTracker;

/// Something to fetch: a URL, or a local file standing in for one.
#[derive(Debug, Clone)]
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
}

/// An archived artifact, ready to parse.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub key: String,
    pub url: String,
    pub artifact_id: i64,
    pub sha256: [u8; 32],
    pub bytes: Bytes,
    pub media_type: Option<String>,
}

/// One observation with row-level provenance. `geo_level` and `review` are
/// filled in by the pipeline from the database and the publication class.
#[derive(Debug, Clone)]
pub struct ObsRow {
    pub indicator_id: String,
    pub geo_code: String,
    pub period: String,
    /// Canonical `k=v;k=v` with keys sorted; empty when undimensioned.
    pub dims: String,
    pub value: Option<f64>,
    pub status: &'static str,
    /// Index into the `Fetched` slice the row came from.
    pub artifact: usize,
    pub locator: serde_json::Value,
    pub verbatim: Option<String>,
    pub method: &'static str,
    pub confidence: Option<f32>,
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

#[derive(Debug, Clone)]
pub struct CrosswalkRow {
    pub from_set: String,
    pub from_code: String,
    pub to_set: String,
    pub to_code: String,
    pub weight: f64,
    pub method: &'static str,
}

/// What a parse produces. One run may produce several outputs.
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
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Read inputs from this directory instead of the network (where the
    /// connector supports it).
    pub from_dir: Option<PathBuf>,
    pub trigger: String,
}

/// Everything a connector may use. No connector writes to the database
/// directly; outputs go through the pipeline.
pub struct Ctx<'a> {
    pub pool: &'a PgPool,
    pub registry: &'a Registry,
    pub archive: &'a Archive,
    pub http: &'a reqwest::Client,
    pub run: &'a RunTracker,
    pub opts: &'a RunOptions,
}

pub trait Connector: Send + Sync {
    fn source_id(&self) -> &'static str;

    /// Lists what to fetch. May call cheap upstream listing endpoints.
    fn discover<'a>(&'a self, ctx: &'a Ctx<'a>) -> BoxFuture<'a, Result<Vec<Candidate>>>;

    /// Parses archived bytes. Must not touch the network.
    fn parse<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        fetched: &'a [Fetched],
    ) -> BoxFuture<'a, Result<Vec<Output>>>;
}

/// Registered connectors, by source id.
#[must_use]
pub fn connector_for(source_id: &str) -> Option<Arc<dyn Connector>> {
    use crate::connectors::{cod_ab::CodAb, dcs_census::DcsCensus2024, world_bank::WorldBankWdi};
    match source_id {
        "worldbank-wdi" => Some(Arc::new(WorldBankWdi)),
        "dcs-census-2024" => Some(Arc::new(DcsCensus2024)),
        "hdx-cod-ab-lka" => Some(Arc::new(CodAb)),
        _ => None,
    }
}

/// Source ids that have a connector implementation.
pub const IMPLEMENTED: &[&str] = &["dcs-census-2024", "hdx-cod-ab-lka", "worldbank-wdi"];

/// Canonical dims key from pairs: keys sorted, `k=v` joined by `;`.
#[must_use]
pub fn dims_key(pairs: &[(&str, &str)]) -> String {
    let mut p: Vec<(&str, &str)> = pairs.to_vec();
    p.sort_unstable();
    p.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_are_canonical() {
        assert_eq!(dims_key(&[]), "");
        assert_eq!(dims_key(&[("sex", "f"), ("age", "0-14")]), "age=0-14;sex=f");
    }
}
