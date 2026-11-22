//! A plug-in point for custom array-based zonal kernels. The built-in
//! `grid_zonal@1` (`methods/grid_zonal.rs`) wraps `lanka_grid::zonal`
//! directly; this adapter remains for kernels outside that library.
//!
//! WP-05 implements the kernel as a pure function over the canonical grid
//! arrays — values and the per-cell unit index (`gnd_idx`) — and wraps it in
//! [`GridZonal`], which makes it a [`Method`]: inputs come from
//! [`Inputs::grid`] (memory-mapped arrays on the worker, or in-memory arrays
//! in tests and `lanka-analysis`), one row per deepest unit is emitted with a
//! `cells` contributor summary, and roll-up to coarser levels is a following
//! `rollup` step in the recipe. The kernel never sees Postgres, the recipe or
//! the runner:
//!
//! ```ignore
//! struct GridZonalKernel;                       // lanka-enrich/src/methods/grid_zonal.rs (WP-05)
//! impl ZonalKernel for GridZonalKernel {
//!     fn info(&self) -> &MethodInfo { &INFO }   // id "grid_zonal", params schema, basis, flags
//!     fn zonal(&self, values: &[f32], unit_idx: &[u32], n_units: usize, params: &Value)
//!         -> Result<Vec<ZonalStat>> { /* rayon over rows of the grid */ }
//! }
//! // methods::builtin(): Arc::new(GridZonal(GridZonalKernel))
//! ```
//!
//! [`SumKernel`] is the reference kernel (and the conservation property
//! test every kernel should copy: Σ units + unassigned = grid total).

use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::method::{
    Contrib, ContribKind, Method, MethodCtx, MethodInfo, MethodReport, OutRow, OutputSink,
    PortKind, PortUse, Role, Row, param_str,
};

/// One unit's statistic and the number of cells behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZonalStat {
    pub value: Option<f64>,
    pub cells: u32,
}

/// A pure zonal kernel over aligned arrays.
pub trait ZonalKernel: Send + Sync {
    fn info(&self) -> &MethodInfo;
    /// Statistics per unit index `0..n_units`. `values[i]` is NaN for nodata;
    /// `unit_idx[i] == u32::MAX` means the cell is in no unit.
    fn zonal(
        &self,
        values: &[f32],
        unit_idx: &[u32],
        n_units: usize,
        params: &Value,
    ) -> Result<Vec<ZonalStat>>;
}

/// Adapts a [`ZonalKernel`] to the [`Method`] contract.
///
/// Parameters it reads itself: `grid` (port), `indicator`, `period`
/// (parsed with the output indicator's frequency); everything else is the
/// kernel's. Rows are emitted at the grid index's unit level.
pub struct GridZonal<K>(pub K);

#[async_trait]
impl<K: ZonalKernel> Method for GridZonal<K> {
    fn info(&self) -> &MethodInfo {
        self.0.info()
    }

    fn ports(&self, params: &Value) -> Result<Vec<PortUse>> {
        Ok(vec![PortUse {
            reference: param_str(params, "grid")?.to_owned(),
            kind: PortKind::Grid,
            indicators: Vec::new(),
        }])
    }

    async fn run(
        &self,
        ctx: &MethodCtx<'_>,
        params: &Value,
        out: &mut dyn OutputSink,
    ) -> Result<MethodReport> {
        let port = param_str(params, "grid")?;
        let indicator = param_str(params, "indicator")?;
        let meta = ctx
            .inputs
            .indicator(indicator)
            .with_context(|| format!("{}: indicator {indicator:?} unknown", self.info().id))?;
        let period = lanka_core::Period::parse(
            meta.freq,
            param_str(params, "period")?,
            ctx.inputs.calendars(),
        )?;
        let g = ctx.inputs.grid(port).await?;
        ensure!(
            g.values.len() == g.unit_idx.len(),
            "grid {}: {} values but {} index cells",
            g.layer,
            g.values.len(),
            g.unit_idx.len()
        );
        let stats = self
            .0
            .zonal(&g.values, &g.unit_idx, g.units.len(), params)?;
        ensure!(
            stats.len() == g.units.len(),
            "kernel returned the wrong number of units"
        );
        let units = ctx.inputs.units().await?;
        let pname: Arc<str> = Arc::from(port);
        let mut n = 0u64;
        for (code, s) in g.units.iter().zip(&stats) {
            let level = units.get(code).map_or(0, |u| u.level);
            let row = Row {
                basis: g.basis,
                flags: g.flags,
                ..Row::computed(
                    indicator,
                    code,
                    level,
                    period.freq,
                    period.start,
                    "",
                    s.value,
                )
            };
            out.row(OutRow {
                row,
                contrib: vec![Contrib {
                    port: pname.clone(),
                    kind: ContribKind::Cells,
                    idx: s.cells,
                    role: Role::CellSum,
                    weight: 1.0,
                }],
            })?;
            n += 1;
        }
        Ok(MethodReport {
            rows: n,
            diagnostics: json!({ "layer": g.layer, "cells": g.values.len() }),
        })
    }
}

