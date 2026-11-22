//! Lanka Data Layer query core.
//!
//! One implementation of every read, shared by the REST v2 server, the `lanka`
//! CLI and (next) the MCP transport, plus the standard-model types every
//! other crate uses: country profile, geography codes, periods, units,
//! basis/status/flags, dimensions and dataset-version hashing.

pub mod basis;
pub mod catalog;
pub mod db;
pub mod dims;
pub mod dsv;
pub mod envelope;
pub mod error;
pub mod features;
pub mod flags;
pub mod frame;
pub mod geo_code;
pub mod geo_units;
pub mod grid;
pub mod period;
pub mod profile;
pub mod query;
pub mod status;
pub mod units;

pub use basis::{Basis, IndicatorClass, Method, Review, Status};
pub use catalog::Catalog;
pub use dims::Dims;
pub use envelope::{CONTRACT_VERSION, Envelope, Meta};
pub use error::{CoreError, CoreResult};
pub use flags::Flags;
pub use geo_code::{AdminCodes, GeoCode, Pcode};
pub use period::{Calendars, Freq, Period};
pub use profile::Profile;

/// Engine version string recorded on every ingestion run.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Open-ended version range bound (`data.observations.vto` of a current row).
pub const VTO_OPEN: i32 = i32::MAX;

/// Git revision recorded on dataset versions (`LANKA_GIT_REV`, else `unknown`).
#[must_use]
pub fn git_rev() -> String {
    std::env::var("LANKA_GIT_REV")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}
