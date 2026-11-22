//! `exposure_overlay` (04 §3, 11 §4 `exposure`): a hazard mask (cells whose
//! hazard value is at or above a threshold) × an exposed quantity (people,
//! buildings) per unit, in one pass over `gnd_idx`, like [`crate::zonal`].
//!
//! Per slot it accumulates the exposed total, the exposed quantity inside
//! the mask, the land area and the masked area, so shares are exact
//! aggregates at every level after [`rollup`] (never averages of shares).
//! A cell whose hazard is nodata is outside the mask; a cell whose exposed
//! value is nodata contributes nothing.

use crate::CellValues;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExposureAcc {
    /// Cells with an exposed value.
    pub cells: u64,
    /// Of those, cells inside the mask.
    pub masked_cells: u64,
    /// Σ exposed.
    pub total: f64,
    /// Σ exposed inside the mask.
    pub exposed: f64,
    /// Σ cell area (m²) of the counted cells.
    pub area: f64,
    /// Σ cell area inside the mask.
    pub masked_area: f64,
}

impl ExposureAcc {
    pub fn merge(&mut self, o: &Self) {
        self.cells += o.cells;
        self.masked_cells += o.masked_cells;
        self.total += o.total;
        self.exposed += o.exposed;
        self.area += o.area;
        self.masked_area += o.masked_area;
    }

    /// Exposed / total (None when nothing is exposed-able).
    #[must_use]
    pub fn share(&self) -> Option<f64> {
        (self.total > 0.0).then(|| self.exposed / self.total)
    }

    /// Masked area / area.
    #[must_use]
    pub fn area_share(&self) -> Option<f64> {
        (self.area > 0.0).then(|| self.masked_area / self.area)
    }
}

/// One pass over the unit index, the hazard and the exposed layer.
///
/// # Panics
/// When the three arrays differ in length or a slot exceeds `slots`.
#[must_use]
pub fn exposure_overlay<H: CellValues + ?Sized, E: CellValues + ?Sized>(
    idx: &[u16],
    hazard: &H,
    threshold: f64,
    exposed: &E,
    cols: u32,
    row_area: &[f64],
    slots: usize,
) -> Vec<ExposureAcc> {
    assert_eq!(idx.len(), hazard.len(), "index and hazard differ in length");
    assert_eq!(
        idx.len(),
        exposed.len(),
        "index and exposed differ in length"
    );
    let mut out = vec![ExposureAcc::default(); slots + 1];
    let cols = cols as usize;
    for (row, chunk) in idx.chunks(cols).enumerate() {
        let a = row_area[row];
        let base = row * cols;
        for (c, &s) in chunk.iter().enumerate() {
            let i = base + c;
            let Some(e) = exposed.value(i) else { continue };
            let inside = hazard.value(i).is_some_and(|h| h >= threshold);
            let acc = &mut out[s as usize];
            acc.cells += 1;
            acc.total += e;
            acc.area += a;
            if inside {
                acc.masked_cells += 1;
                acc.exposed += e;
                acc.masked_area += a;
            }
        }
    }
    out
}

/// Rolls slot accumulators up to groups (`parents[s]` = every unit slot `s`
/// counts towards), sorted by group.
#[must_use]
pub fn rollup<G: Ord + Clone>(accs: &[ExposureAcc], parents: &[Vec<G>]) -> Vec<(G, ExposureAcc)> {
    let mut m: std::collections::BTreeMap<G, ExposureAcc> = std::collections::BTreeMap::new();
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

    #[test]
    fn masks_and_rolls_up_exactly() {
        // 2×3 grid, slots 1 and 2, one cell unassigned (0)
        let idx = [1_u16, 1, 2, 2, 2, 0];
        let hazard = [0.9_f32, 0.1, f32::NAN, 0.5, 0.7, 1.0];
        let pop = [10.0_f32, 20.0, 5.0, 5.0, 10.0, 99.0];
        let accs = exposure_overlay(&idx, &hazard[..], 0.5, &pop[..], 3, &[1.0, 2.0], 2);
        assert_eq!(accs[1].total, 30.0);
        assert_eq!(accs[1].exposed, 10.0);
        assert_eq!(accs[2].total, 20.0);
        assert_eq!(accs[2].exposed, 15.0);
        assert_eq!(accs[2].masked_cells, 2);
        let up = rollup(&accs, &[vec![], vec!["A", "P"], vec!["B", "P"]]);
        let p = up.iter().find(|(g, _)| *g == "P").expect("P").1;
        assert_eq!(p.exposed, 25.0);
        assert_eq!(p.share(), Some(0.5));
        // area share: slot 1 row 0 (area 1 each): 1 of 2 cells masked
        assert_eq!(accs[1].area_share(), Some(0.5));
    }
}
