//! `features_zonal@1` — point/line/polygon features → counts, lengths and
//! areas per admin unit.
//!
//! ```yaml
//! with:
//!   features: pois                    # a features port (WP-04's geo.features shape)
//!   measure: count                    # count | length_km | area_km2
//!   kinds: [hospital, clinic, school] # optional filter on feature kind
//!   attrs: { operator_type: public }  # optional equality filter on attrs
//!   dim: { id: amenity_type, from: kind, total: total }   # optional breakdown
//!   indicator: derived.facility_count
//!   period: "2026"
//!   levels: [1, 2, 3, 4]
//!   zeros: true                       # units without features get 0
//! ```
//!
//! A feature counts towards its `unit` (the deepest admin unit, by point on
//! surface for lines and polygons) and every ancestor. Lines and polygons
//! are attributed whole to that unit (no clipping at unit boundaries in
//! v1). Flags of the features (`UNOFFICIAL_GEOM`) pass through to the rows
//! they contribute to; a unit with no features carries no flags.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lanka_core::{Basis, Dims, Flags, Freq, Period};
use serde_json::{Value, json};

use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortSpec, PortUse, Role, Row, UncertaintyKind, Version, param_levels, param_str,
    param_strs,
};

pub struct FeaturesZonal;

static INFO: MethodInfo = MethodInfo {
    id: "features_zonal",
    version: Version::new(1, 0, 0),
    summary: "Features → counts, lengths and areas per unit",
    citation: "Point-in-polygon aggregation by p-code hierarchy",
    inputs: &[PortSpec {
        name: "features",
        kind: PortKind::Features,
        required: true,
    }],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false,
      "required": ["features", "measure", "indicator", "period", "levels"],
      "properties": {
        "features": {"type": "string"},
        "measure": {"enum": ["count", "length_km", "area_km2"]},
        "kinds": {"type": "array", "items": {"type": "string"}},
        "attrs": {"type": "object", "additionalProperties": {"type": ["string", "number", "boolean"]}},
        "dim": {"type": "object", "additionalProperties": false, "required": ["id", "from"],
          "properties": {"id": {"type": "string"}, "from": {"type": "string"}, "total": {"type": "string"},
                         "codes": {"type": "array", "items": {"type": "string"}}}},
        "indicator": {"type": "string"},
        "period": {"type": "string"},
        "freq": {"type": "string", "pattern": "^[AFHSQMWDP]$"},
        "levels": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 1},
        "zeros": {"type": "boolean"}
      }
    }"#,
    output_basis: Basis::Computed,
    uncertainty: UncertaintyKind::None,
    mandatory_flags: Flags::EMPTY,
    tools: &[],
    partial_recompute: false,
};

/// The feature's breakdown code: `kind`, or an attribute.
fn code_of(f: &crate::method::Feature, from: &str) -> Option<String> {
    if from == "kind" {
        return Some(f.kind.clone());
    }
    match f.attrs.get(from)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        v => Some(v.to_string()),
    }
}

#[async_trait]
impl Method for FeaturesZonal {
    fn info(&self) -> &MethodInfo {
        &INFO
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        Ok(vec![PortUse {
            reference: param_str(params, "features")?.to_owned(),
            kind: PortKind::Features,
            indicators: Vec::new(),
        }])
    }

