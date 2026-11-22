//! The grid engine (05 §8, ADR 0013): rasters → canonical grid arrays →
//! zonal observations.
//!
//! - [`array`]: one layer × period on the canonical 0.001° grid
//!   (`lanka_core::grid`), typed (`u8/u16/i16/u32/f32`) with scale, offset
//!   and nodata; written as a `.bin` with the 4 KiB `LKGRID01` header
//!   ([`header`]) so the payload starts page-aligned at 4096.
//! - [`mmap`]: the only `unsafe` in the workspace — read-only memory maps of
//!   immutable `.bin` files for the server.
//! - [`geotiff`]: pure-Rust GeoTIFF reading (tiled/stripped, deflate/LZW,
//!   predictors 1–3, PixelIsArea/PixelIsPoint, EPSG:4326 only).
//! - [`resample`]: raster → canonical grid (`split_sum` = the v1 WorldPop
//!   method, `centre_mean`, `centre_classes`, `centre_sample`), [`slope`].
//! - [`polygon`] + [`lookup`]: v1-parity point-in-polygon → `gnd_idx` and the
//!   `land_mask` that tells the server when a cell answers alone.
//! - [`zonal`]: one linear pass over `gnd_idx` + a layer into per-unit
//!   accumulators, and their roll-up through ancestors.
//! - [`cog`]: Cloud Optimized GeoTIFF writer; [`tile`]: web-mercator PNG tiles.

pub mod array;
pub mod cog;
pub mod geotiff;
pub mod header;
pub mod lookup;
pub mod mmap;
pub mod polygon;
pub mod resample;
pub mod slope;
pub mod tile;
pub mod zonal;

pub use array::{Dtype, GridArray, GridData, LayerEncoding};
pub use header::Header;
pub use mmap::MappedGrid;

use lanka_core::grid::Grid;

/// Number of cells of a grid.
#[must_use]
pub fn cells(g: &Grid) -> usize {
    g.rows as usize * g.cols as usize
}

/// Area of one cell of row `row`, m² (authalic sphere, R = 6,371,007.2 m):
/// `R² · Δλ · (sin φn − sin φs)`. Every cell of a row has the same area, so
/// one value per row is enough (05 §8 `cell_area_m2`).
#[must_use]
pub fn cell_area_m2(g: &Grid, row: u32) -> f64 {
    const R: f64 = 6_371_007.2;
    let north = g.lat_max - f64::from(row) * g.step;
    let south = north - g.step;
    R * R * g.step.to_radians() * (north.to_radians().sin() - south.to_radians().sin())
}

/// `cell_area_m2` for every row.
#[must_use]
pub fn row_areas(g: &Grid) -> Vec<f64> {
    (0..g.rows).map(|r| cell_area_m2(g, r)).collect()
}

/// Something that yields a physical value (or nodata) per cell.
pub trait CellValues: Sync {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Physical value of cell `i` (`raw · scale + offset`), `None` = nodata.
    fn value(&self, i: usize) -> Option<f64>;
}

impl CellValues for [f32] {
    fn len(&self) -> usize {
        <[f32]>::len(self)
    }
    fn value(&self, i: usize) -> Option<f64> {
        let v = self[i];
        v.is_finite().then_some(f64::from(v))
    }
}

impl CellValues for Vec<f32> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }
    fn value(&self, i: usize) -> Option<f64> {
        self.as_slice().value(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_areas_are_about_111m_squared() {
        let g = Grid::CANONICAL;
        let a0 = cell_area_m2(&g, 0);
        let a_last = cell_area_m2(&g, g.rows - 1);
        // ~111.2 m × 109.6 m at 10° N, ~111.2 m × 110.6 m at 5.8° N.
        assert!((12_000.0..12_400.0).contains(&a0), "{a0}");
        assert!(a_last > a0);
        // The bbox area equals the zone area of the sphere slice.
        let total: f64 = row_areas(&g).iter().sum::<f64>() * f64::from(g.cols);
        assert!((total / 1e6 - 138_900.0).abs() < 1_500.0, "{total}");
    }
}
