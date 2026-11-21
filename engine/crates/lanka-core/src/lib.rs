//! Lanka Data Layer query core.
//!
//! One implementation of every read, shared by the REST v2 server, the `lanka`
//! CLI and (next) the MCP transport: canonical grid math, p-code types, the
//! response envelope, the catalog snapshot and the Postgres queries.

pub mod catalog;
pub mod db;
pub mod envelope;
pub mod error;
pub mod grid;
pub mod pcode;
pub mod period;
pub mod query;

pub use catalog::Catalog;
pub use envelope::{CONTRACT_VERSION, Envelope, Meta};
pub use error::{CoreError, CoreResult};
pub use pcode::Pcode;

/// Dataset id of the admin boundaries every area response relies on.
pub const BOUNDARY_DATASET: &str = "cod-ab-lka-boundaries";

/// Engine version string recorded on every ingestion run.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
