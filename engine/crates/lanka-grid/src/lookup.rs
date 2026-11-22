//! Lookup arrays of the `grid-lookups` dataset (05 §8): `gnd_idx` (u16:
//! slot of the deepest-level unit of the reference boundary set holding the
//! cell centre; 0 = none) and `land_mask` (u8, how far the cell can answer
//! a point query on its own).
//!
//! `gnd_idx` follows v1 `cell-lookup` exactly: cells in row-major order, a
//! sticky last match tried first, then the bucket index (first containing
//! candidate in insertion order), then the nearest polygon boundary within
//! 2 km for near-shore centres.
//!
//! `land_mask`:
//!
//! | value | meaning | point query |
//! |---|---|---|
//! | 0 | no unit within 2 km of the centre | polygon path (normally 404) |
//! | 1 | interior: the centre is inside a unit and no boundary crosses the cell | **the grid answers** |
//! | 2 | a unit boundary (or the coast) crosses the cell; the centre is inside a unit | polygon path |
//! | 3 | near-shore: the centre is outside every unit, `gnd_idx` from the 2 km fallback | polygon path |
//! | 4 | a boundary crosses the cell and its centre is outside every unit (fallback) | polygon path |
//!
//! Zonal passes over intensive or areal values (means, land-cover areas)
//! use only cells whose centre is inside a unit (`land_mask` 1 or 2);
//! counts (population) use every assigned cell, as v1 did.

use lanka_core::grid::Grid;
use serde::Serialize;

use crate::polygon::{MultiPolygon, PolygonIndex};

pub const MASK_NONE: u8 = 0;
pub const MASK_INTERIOR: u8 = 1;
pub const MASK_EDGE: u8 = 2;
pub const MASK_NEAR: u8 = 3;
pub const MASK_EDGE_OUTSIDE: u8 = 4;

/// Is the cell centre inside a unit (`land_mask` 1 or 2)?
#[must_use]
pub const fn centre_inside(mask: u8) -> bool {
    matches!(mask, MASK_INTERIOR | MASK_EDGE)
}

/// v1 `ADM4_BUCKET_CELL_DEG`.
pub const BUCKET_DEG: f64 = 0.03;
/// v1 `NEAREST_POLYGON_FALLBACK_M` (= `lanka_core::query::REVERSE_SNAP_M`).
pub const FALLBACK_M: f64 = 2000.0;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct LookupReport {
    pub cells: u64,
    pub direct: u64,
    pub sticky_hits: u64,
    pub fallback: u64,
    pub none: u64,
    pub interior: u64,
    pub edge: u64,
    pub near: u64,
    pub segments: u64,
}

#[derive(Debug, Clone)]
pub struct Lookups {
    pub gnd_idx: Vec<u16>,
    pub land_mask: Vec<u8>,
    pub report: LookupReport,
}

/// Which cells to resolve.
#[derive(Debug, Clone, Copy)]
pub enum Cells<'a> {
    All,
    /// Only these cell ids (ascending) — the v1 iteration over `cells`.
    Only(&'a [u32]),
}

/// Builds `gnd_idx` + `land_mask`. `units[i]` gets slot `i + 1` and is
/// inserted into the index in that order.
///
/// # Panics
/// If there are more than 65,535 units.
#[must_use]
pub fn build(g: &Grid, units: &[MultiPolygon], which: Cells<'_>) -> Lookups {
    assert!(
        units.len() < usize::from(u16::MAX),
        "too many units for a u16 index"
    );
    let n = crate::cells(g);
    let mut idx = PolygonIndex::new(BUCKET_DEG);
    for u in units {
        idx.insert(u.clone());
    }
    let mut rep = LookupReport::default();
    let mut gnd = vec![0u16; n];
    let mut mask = vec![MASK_NONE; n];

    // Boundary cells first (independent of the centre test).
    let mut edge = vec![false; n];
    for u in units {
        for p in &u.0 {
            for ring in p {
                for w in ring.windows(2) {
                    rep.segments += 1;
                    mark_segment(g, w[0], w[1], &mut edge);
                }
            }
        }
    }

    let mut sticky: Option<u32> = None;
    let mut resolve = |cell: u32, rep: &mut LookupReport| {
        let Some((lat, lon)) = centre(g, cell) else {
            return;
        };
        rep.cells += 1;
        let i = cell as usize;
        let mut hit = None;
        if let Some(s) = sticky
            && crate::polygon::point_in_geometry(lon, lat, &idx.geoms[s as usize])
        {
            hit = Some(s);
            rep.sticky_hits += 1;
        }
        if hit.is_none() {
            hit = idx.contains(lon, lat);
            if hit.is_some() {
                sticky = hit;
            }
        }
        let direct = hit.is_some();
        if direct {
            rep.direct += 1;
        } else if let Some((id, _)) = idx.nearest_within(lon, lat, FALLBACK_M) {
            hit = Some(id);
            sticky = hit;
            rep.fallback += 1;
        }
        match hit {
            #[allow(clippy::cast_possible_truncation)]
            Some(id) => gnd[i] = (id + 1) as u16,
            None => rep.none += 1,
        }
        mask[i] = if edge[i] && direct {
            MASK_EDGE
        } else if edge[i] {
            MASK_EDGE_OUTSIDE
        } else if direct {
            MASK_INTERIOR
        } else if hit.is_some() {
            MASK_NEAR
        } else {
            MASK_NONE
        };
    };
    match which {
        Cells::All =>
        {
            #[allow(clippy::cast_possible_truncation)]
            for c in 0..n as u32 {
                resolve(c, &mut rep);
            }
        }
        Cells::Only(ids) => {
            for &c in ids {
                resolve(c, &mut rep);
            }
        }
    }
    for &m in &mask {
        match m {
            MASK_INTERIOR => rep.interior += 1,
            MASK_EDGE | MASK_EDGE_OUTSIDE => rep.edge += 1,
            MASK_NEAR => rep.near += 1,
            _ => {}
        }
    }
    Lookups {
        gnd_idx: gnd,
        land_mask: mask,
        report: rep,
    }
}