    #[allow(clippy::too_many_lines)]
    async fn run(
        &self,
        ctx: &MethodCtx<'_>,
        params: &Value,
        out: &mut dyn OutputSink,
    ) -> Result<MethodReport> {
        let port = param_str(params, "features")?;
        let measure = param_str(params, "measure")?;
        let indicator = param_str(params, "indicator")?;
        let kinds: BTreeSet<String> = param_strs(params, "kinds").into_iter().collect();
        let attrs = params
            .get("attrs")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let dim = params.get("dim");
        let dim_id = dim.and_then(|d| d.get("id")).and_then(Value::as_str);
        let dim_from = dim
            .and_then(|d| d.get("from"))
            .and_then(Value::as_str)
            .unwrap_or("kind");
        let dim_total = dim.and_then(|d| d.get("total")).and_then(Value::as_str);
        let fixed_codes: Vec<String> = dim.map(|d| param_strs(d, "codes")).unwrap_or_default();
        let zeros = params.get("zeros").and_then(Value::as_bool).unwrap_or(true);
        let levels = param_levels(params, "levels");
        let freq = match params.get("freq").and_then(Value::as_str) {
            Some(f) => f
                .chars()
                .next()
                .and_then(Freq::from_char)
                .context("freq")?,
            None => ctx
                .inputs
                .indicator(indicator)
                .map(|m| m.freq)
                .with_context(|| {
                    format!("features_zonal: indicator {indicator:?} is not in the catalogue (or give freq)")
                })?,
        };
        let period = Period::parse(freq, param_str(params, "period")?, ctx.inputs.calendars())
            .context("features_zonal: period")?;
        let units = ctx.inputs.units().await?;
        let features = ctx.inputs.features(port).await?;
        let pname: Arc<str> = Arc::from(port);

        // (unit code, breakdown code) → (value, contributors, flags)
        type Acc = BTreeMap<(String, String), (f64, Vec<Contrib>, Flags)>;
        let mut acc: Acc = BTreeMap::new();
        let mut codes: BTreeSet<String> = fixed_codes.iter().cloned().collect();
        let mut unplaced = 0u64;
        let mut used = 0u64;
        for (i, f) in features.iter().enumerate() {
            if !kinds.is_empty() && !kinds.contains(&f.kind) {
                continue;
            }
            if !attrs
                .iter()
                .all(|(k, v)| f.attrs.get(k).is_some_and(|a| a == v))
            {
                continue;
            }
            let qty = match measure {
                "count" => 1.0,
                "length_km" => f.length_km.unwrap_or(0.0),
                "area_km2" => f.area_km2.unwrap_or(0.0),
                other => bail!("features_zonal: unknown measure {other:?}"),
            };
            let Some(unit) = f.unit.as_deref().and_then(|u| units.get(u)) else {
                unplaced += 1;
                continue;
            };
            let code = if dim_id.is_some() {
                match code_of(f, dim_from) {
                    Some(c) if fixed_codes.is_empty() || fixed_codes.contains(&c) => c,
                    _ => continue,
                }
            } else {
                String::new()
            };
            used += 1;
            if dim_id.is_some() {
                codes.insert(code.clone());
            }
            let c = Contrib {
                port: pname.clone(),
                kind: ContribKind::Feature,
                idx: u32::try_from(i)?,
                role: Role::Feature,
                weight: qty,
            };
            let mut u = Some(unit);
            while let Some(cur) = u {
                if levels.contains(&cur.level) {
                    let mut targets = vec![code.clone()];
                    if let Some(t) = dim_total
                        && dim_id.is_some()
                    {
                        targets.push(t.to_owned());
                    }
                    for t in targets {
                        let e = acc
                            .entry((cur.code.clone(), t))
                            .or_insert_with(|| (0.0, Vec::new(), Flags::EMPTY));
                        e.0 += qty;
                        e.1.push(c.clone());
                        e.2 = e.2.union(f.flags);
                    }
                }
                u = cur.parent.as_deref().and_then(|p| units.get(p));
            }
        }
        if let Some(t) = dim_total
            && dim_id.is_some()
        {
            codes.insert(t.to_owned());
        }
        let codes: Vec<String> = if dim_id.is_some() {
            codes.into_iter().collect()
        } else {
            vec![String::new()]
        };
        let mut emitted = 0u64;
        for &level in &levels {
            for unit in units.at_level(level) {
                for code in &codes {
                    let dims = match dim_id {
                        Some(d) => Dims::from_pairs([(d, code.as_str())])?.canonical(),
                        None => String::new(),
                    };
                    let (value, contrib, flags) =
                        match acc.remove(&(unit.code.clone(), code.clone())) {
                            Some(v) => v,
                            None if zeros => (0.0, Vec::new(), Flags::EMPTY),
                            None => continue,
                        };
                    let row = Row {
                        flags,
                        ..Row::computed(
                            indicator,
                            unit.code.clone(),
                            unit.level,
                            period.freq,
                            period.start,
                            dims,
                            Some(value),
                        )
                    };
                    out.row(OutRow { row, contrib })?;
                    emitted += 1;
                }
            }
        }
        Ok(MethodReport {
            rows: emitted,
            diagnostics: json!({ "features": features.len(), "used": used, "unplaced": unplaced }),
        })
    }

