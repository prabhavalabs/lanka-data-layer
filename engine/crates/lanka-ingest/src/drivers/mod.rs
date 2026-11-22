//! Declarative drivers (03 §2.2, ADR 0008): generic connectors implemented
//! once in Rust and configured per source by `registry/connectors/<id>.yaml`.
//!
//! WP-02 ships `http_json` (REST/JSON APIs); the other drivers of the table
//! in 03 §2.2 follow in WP-07 behind the same lookup.

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::connector::Connector;
use crate::registry::{ConnectorSpec, Loaded};

pub mod feature_rules;
pub mod geo_file;
pub mod http_json;
pub mod json_path;
pub mod osm_pbf;
pub mod raster;

/// The driver a connector spec names.
pub fn driver_for(spec: &Loaded<ConnectorSpec>) -> Result<Arc<dyn Connector>> {
    match spec.spec.driver.as_str() {
        "http_json" => Ok(Arc::new(http_json::HttpJson::from_spec(spec)?)),
        "osm_pbf" => Ok(Arc::new(osm_pbf::OsmPbf::from_spec(spec)?)),
        "geo_file" => Ok(Arc::new(geo_file::GeoFile::from_spec(spec)?)),
        "raster" => Ok(Arc::new(raster::Raster::from_spec(spec)?)),
        d => bail!(
            "{}: driver {d:?} is not implemented (available: http_json, osm_pbf, geo_file, raster)",
            spec.path.display()
        ),
    }
}
