//! Lanka Data Layer analysis engine (11-analysis-engine.md, ADR 0021/0022).
//!
//! A request (`ldl-analysis/1`) runs through a fixed pipeline:
//!
//! ```text
//! request ─► RESOLVE (pin → release, ids → dataset versions, defaults explicit)
//!         ─► PLAN    (support: aggregate up, never down; alignment per variable)
//!         ─► fingerprint → stored?  ── yes ─► the stored bytes (same id, same bytes)
//!         ─► ALIGN   (frames, rollup / inherit, grid_zonal, features_zonal, exposure_overlay)
//!         ─► GUARD   (block → no estimates) ─► COMPUTE (lanka-stats kernels, seeded)
//!         ─► PRESENT (ldl-analysis-result/1, outputs, templated interpretation, citation)
//! ```
//!
//! - [`request`]: request types + the embedded `spec/analysis/1` schemas.
//! - [`pipeline`]: resolve, plan, align (reads only; never writes `data.*`).
//! - [`frame`]: the aligned frame. [`method`] / [`methods`]: the tier-1
//!   methods (`describe`, `relate`, `regress`, `group_compare`, `exposure`,
//!   `zonal`, `classify`). [`guards`]: the registry. [`result`]: guards,
//!   fingerprint, the result document. [`engine`]: sync executor + store.
//! - [`bundle`]: the result bundle (zip).

pub mod bundle;
pub mod engine;
pub mod frame;
pub mod guards;
pub mod method;
pub mod methods;
pub mod pipeline;
pub mod request;
pub mod result;

pub use engine::{Engine, EngineError, Ran};
pub use request::Request;

#[cfg(test)]
mod tests;
