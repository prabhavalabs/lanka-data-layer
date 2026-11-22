//! Lanka Data Layer enrichment (04-enrichment.md, ADR 0010).
//!
//! - [`method`]: the `Method` contract, rows, inputs, sinks, contributors.
//! - [`methods`]: the library — `rollup`, `ratio`, `features_zonal`,
//!   `unit_metrics` (WP-06); `grid_zonal` plugs in through [`zonal`] (WP-05).
//! - [`recipe`]: `registry/recipes/*.yaml`, schema validation, type checks.
//! - [`dag`]: the global recipe DAG (topological order, downstream sets).
//! - [`runner`]: steps → propagation → emit → QA; fingerprints; explain.
//! - [`propagate`]: basis/status/flag rules the runner enforces.
//! - [`pg`]: inputs over pinned Postgres versions; [`provenance`]: lineage
//!   of derived observations for the provenance endpoint.
//!
//! Publication (stage → derivation record → gate) lives in
//! `lanka-ingest::derive`, next to the staging path it reuses.

pub mod dag;
pub mod expr;
pub mod method;
pub mod methods;
pub mod pg;
pub mod propagate;
pub mod provenance;
pub mod recipe;
pub mod runner;
pub mod zonal;

pub use method::{Inputs, MemInputs, Method, MethodCtx, MethodInfo, OutputSink, Row, RowKey};
pub use methods::Methods;
pub use recipe::{RecipeSpec, Recipes};
