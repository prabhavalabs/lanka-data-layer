//! `grid_zonal@1` — zonal statistics of a canonical grid layer per admin
//! unit (04 §2, 05 §8): one linear pass over the unit index (`gnd_idx`) and
//! the layer (`lanka_grid::zonal::zonal`), the roll-up of the deepest units
//! to every ancestor (`lanka_grid::zonal::rollup`) and `Acc::stat` — the same
//! kernel and semantics as the raster driver's `zonal:` entries
//! (`lanka_ingest::grid::zonal_rows`).
//!
//! ```yaml
//! with:
//!   grid: pop                 # recipe input: the raster dataset owning the layer
//!   layer: worldpop.population
//!   layer_period: "2025"      # required when the layer has several periods
//!   indicator: derived.pop_zonal
//!   period: "2025"
//!   stat: sum                 # sum | mean (area-weighted) | area_sum | max
//!   cells: all                # all | inside (centre inside a unit, land_mask 1/2)
//!   factor: 1                 # multiplies every value (m² → km²: 1e-6)
//!   levels: [1, 2, 3, 4]
//!   dims: ""                  # canonical k=v;k=v of the output rows
//!   basis: estimated          # computed (default) | estimated
//! ```
//!
//! Units without a valid cell get no row. Every row has one `cells`
//! contributor whose `idx` is the number of valid cells behind it.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use lanka_core::{Basis, Flags, Period};
use lanka_grid::zonal::{Stat, rollup, zonal};
use serde_json::{Value, json};

use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortSpec, PortUse, Role, Row, UncertaintyKind, Version, param_levels, param_str,
};

pub struct GridZonal;

static INFO: MethodInfo = MethodInfo {
    id: "grid_zonal",
    version: Version::new(1, 0, 0),
    summary: "Zonal statistic (sum, area-weighted mean, area sum, max) of a grid layer per admin unit and ancestors",
    citation: "05 §8: one pass over gnd_idx (cell centre → deepest unit, v1 cell-lookup semantics) and the layer",
    inputs: &[PortSpec {
        name: "grid",
        kind: PortKind::Grid,
        required: true,
    }],
    params: r#"{
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object", "additionalProperties": false,
      "required": ["grid", "layer", "indicator", "period"],
      "properties": {
        "grid": {"type": "string"},
        "layer": {"type": "string"},
        "layer_period": {"type": "string"},
        "indicator": {"type": "string"},
        "period": {"type": "string"},
        "stat": {"enum": ["sum", "mean", "area_sum", "max"]},
        "cells": {"enum": ["all", "inside"]},
        "factor": {"type": "number"},
        "levels": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 1},
        "dims": {"type": "string"},
        "basis": {"enum": ["computed", "estimated"]}
      }
    }"#,
    output_basis: Basis::Computed,
    uncertainty: UncertaintyKind::None,
    mandatory_flags: Flags::EMPTY,
    tools: &[],
    partial_recompute: false,
};

fn stat_of(params: &Value) -> Result<Stat> {
    Ok(match params.get("stat").and_then(Value::as_str) {
        None | Some("sum") => Stat::Sum,
        Some("mean") => Stat::Mean,
        Some("area_sum") => Stat::AreaSum,
        Some("max") => Stat::Max,
        Some(s) => bail!("grid_zonal: stat {s:?}"),
    })
}

fn inside_of(params: &Value) -> Result<bool> {
    Ok(match params.get("cells").and_then(Value::as_str) {
        None | Some("all") => false,
        Some("inside") => true,
        Some(c) => bail!("grid_zonal: cells {c:?}"),
    })
}

fn basis_of(params: &Value) -> Result<Basis> {
    Ok(match params.get("basis").and_then(Value::as_str) {
        None | Some("computed") => Basis::Computed,
        Some("estimated") => Basis::Estimated,
        Some(b) => bail!("grid_zonal: basis {b:?}"),
    })
}