    fn formula(&self, params: &Value, out: &OutRow) -> String {
        format!(
            "{}[{}, {}] = {} of {} feature(s) in the unit and its descendants",
            out.row.indicator,
            out.row.geo,
            out.row.dims,
            params
                .get("measure")
                .and_then(Value::as_str)
                .unwrap_or("count"),
            out.contrib.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;
    use lanka_core::{Calendars, Status};
    use proptest::prelude::*;

    use super::*;
    use crate::method::{CollectSink, Feature, MemInputs};
    use crate::methods::rollup::tests::{meta, tree};

    fn feat(key: &str, kind: &str, unit: &str, flags: Flags) -> Feature {
        Feature {
            key: key.into(),
            kind: kind.into(),
            unit: Some(unit.into()),
            attrs: json!({}),
            flags,
            length_km: Some(2.5),
            area_km2: None,
        }
    }

    fn inputs(fs: Vec<Feature>) -> MemInputs {
        let mut m = meta("fac", "sum");
        m.freq = Freq::A;
        MemInputs {
            features: HashMap::from([("pois".to_owned(), Arc::new(fs))]),
            units: Arc::new(tree(&[2, 2])),
            indicators: HashMap::from([("fac".to_owned(), m)]),
            calendars: Calendars::default(),
            ..MemInputs::default()
        }
    }

    async fn run(inp: &MemInputs, params: Value) -> Vec<OutRow> {
        let ctx = MethodCtx {
            inputs: inp,
            rng_seed: 0,
            as_of: NaiveDate::from_ymd_opt(2026, 1, 1).expect("date"),
            changed_keys: None,
        };
        let mut sink = CollectSink::default();
        FeaturesZonal
            .run(&ctx, &params, &mut sink)
            .await
            .expect("run");
        sink.rows
    }

    fn params() -> Value {
        json!({"features": "pois", "measure": "count", "indicator": "fac", "period": "2026",
               "levels": [0, 1, 2], "dim": {"id": "amenity_type", "from": "kind", "total": "total"}})
    }

    #[tokio::test]
    async fn counts_by_kind_up_the_hierarchy_with_flags() {
        let inp = inputs(vec![
            feat("n/1", "hospital", "ZZ11", Flags::UNOFFICIAL_GEOM),
            feat("n/2", "school", "ZZ11", Flags::EMPTY),
            feat("n/3", "school", "ZZ21", Flags::EMPTY),
            Feature {
                unit: None,
                ..feat("n/4", "school", "", Flags::EMPTY)
            },
        ]);
        let out = run(&inp, params()).await;
        let get = |g: &str, d: &str| {
            out.iter()
                .find(|o| o.row.geo == g && o.row.dims == format!("amenity_type={d}"))
                .map(|o| (o.row.value, o.row.flags))
        };
        assert_eq!(get("ZZ", "total").map(|v| v.0), Some(Some(3.0)));
        assert_eq!(get("ZZ1", "school").map(|v| v.0), Some(Some(1.0)));
        assert_eq!(get("ZZ12", "school").map(|v| v.0), Some(Some(0.0)), "zeros");
        assert!(get("ZZ11", "hospital").is_some_and(|v| v.1.contains(Flags::UNOFFICIAL_GEOM)));
        assert!(get("ZZ21", "school").is_some_and(|v| !v.1.contains(Flags::UNOFFICIAL_GEOM)));
        // 1 + 2 + 4 units × 3 codes
        assert_eq!(out.len(), 7 * 3);
        assert!(out.iter().all(|o| o.row.status == Status::Normal));
    }

    #[tokio::test]
    async fn lengths_and_filters() {
        let inp = inputs(vec![
            feat("w/1", "road", "ZZ11", Flags::EMPTY),
            feat("w/2", "road", "ZZ12", Flags::EMPTY),
            feat("w/3", "rail", "ZZ12", Flags::EMPTY),
        ]);
        let out = run(
            &inp,
            json!({"features": "pois", "measure": "length_km", "kinds": ["road"], "indicator": "fac",
                   "period": "2026", "levels": [1], "zeros": false}),
        )
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].row.value, Some(5.0));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// Every placed feature is counted exactly once per level.
        #[test]
        fn counts_conserve_features(placement in prop::collection::vec(0usize..4, 0..60)) {
            let leaves = ["ZZ11", "ZZ12", "ZZ21", "ZZ22"];
            let fs: Vec<Feature> = placement
                .iter()
                .enumerate()
                .map(|(i, p)| feat(&format!("n/{i}"), "school", leaves[*p], Flags::EMPTY))
                .collect();
            let n = fs.len() as f64;
            let inp = inputs(fs);
            let rt = tokio::runtime::Builder::new_current_thread().build().expect("rt");
            let out = rt.block_on(run(&inp, json!({"features": "pois", "measure": "count",
                "indicator": "fac", "period": "2026", "levels": [0, 1, 2]})));
            for l in 0..3 {
                let s: f64 = out.iter().filter(|o| o.row.level == l).filter_map(|o| o.row.value).sum();
                prop_assert_eq!(s, n);
            }
        }
    }
}
