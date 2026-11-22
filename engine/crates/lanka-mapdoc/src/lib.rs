//! `lanka-mapdoc` — the open map document `ldl-map/1` (10 §2, ADR 0018).
//!
//! - [`doc`]: serde types; `x-*` keys survive a parse → serialise round trip.
//! - [`validate`]: the JSON Schema (`spec/map/1/map.schema.json`, embedded)
//!   plus the rules it cannot express, with JSON-pointer paths.
//! - [`canon`]: RFC 8785 canonical JSON, the content hash, snapshot ids.
//! - [`refs`]: what a document depends on (`app.map_refs`).
//! - [`resolve`]: resolution against the catalogue — versions, data URLs,
//!   attribution, notices, problems.
//! - [`ramps`]: the ramp and tone registry (`spec/map/1/ramps.json`).
//! - [`ts`]: TypeScript declarations generated from the schema.

pub mod canon;
pub mod doc;
pub mod ramps;
pub mod refs;
pub mod resolve;
pub mod ts;
pub mod validate;

pub use doc::{MapDocument, SPEC};
pub use validate::{Problem, validate, validate_str};