/// Cell centre `(lat, lon)` with the v1 formula.
fn centre(g: &Grid, cell: u32) -> Option<(f64, f64)> {
    if *g == Grid::CANONICAL {
        return lanka_core::grid::cell_center(cell);
    }
    let (row, col) = (f64::from(cell / g.cols), f64::from(cell % g.cols));
    (cell < g.rows * g.cols).then_some((
        g.lat_max - (row + 0.5) * g.step,
        g.lon_min + (col + 0.5) * g.step,
    ))
}

/// Marks every cell a segment passes through (Amanatides–Woo traversal in
/// cell coordinates; diagonal corner crossings mark both neighbours).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn mark_segment(g: &Grid, a: [f64; 2], b: [f64; 2], edge: &mut [bool]) {
    let to = |p: [f64; 2]| ((p[0] - g.lon_min) / g.step, (g.lat_max - p[1]) / g.step);
    let (x0, y0) = to(a);
    let (x1, y1) = to(b);
    let (rows, cols) = (f64::from(g.rows), f64::from(g.cols));
    // Entirely outside the grid on one side: nothing to mark.
    if (x0 < 0.0 && x1 < 0.0)
        || (y0 < 0.0 && y1 < 0.0)
        || (x0 >= cols && x1 >= cols)
        || (y0 >= rows && y1 >= rows)
    {
        return;
    }
    let (mut cx, mut cy) = (x0.floor() as i64, y0.floor() as i64);
    let (ex, ey) = (x1.floor() as i64, y1.floor() as i64);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let sx: i64 = if dx > 0.0 { 1 } else { -1 };
    let sy: i64 = if dy > 0.0 { 1 } else { -1 };
    let tdx = if dx == 0.0 {
        f64::INFINITY
    } else {
        1.0 / dx.abs()
    };
    let tdy = if dy == 0.0 {
        f64::INFINITY
    } else {
        1.0 / dy.abs()
    };
    let mut tmx = if dx > 0.0 {
        (cx as f64 + 1.0 - x0) / dx
    } else if dx < 0.0 {
        (x0 - cx as f64) / -dx
    } else {
        f64::INFINITY
    };
    let mut tmy = if dy > 0.0 {
        (cy as f64 + 1.0 - y0) / dy
    } else if dy < 0.0 {
        (y0 - cy as f64) / -dy
    } else {
        f64::INFINITY
    };
    let max_steps = (ex - cx).abs() + (ey - cy).abs() + 2;
    let (ncols, nrows) = (i64::from(g.cols), i64::from(g.rows));
    for _ in 0..=max_steps {
        if (0..ncols).contains(&cx) && (0..nrows).contains(&cy) {
            edge[(cy * ncols + cx) as usize] = true;
        }
        if cx == ex && cy == ey {
            break;
        }
        if tmx < tmy {
            cx += sx;
            tmx += tdx;
        } else if tmy < tmx {
            cy += sy;
            tmy += tdy;
        } else {
            // Exact corner: mark both side cells, then move diagonally.
            for (px, py) in [(cx + sx, cy), (cx, cy + sy)] {
                if (0..ncols).contains(&px) && (0..nrows).contains(&py) {
                    edge[(py * ncols + px) as usize] = true;
                }
            }
            cx += sx;
            cy += sy;
            tmx += tdx;
            tmy += tdy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Grid {
        Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 20,
            cols: 20,
        }
    }

    fn rect(w: f64, s: f64, e: f64, n: f64) -> MultiPolygon {
        MultiPolygon(vec![vec![vec![[w, s], [e, s], [e, n], [w, n], [w, s]]]])
    }

    #[test]
    fn two_units_and_the_sea() {
        let g = small();
        // Left unit covers cols 2..10, right unit cols 10..15.
        let a = rect(79.402, 9.985, 79.410, 9.998);
        let b = rect(79.410, 9.985, 79.415, 9.998);
        let l = build(&g, &[a, b], Cells::All);
        let at = |r: u32, c: u32| (r * 20 + c) as usize;
        assert_eq!(l.gnd_idx[at(5, 5)], 1);
        assert_eq!(l.land_mask[at(5, 5)], MASK_INTERIOR);
        assert_eq!(l.gnd_idx[at(5, 12)], 2);
        assert_eq!(l.land_mask[at(5, 12)], MASK_INTERIOR);
        // 0.010 / 0.001 is not exact in binary: the shared edge lands in col 9 or 10.
        assert!(l.land_mask[at(5, 9)] == MASK_EDGE || l.land_mask[at(5, 10)] == MASK_EDGE);
        // Off the units but within 2 km: near-shore fallback.
        assert_eq!(l.land_mask[at(5, 18)], MASK_NEAR);
        assert_eq!(l.gnd_idx[at(5, 18)], 2);
        assert_eq!(l.report.cells, 400);
        assert_eq!(l.report.none, 0);
        assert!(l.report.sticky_hits > 0);
    }

    #[test]
    fn diagonal_segments_mark_a_connected_trail() {
        let g = small();
        let mut e = vec![false; 400];
        mark_segment(&g, [79.4005, 9.9995], [79.4155, 9.9845], &mut e);
        // Every row from 0 to 15 has at least one marked cell.
        for r in 0..16 {
            assert!((0..20).any(|c| e[r * 20 + c]), "row {r}");
        }
    }
}