/// Reference kernel: the sum of valid cells per unit.
pub struct SumKernel(pub &'static MethodInfo);

impl ZonalKernel for SumKernel {
    fn info(&self) -> &MethodInfo {
        self.0
    }

    fn zonal(
        &self,
        values: &[f32],
        unit_idx: &[u32],
        n_units: usize,
        _params: &Value,
    ) -> Result<Vec<ZonalStat>> {
        let mut sum = vec![0.0f64; n_units];
        let mut cells = vec![0u32; n_units];
        for (v, u) in values.iter().zip(unit_idx) {
            if v.is_nan() || *u == u32::MAX {
                continue;
            }
            let u = *u as usize;
            sum[u] += f64::from(*v);
            cells[u] += 1;
        }
        Ok(sum
            .into_iter()
            .zip(cells)
            .map(|(s, c)| ZonalStat {
                value: (c > 0).then_some(s),
                cells: c,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;
    use lanka_core::{Basis, Calendars, Flags, Freq};
    use proptest::prelude::*;

    use super::*;
    use crate::method::{CollectSink, GridInput, MemInputs, PortSpec, UncertaintyKind, Version};
    use crate::methods::rollup::tests::{meta, tree};

    static INFO: MethodInfo = MethodInfo {
        id: "grid_sum_test",
        version: Version::new(0, 1, 0),
        summary: "test kernel",
        citation: "",
        inputs: &[PortSpec {
            name: "grid",
            kind: PortKind::Grid,
            required: true,
        }],
        params: "{}",
        output_basis: Basis::Computed,
        uncertainty: UncertaintyKind::None,
        mandatory_flags: Flags::EMPTY,
        tools: &[],
        partial_recompute: false,
    };

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// Σ over units + unassigned cells = grid total.
        #[test]
        fn sum_kernel_conserves_the_grid_total(
            cells in prop::collection::vec((0.0f32..1000.0, 0u32..5), 1..500),
        ) {
            let values: Vec<f32> = cells.iter().map(|c| c.0).collect();
            let idx: Vec<u32> = cells.iter().map(|c| if c.1 == 4 { u32::MAX } else { c.1 }).collect();
            let stats = SumKernel(&INFO).zonal(&values, &idx, 4, &json!({})).expect("zonal");
            let units: f64 = stats.iter().filter_map(|s| s.value).sum();
            let unassigned: f64 = values.iter().zip(&idx).filter(|(_, u)| **u == u32::MAX).map(|(v, _)| f64::from(*v)).sum();
            let total: f64 = values.iter().map(|v| f64::from(*v)).sum();
            prop_assert!((units + unassigned - total).abs() <= 1e-6 * total.max(1.0));
        }
    }

    #[tokio::test]
    async fn adapter_emits_one_row_per_unit() {
        let g = GridInput {
            layer: "pop".into(),
            values: Arc::from(vec![1.0f32, 2.0, f32::NAN, 4.0]),
            unit_idx: Arc::from(vec![0u32, 0, 1, 1]),
            units: Arc::from(vec!["ZZ1".to_owned(), "ZZ2".to_owned()]),
            unit_idx_inside: None,
            cols: 0,
            row_area: Arc::from(Vec::<f64>::new()),
            basis: Basis::Estimated,
            flags: Flags::EMPTY,
        };
        let inp = MemInputs {
            grids: HashMap::from([("pop".to_owned(), Arc::new(g))]),
            units: Arc::new(tree(&[2])),
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
        let mut sink = CollectSink::default();
        GridZonal(SumKernel(&INFO))
            .run(
                &ctx,
                &json!({"grid": "pop", "indicator": "p", "period": "2025"}),
                &mut sink,
            )
            .await
            .expect("run");
        let v: Vec<(String, Option<f64>, Basis, Freq)> = sink
            .rows
            .iter()
            .map(|r| (r.row.geo.clone(), r.row.value, r.row.basis, r.row.freq))
            .collect();
        assert_eq!(
            v,
            vec![
                ("ZZ1".into(), Some(3.0), Basis::Estimated, Freq::A),
                ("ZZ2".into(), Some(4.0), Basis::Estimated, Freq::A)
            ]
        );
        assert_eq!(sink.rows[1].contrib[0].idx, 1, "one valid cell");
    }
}
