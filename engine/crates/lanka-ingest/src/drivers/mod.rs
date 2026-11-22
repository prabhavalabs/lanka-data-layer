//! Declarative drivers (03 §2.2, ADR 0008): generic connectors implemented
//! once in Rust and configured per source by `registry/connectors/<id>.yaml`.
//!
//! `http_json` (REST/JSON APIs: the per-key form of WP-02, or the table form
//! of `tabular.rs`), the tabular drivers `csv`, `sdmx`, `ckan`, `xlsx_table`,
//! `html_table` and `git_files` (WP-07, one implementation in `tabular.rs`
//! with the selector language of `table.rs`), and the spatial drivers
//! `osm_pbf`, `geo_file` and `raster`, and `pdf_table` (WP-08: PDF tables
//! through registry templates and the document sandbox). `gtfs` follows.

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::connector::Connector;
use crate::registry::{ConnectorSpec, Loaded};

pub mod feature_rules;
pub mod geo_file;
pub mod http_json;
pub mod json_path;
pub mod osm_pbf;
pub mod pdf_table;
pub mod raster;
pub mod table;
pub mod tabular;

/// The driver a connector spec names.
pub fn driver_for(spec: &Loaded<ConnectorSpec>) -> Result<Arc<dyn Connector>> {
    match spec.spec.driver.as_str() {
        // The per-key WDI form has `extract.records`; the table form has
        // `standardise.outputs`.
        "http_json" if spec.json["extract"].get("records").is_some() => {
            Ok(Arc::new(http_json::HttpJson::from_spec(spec)?))
        }
        "http_json" | "csv" | "sdmx" | "ckan" | "xlsx_table" | "html_table" | "git_files" => {
            Ok(Arc::new(tabular::Tabular::from_spec(spec)?))
        }
        "osm_pbf" => Ok(Arc::new(osm_pbf::OsmPbf::from_spec(spec)?)),
        "geo_file" => Ok(Arc::new(geo_file::GeoFile::from_spec(spec)?)),
        "raster" => Ok(Arc::new(raster::Raster::from_spec(spec)?)),
        "pdf_table" => Ok(Arc::new(pdf_table::PdfTable::from_spec(spec)?)),
        d => bail!(
            "{}: driver {d:?} is not implemented (available: http_json, csv, sdmx, ckan, xlsx_table, html_table, git_files, osm_pbf, geo_file, raster, pdf_table)",
            spec.path.display()
        ),
    }
}
