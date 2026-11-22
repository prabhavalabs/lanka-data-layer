//! `unit_metrics@1` — attributes of the admin units themselves: geodesic
//! area (`geo.unit_geoms.area_km2`, `ST_Area(geom::geography)` of the
//! reference boundary set) and the number of child units.
//!
//! ```yaml
//! with:
//!   boundaries: admin
//!   period: "2024"
//!   area: admin.area_km2          # output indicator for areas (optional)
//!   child_count: admin.child_count  # output indicator for child counts (optional)
//!   levels: [0, 1, 2, 3, 4]
//! ```
//!
//! Child counts are emitted for levels that have a level below them.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lanka_core::{Basis, Flags, Freq, Period};
use serde_json::{Value, json};

use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortSpec, PortUse, Role, Row, UncertaintyKind, Version, param_levels, param_str,
};

pub struct UnitMetrics;

static INFO: MethodInfo = MethodInfo {
    id: "unit_metrics",
    version: Version::new(1, 0, 0),
    summary: "Admin unit attributes: geodesic area and number of child units",
    citation: "PostGIS ST_Area(geography) on the reference boundary set",
    inputs: &[PortSpec {
        name: "boundaries",
        kind: PortKind::Boundaries,
        required: true,
    }],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false, "required": ["boundaries", "period", "levels"],
      "properties": {
        "boundaries": {"type": "string"},
        "period": {"type": "string"},
        "area": {"type": "string"},
        "child_count": {"type": "string"},
        "levels": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 1}
      }
    }"#,
    output_basis: Basis::Computed,
    uncertainty: UncertaintyKind::None,
    mandatory_flags: Flags::EMPTY,
    tools: &[],
    partial_recompute: false,
};

#[async_trait]
impl Method for UnitMetrics {
    fn info(&self) -> &MethodInfo {
        &INFO
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        Ok(vec![PortUse {
            reference: param_str(params, "boundaries")?.to_owned(),
            kind: PortKind::Boundaries,
            indicators: Vec::new(),
        }])
    }

    async fn run(
        &self,
        ctx: &MethodCtx<'_>,
        params: &Value,
        out: &mut dyn OutputSink,
    ) -> Result<MethodReport> {
        let port: Arc<str> = Arc::from(param_str(params, "boundaries")?);
        let area = params.get("area").and_then(Value::as_str);
        let child = params.get("child_count").and_then(Value::as_str);
        if area.is_none() && child.is_none() {
            bail!("unit_metrics: give area and/or child_count");
        }
        let period = Period::parse(
            Freq::P,
            param_str(params, "period")?,
            ctx.inputs.calendars(),
        )
        .context("unit_metrics: period")?;
        let levels = param_levels(params, "levels");
        let units = ctx.inputs.units().await?;
        let deepest = units.list.iter().map(|u| u.level).max().unwrap_or(0);
        let index: std::collections::HashMap<&str, u32> = units
            .list
            .iter()
            .enumerate()
            .map(|(i, u)| (u.code.as_str(), u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        let mut n = 0u64;
        let mut no_area = 0u64;
        for &level in &levels {
            for u in units.at_level(level) {
                let me = Contrib {
                    port: port.clone(),
                    kind: ContribKind::Unit,
                    idx: index[u.code.as_str()],
                    role: Role::Geometry,
                    weight: 1.0,
                };
                if let Some(ind) = area {
                    if u.area_km2.is_none() {
                        no_area += 1;
                    }
                    out.row(OutRow {
                        row: Row::computed(
                            ind,
                            &u.code,
                            u.level,
                            period.freq,
                            period.start,
                            "",
                            u.area_km2,
                        ),
                        contrib: vec![me],
                    })?;
                    n += 1;
                }
                if let Some(ind) = child
                    && level < deepest
                {
                    let kids: Vec<Contrib> = units
                        .children(&u.code)
                        .map(|c| Contrib {
                            port: port.clone(),
                            kind: ContribKind::Unit,
                            idx: index[c.code.as_str()],
                            role: Role::SourceUnit,
                            weight: 1.0,
                        })
                        .collect();
                    #[allow(clippy::cast_precision_loss)]
                    let v = kids.len() as f64;
                    out.row(OutRow {
                        row: Row::computed(
                            ind,
                            &u.code,
                            u.level,
                            period.freq,
                            period.start,
                            "",
                            Some(v),
                        ),
                        contrib: kids,
                    })?;
                    n += 1;
                }
            }
        }
        Ok(MethodReport {
            rows: n,
            diagnostics: json!({ "units_without_area": no_area }),
        })
    }

    fn formula(&self, _params: &Value, out: &OutRow) -> String {
        if out
            .contrib
            .first()
            .is_some_and(|c| c.role == Role::Geometry)
        {
            format!(
                "{}[{}] = ST_Area(geom::geography) / 1e6",
                out.row.indicator, out.row.geo
            )
        } else {
            format!(
                "{}[{}] = count(children) = {}",
                out.row.indicator,
                out.row.geo,
                out.contrib.len()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;
    use lanka_core::Calendars;

    use super::*;
    use crate::method::{CollectSink, MemInputs};
    use crate::methods::rollup::tests::tree;

    #[tokio::test]
    async fn areas_and_child_counts() {
        let inp = MemInputs {
            units: Arc::new(tree(&[2, 3])),
            calendars: Calendars::default(),
            indicators: HashMap::new(),
            ..MemInputs::default()
        };
        let ctx = MethodCtx {
            inputs: &inp,
            rng_seed: 0,
            as_of: NaiveDate::from_ymd_opt(2024, 1, 1).expect("date"),
            changed_keys: None,
        };
        let mut sink = CollectSink::default();
        UnitMetrics
            .run(
                &ctx,
                &json!({"boundaries": "admin", "period": "2024", "area": "a", "child_count": "c",
                        "levels": [0, 1, 2]}),
                &mut sink,
            )
            .await
            .expect("run");
        let c = |g: &str| {
            sink.rows
                .iter()
                .find(|r| r.row.indicator == "c" && r.row.geo == g)
                .and_then(|r| r.row.value)
        };
        assert_eq!(c("ZZ"), Some(2.0));
        assert_eq!(c("ZZ1"), Some(3.0));
        assert_eq!(c("ZZ11"), None, "no child count at the deepest level");
        assert_eq!(
            sink.rows.iter().filter(|r| r.row.indicator == "a").count(),
            9
        );
    }
}
