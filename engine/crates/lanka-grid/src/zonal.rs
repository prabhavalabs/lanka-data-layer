//! Zonal statistics: one linear pass over `gnd_idx` and a layer into
//! per-slot accumulators (05 §8), then a roll-up through unit ancestors.
//!
//! Pure functions over arrays: no database, no files. The enrichment
//! method `grid_zonal` (04 §2, WP-06's runner) wraps [`zonal`] +
//! [`rollup`] + [`Acc::stat`]; until then `lanka-ingest::grid` publishes
//! their output directly.

use serde::{Deserialize, Serialize};

use crate::CellValues;

/// Per-slot accumulator. Weights are cell areas (m²).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Acc {
    /// Valid cells.
    pub n: u64,
    /// Σ v.
    pub sum: f64,
    /// Σ v · a.
    pub wsum: f64,
    /// Σ a over valid cells.
    pub area: f64,
    pub max: Option<f64>,
}

impl Acc {
    #[inline]
    pub fn add(&mut self, v: f64, a: f64) {
        self.n += 1;
        self.sum += v;
        self.wsum += v * a;
        self.area += a;
        self.max = Some(self.max.map_or(v, |m| m.max(v)));
    }

    pub fn merge(&mut self, o: &Self) {
        self.n += o.n;
        self.sum += o.sum;
        self.wsum += o.wsum;
        self.area += o.area;
        self.max = match (self.max, o.max) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }

    /// The statistic, `None` when no valid cell fell in the unit.
    #[must_use]
    pub fn stat(&self, s: Stat) -> Option<f64> {
        if self.n == 0 {
            return None;
        }
        match s {
            Stat::Sum => Some(self.sum),
            Stat::Mean => (self.area > 0.0).then(|| self.wsum / self.area),
            Stat::AreaSum => Some(self.wsum),
            Stat::Max => self.max,
        }
    }
}

/// `geo.grid_layers.zonal` / `grid_zonal.stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stat {
    /// Σ v (counts: population).
    Sum,
    /// Area-weighted mean Σ v·a / Σ a (intensive values: elevation, climate).
    Mean,
    /// Σ v·a (a fraction × cell area = an area, m²: land-cover classes).
    AreaSum,
    Max,
}

/// One pass: `out[s]` accumulates every cell with `idx[cell] == s` (slot 0
/// = cells assigned to no unit). `row_area[r]` is the cell area of row `r`.
///
/// # Panics
/// When `idx` and `values` have different lengths or a slot exceeds `slots`.
#[must_use]
pub fn zonal<V: CellValues + ?Sized>(
    idx: &[u16],
    values: &V,
    cols: u32,
    row_area: &[f64],
    slots: usize,
) -> Vec<Acc> {
    assert_eq!(idx.len(), values.len(), "index and layer differ in length");
    let mut out = vec![Acc::default(); slots + 1];
    let cols = cols as usize;
    for (row, chunk) in idx.chunks(cols).enumerate() {
        let a = row_area[row];
        let base = row * cols;
        for (c, &s) in chunk.iter().enumerate() {
            if let Some(v) = values.value(base + c) {
                out[s as usize].add(v, a);
            }
        }
    }
    out
}

/// Rolls slot accumulators up to every ancestor: `parents[s]` lists the
/// group ids (ancestor units at every level, and the slot's own unit) that
/// slot `s` contributes to. Returns `(group, acc)` sorted by group.
#[must_use]
pub fn rollup<G: Ord + Clone>(accs: &[Acc], parents: &[Vec<G>]) -> Vec<(G, Acc)> {
    let mut m: std::collections::BTreeMap<G, Acc> = std::collections::BTreeMap::new();
    for (s, a) in accs.iter().enumerate().skip(1) {
        let Some(groups) = parents.get(s) else {
            continue;
        };
        for g in groups {
            m.entry(g.clone()).or_default().merge(a);
        }
    }
    m.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `grid_zonal(sum)` over all units + unassigned cells equals the grid
        /// total (04 §8), and the roll-up of the leaves to one root equals it too.
        #[test]
        fn zonal_sum_preserves_the_total(
            rows in 1usize..30,
            cols in 1usize..30,
            slots in 1usize..20,
            seed in proptest::collection::vec((0u16..25, proptest::option::weighted(0.9, 0.0f32..1e6)), 900),
        ) {
            let n = rows * cols;
            let idx: Vec<u16> = seed.iter().take(n).map(|(s, _)| s % (slots as u16 + 1)).collect();
            let vals: Vec<f32> = seed.iter().take(n).map(|(_, v)| v.unwrap_or(f32::NAN)).collect();
            prop_assume!(idx.len() == n);
            let area = vec![1.0; rows];
            #[allow(clippy::cast_possible_truncation)]
            let accs = zonal(&idx, &vals, cols as u32, &area, slots);
            let total: f64 = vals.iter().filter(|v| v.is_finite()).map(|&v| f64::from(v)).sum();
            let by_slot: f64 = accs.iter().map(|a| a.sum).sum();
            prop_assert!((by_slot - total).abs() <= 1e-9 * total.abs().max(1.0));
            let cells: u64 = accs.iter().map(|a| a.n).sum();
            prop_assert_eq!(cells as usize, vals.iter().filter(|v| v.is_finite()).count());
            // Every slot rolls up to one root (0) and to itself.
            let parents: Vec<Vec<i64>> = (0..=slots).map(|s| vec![0, s as i64]).collect();
            let up = rollup(&accs, &parents);
            let root = up.iter().find(|(g, _)| *g == 0).map_or(0.0, |(_, a)| a.sum);
            prop_assert!((root + accs[0].sum - total).abs() <= 1e-9 * total.abs().max(1.0));
        }
    }

    #[test]
    fn mean_is_area_weighted() {
        let idx = [1u16, 1, 1, 1];
        let vals = vec![10.0f32, 20.0, 30.0, f32::NAN];
        let accs = zonal(&idx, &vals, 2, &[1.0, 3.0], 1);
        // (10·1 + 20·1 + 30·3) / (1 + 1 + 3) = 24
        assert_eq!(accs[1].stat(Stat::Mean), Some(24.0));
        assert_eq!(accs[1].stat(Stat::Sum), Some(60.0));
        assert_eq!(accs[1].stat(Stat::Max), Some(30.0));
        assert_eq!(accs[1].n, 3);
        assert_eq!(Acc::default().stat(Stat::Sum), None);
    }
}
