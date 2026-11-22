//! The method library (04 §3). WP-06 ships `rollup`, `ratio`,
//! `features_zonal` and `unit_metrics`; WP-05 adds `grid_zonal` through
//! [`crate::zonal::GridZonal`].

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::method::{Method, VersionReq, parse_method_ref};

pub mod features_zonal;
pub mod ratio;
pub mod rollup;
pub mod unit_metrics;

/// Every method this engine version implements.
#[derive(Clone)]
pub struct Methods {
    list: Vec<Arc<dyn Method>>,
}

impl Methods {
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            list: vec![
                Arc::new(rollup::Rollup),
                Arc::new(ratio::Ratio),
                Arc::new(features_zonal::FeaturesZonal),
                Arc::new(unit_metrics::UnitMetrics),
            ],
        }
    }

    /// Adds a method (WP-05's `grid_zonal`, tests).
    #[must_use]
    pub fn with(mut self, m: Arc<dyn Method>) -> Self {
        self.list.push(m);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Method>> {
        self.list.iter()
    }

    /// The highest version of `id` satisfying `req`.
    pub fn resolve_req(&self, id: &str, req: VersionReq) -> Result<Arc<dyn Method>> {
        let best = self
            .list
            .iter()
            .filter(|m| m.info().id == id && req.matches(m.info().version))
            .max_by_key(|m| m.info().version);
        match best {
            Some(m) => Ok(m.clone()),
            None if self.list.iter().any(|m| m.info().id == id) => bail!(
                "method {id}: no implemented version matches the requirement (have {})",
                self.list
                    .iter()
                    .filter(|m| m.info().id == id)
                    .map(|m| m.info().version.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => bail!("unknown method {id:?}"),
        }
    }

    /// Resolves `rollup@^1`.
    pub fn resolve(&self, reference: &str) -> Result<Arc<dyn Method>> {
        let (id, req) = parse_method_ref(reference)?;
        self.resolve_req(id, req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_by_semver() {
        let m = Methods::builtin();
        assert_eq!(m.resolve("rollup@^1").expect("r").info().id, "rollup");
        assert!(m.resolve("rollup@^2").is_err());
        assert!(m.resolve("nope@^1").is_err());
        for x in m.iter() {
            let schema: serde_json::Value =
                serde_json::from_str(x.info().params).expect("params schema is JSON");
            jsonschema::draft202012::new(&schema).expect("params schema compiles");
        }
    }
}