#[async_trait]
impl Method for GridZonal {
    fn info(&self) -> &MethodInfo {
        &INFO
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        Ok(vec![PortUse {
            reference: param_str(params, "grid")?.to_owned(),
            kind: PortKind::Grid,
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
        let port = param_str(params, "grid")?;
        let layer = param_str(params, "layer")?;
        let layer_period = params.get("layer_period").and_then(Value::as_str);
        let indicator = param_str(params, "indicator")?;
        let meta = ctx
            .inputs
            .indicator(indicator)
            .with_context(|| format!("grid_zonal: indicator {indicator:?} unknown"))?;
        let period = Period::parse(
            meta.freq,
            param_str(params, "period")?,
            ctx.inputs.calendars(),
        )?;
        let stat = stat_of(params)?;
        let inside = inside_of(params)?;
        let basis = basis_of(params)?;
        let factor = params.get("factor").and_then(Value::as_f64).unwrap_or(1.0);
        let mut levels = param_levels(params, "levels");
        if levels.is_empty() {
            levels = vec![1, 2, 3, 4];
        }
        let dims = params.get("dims").and_then(Value::as_str).unwrap_or("");

        let g = ctx.inputs.grid_layer(port, layer, layer_period).await?;
        let idx: &[u32] = if inside {
            g.unit_idx_inside.as_deref().with_context(|| {
                format!("grid_zonal: {layer} has no land mask for cells: inside")
            })?
        } else {
            &g.unit_idx
        };
        ensure!(
            g.values.len() == idx.len(),
            "grid {}: {} values but {} index cells",
            g.layer,
            g.values.len(),
            idx.len()
        );
        // lanka_grid slots: 0 = no unit, s = units[s - 1].
        let n_units = g.units.len();
        ensure!(
            n_units < usize::from(u16::MAX),
            "grid_zonal: {n_units} units exceed the u16 slot space"
        );
        let slots: Vec<u16> = idx
            .iter()
            .map(|&u| {
                if u == u32::MAX {
                    Ok(0)
                } else {
                    ensure!((u as usize) < n_units, "unit index {u} out of range");
                    Ok(u16::try_from(u + 1)?)
                }
            })
            .collect::<Result<_>>()?;
        let (cols, areas): (u32, Vec<f64>) = if g.cols == 0 || g.row_area.is_empty() {
            (u32::try_from(g.values.len().max(1))?, vec![1.0])
        } else {
            (g.cols, g.row_area.to_vec())
        };
        ensure!(
            areas.len() * cols as usize == g.values.len(),
            "grid {}: {} rows × {cols} cols ≠ {} cells",
            g.layer,
            areas.len(),
            g.values.len()
        );
        let accs = zonal(&slots, &*g.values, cols, &areas, n_units);
        let grid_total: f64 = g
            .values
            .iter()
            .filter(|v| v.is_finite())
            .map(|&v| f64::from(v))
            .sum();
        let by_slot: f64 = accs.iter().map(|a| a.sum).sum();
        ensure!(
            (by_slot - grid_total).abs() <= 1e-6 * grid_total.abs().max(1.0),
            "grid_zonal: the pass over {} does not preserve the grid total",
            g.layer
        );

        // Slot → the unit and every ancestor (by code).
        let units = ctx.inputs.units().await?;
        let mut parents: Vec<Vec<String>> = Vec::with_capacity(n_units + 1);
        parents.push(Vec::new());
        for code in g.units.iter() {
            let mut chain = vec![code.clone()];
            let mut cur = units.get(code);
            while let Some(p) = cur.and_then(|u| u.parent.as_deref()) {
                chain.push(p.to_owned());
                cur = units.get(p);
            }
            parents.push(chain);
        }
        let up = rollup(&accs, &parents);
        let pname: Arc<str> = Arc::from(port);
        let mut n = 0u64;
        let mut empty = 0usize;
        let mut by_level: HashMap<i16, u64> = HashMap::new();
        for (code, a) in &up {
            let Some(u) = units.get(code) else { continue };
            if !levels.contains(&u.level) {
                continue;
            }
            let Some(v) = a.stat(stat) else {
                empty += 1;
                continue;
            };
            let row = Row {
                basis: basis.max(g.basis),
                flags: g.flags,
                ..Row::computed(
                    indicator,
                    code.as_str(),
                    u.level,
                    period.freq,
                    period.start,
                    dims,
                    Some(v * factor),
                )
            };
            out.row(OutRow {
                row,
                contrib: vec![Contrib {
                    port: pname.clone(),
                    kind: ContribKind::Cells,
                    idx: u32::try_from(a.n).unwrap_or(u32::MAX),
                    role: Role::CellSum,
                    weight: 1.0,
                }],
            })?;
            n += 1;
            *by_level.entry(u.level).or_default() += 1;
        }
        Ok(MethodReport {
            rows: n,
            diagnostics: json!({
                "layer": g.layer, "cells": g.values.len(), "grid_total": grid_total,
                "unassigned": accs[0].sum, "unassigned_cells": accs[0].n,
                "empty_units": empty, "rows_by_level": by_level,
            }),
        })
    }

    fn formula(&self, params: &Value, out: &OutRow) -> String {
        let cells = out.contrib.first().map_or(0, |c| c.idx);
        format!(
            "{}[{}] = {}({} {}) over {cells} cell(s){}",
            out.row.indicator,
            out.row.geo,
            params.get("stat").and_then(Value::as_str).unwrap_or("sum"),
            params.get("layer").and_then(Value::as_str).unwrap_or("?"),
            params
                .get("layer_period")
                .and_then(Value::as_str)
                .unwrap_or(""),
            params
                .get("factor")
                .and_then(Value::as_f64)
                .map_or_else(String::new, |f| format!(" × {f}"))
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;
    use lanka_core::Calendars;

    use super::*;
    use crate::method::{CollectSink, GridInput, MemInputs};
    use crate::methods::rollup::tests::{meta, tree};

    fn grid(values: Vec<f32>, idx: Vec<u32>, inside: Option<Vec<u32>>) -> GridInput {
        GridInput {
            layer: "pop".into(),
            values: Arc::from(values),
            unit_idx: Arc::from(idx),
            units: Arc::from(vec!["ZZ11".to_owned(), "ZZ12".to_owned()]),
            unit_idx_inside: inside.map(Arc::from),
            cols: 2,
            row_area: Arc::from(vec![1.0, 3.0]),
            basis: Basis::Computed,
            flags: Flags::EMPTY,
        }
    }

    async fn run(g: GridInput, with: Value) -> Vec<(String, Option<f64>, u32)> {
        let inp = MemInputs {
            grids: HashMap::from([("g".to_owned(), Arc::new(g))]),
            units: Arc::new(tree(&[2, 2])),
            indicators: HashMap::from([("p".to_owned(), meta("p", "sum"))]),
            calendars: Calendars::default(),
            ..MemInputs::default()
        };
        let ctx = MethodCtx {
            inputs: &inp,
            rng_seed: 0,
            as_of: NaiveDate::from_ymd_opt(2025, 1, 1).expect("d"),
            changed_keys: None,
        };
        let mut params = json!({"grid": "g", "layer": "pop", "indicator": "p", "period": "2025"});
        for (k, v) in with.as_object().expect("obj") {
            params[k] = v.clone();
        }
        let mut sink = CollectSink::default();
        GridZonal.run(&ctx, &params, &mut sink).await.expect("run");
        sink.rows
            .iter()
            .map(|r| (r.row.geo.clone(), r.row.value, r.contrib[0].idx))
            .collect()
    }

    #[tokio::test]
    async fn sums_roll_up_to_every_ancestor() {
        // rows: [ZZ11 ZZ12] [ZZ11 none]
        let g = grid(vec![1.0, 2.0, 4.0, 8.0], vec![0, 1, 0, u32::MAX], None);
        let v = run(g, json!({"levels": [0, 1, 2]})).await;
        assert_eq!(
            v,
            vec![
                ("ZZ".into(), Some(7.0), 3),
                ("ZZ1".into(), Some(7.0), 3),
                ("ZZ11".into(), Some(5.0), 2),
                ("ZZ12".into(), Some(2.0), 1),
            ]
        );
    }

    #[tokio::test]
    async fn mean_is_area_weighted_and_inside_uses_the_mask() {
        let g = grid(
            vec![10.0, 20.0, 30.0, f32::NAN],
            vec![0, 0, 0, 0],
            Some(vec![0, u32::MAX, 0, 0]),
        );
        let v = run(g.clone(), json!({"stat": "mean", "levels": [2]})).await;
        // (10·1 + 20·1 + 30·3) / 5 = 24
        assert_eq!(v, vec![("ZZ11".into(), Some(24.0), 3)]);
        let v = run(
            g,
            json!({"stat": "sum", "cells": "inside", "factor": 0.5, "levels": [2]}),
        )
        .await;
        assert_eq!(v, vec![("ZZ11".into(), Some(20.0), 2)]);
    }
}
