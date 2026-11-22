//! Raster → canonical grid.
//!
//! | method | for | rule |
//! |---|---|---|
//! | [`split_sum`] | counts on a coarser raster (WorldPop ~1 km) | each valid pixel's value is split evenly over the cells whose **centre** lies in the pixel (half-open `[west, east) × [south, north)`); the v1 `cells` step, operation for operation |
//! | [`area_sum`] | counts on a raster of similar or finer resolution (WorldPop 100 m, 3″) | each valid pixel's value is split over the cells it overlaps in proportion to the overlap area (area-weighted redistribution); conserves the total inside the grid at any resolution ratio |
//! | [`CentreMean`] | intensive values on a finer raster (DEM 30 m) | mean of the valid pixels whose centre lies in the cell |
//! | [`CentreClasses`] | categories on a finer raster (WorldCover 10 m) | per-class count of pixels whose centre lies in the cell |
//! | [`CentreMax`] | depths on a finer raster (JRC flood 90 m) | maximum of the valid pixels whose centre lies in the cell |
//! | [`CentreShare`] | extents on a finer raster (JRC flood 90 m) | share of the pixels whose centre lies in the cell that are valid and above a threshold |
//! | [`centre_sample`] | intensive values on a coarser raster (WorldClim 30″) | the pixel containing the cell centre |
//!
//! All rules are deterministic and exact in IEEE-754 double arithmetic; no
//! interpolation invents detail below the source resolution.

use lanka_core::grid::Grid;

use crate::geotiff::{Block, GeoRef};

/// Cell ids whose centre lies in `[west, east) × [south, north)`, clipped to
/// the grid — `foundry/src/lib/raster.ts::fineCellsForBounds`, same
/// operations in the same order.
#[must_use]
pub fn cells_for_bounds(g: &Grid, west: f64, south: f64, east: f64, north: f64) -> CellRange {
    let step = g.step;
    let col_start_raw = ((west - g.lon_min) / step - 0.5).ceil();
    let col_end_raw = ((east - g.lon_min) / step - 0.5).ceil() - 1.0;
    let row_end_raw = ((g.lat_max - south) / step - 0.5).floor();
    let row_start_raw = ((g.lat_max - north) / step - 0.5).floor() + 1.0;
    let col_start = col_start_raw.max(0.0);
    let col_end = col_end_raw.min(f64::from(g.cols) - 1.0);
    let row_start = row_start_raw.max(0.0);
    let row_end = row_end_raw.min(f64::from(g.rows) - 1.0);
    if col_end < col_start || row_end < row_start {
        return CellRange::EMPTY;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    CellRange {
        row0: row_start as u32,
        row1: row_end as u32 + 1,
        col0: col_start as u32,
        col1: col_end as u32 + 1,
    }
}

/// A rectangle of cells, half-open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    pub row0: u32,
    pub row1: u32,
    pub col0: u32,
    pub col1: u32,
}

impl CellRange {
    pub const EMPTY: Self = Self {
        row0: 0,
        row1: 0,
        col0: 0,
        col1: 0,
    };

    #[must_use]
    pub const fn count(&self) -> u32 {
        (self.row1 - self.row0) * (self.col1 - self.col0)
    }
}

/// Totals of a [`split_sum`] pass (the v1 step logged the same three).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SplitReport {
    /// Sum of all valid positive pixels.
    pub raster_total: f64,
    pub positive_pixels: u64,
    /// Pixels with a positive value whose footprint holds no cell centre.
    pub pixels_without_cells: u64,
    /// Sum distributed onto the grid.
    pub distributed_total: f64,
}

/// v1 `cells`: valid pixels (finite, not nodata, `> 0`) split evenly over
/// the cells whose centre they contain. Cells covered by a valid zero pixel
/// are 0; cells covered by no valid pixel are NaN (nodata).
///
/// The pixel bounds are computed as the v1 step did from `ModelPixelScale`
/// and `ModelTiepoint` (`north = y0 + row·(−dy)`, `south = north + (−dy)`,
/// `west = x0 + col·dx`, `east = west + dx`).
#[must_use]
pub fn split_sum(
    g: &Grid,
    geo: &GeoRef,
    band: &Block,
    b: u16,
    nodata: Option<f64>,
) -> (Vec<f32>, SplitReport) {
    let n = crate::cells(g);
    let mut acc = vec![f64::NAN; n];
    let mut rep = SplitReport::default();
    let yres = -geo.dy;
    for row in 0..band.height {
        let north = geo.y0 + f64::from(band.y + row) * yres;
        let south = north + yres;
        for col in 0..band.width {
            let raw = band.get(col, row, b);
            let v = f64::from(raw);
            // Nodata compared at sample precision (see `GeoTiff::is_nodata`).
            #[allow(clippy::cast_possible_truncation)]
            let valid = v.is_finite() && nodata.is_none_or(|nd| raw != nd as f32);
            if !valid {
                continue;
            }
            let west = geo.x0 + f64::from(band.x + col) * geo.dx;
            let east = west + geo.dx;
            let r = cells_for_bounds(g, west, south, east, north);
            if v <= 0.0 {
                // A valid empty pixel: its cells are 0, not nodata.
                for_each_cell(g, r, |i| {
                    if acc[i].is_nan() {
                        acc[i] = 0.0;
                    }
                });
                continue;
            }
            rep.positive_pixels += 1;
            rep.raster_total += v;
            if r.count() == 0 {
                rep.pixels_without_cells += 1;
                continue;
            }
            let per = v / f64::from(r.count());
            for_each_cell(g, r, |i| {
                acc[i] = if acc[i].is_nan() { per } else { acc[i] + per };
            });
        }
    }
    let mut out = Vec::with_capacity(n);
    for a in acc {
        if a.is_finite() {
            rep.distributed_total += a;
        }
        #[allow(clippy::cast_possible_truncation)]
        out.push(a as f32);
    }
    (out, rep)
}

/// Totals of an [`area_sum`] pass.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AreaSumReport {
    /// Sum of all valid positive pixels.
    pub raster_total: f64,
    pub positive_pixels: u64,
    /// Sum distributed onto the grid (less than the raster total only by
    /// the parts of pixels outside the grid).
    pub distributed_total: f64,
}

/// Area-weighted redistribution: every valid pixel (finite, not nodata) is
/// split over the cells its footprint overlaps, each cell receiving
/// `v × overlap / pixel area` (planar in degrees — at 3″ the latitude
/// change of the metric across one pixel is below 1e-6). A pixel of 3″ on
/// the 0.001° grid overlaps at most 2 × 2 cells, so a cell holds the
/// population of the area it covers, not a whole pixel (no aliasing as with
/// centre sampling at a 1.2 resolution ratio). Cells overlapped only by valid
/// zero pixels are 0; cells overlapped by no valid pixel are NaN.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub fn area_sum(
    g: &Grid,
    geo: &GeoRef,
    band: &Block,
    b: u16,
    nodata: Option<f64>,
) -> (Vec<f32>, AreaSumReport) {
    let n = crate::cells(g);
    let mut acc = vec![f64::NAN; n];
    let mut rep = AreaSumReport::default();
    let step = g.step;
    let pixel_area = geo.dx * geo.dy;
    for row in 0..band.height {
        let north = geo.y0 - f64::from(band.y + row) * geo.dy;
        let south = north - geo.dy;
        // Grid rows overlapping [south, north).
        let r0 = ((g.lat_max - north) / step).floor().max(0.0);
        let r1 = ((g.lat_max - south) / step).ceil().min(f64::from(g.rows));
        if r1 <= r0 {
            continue;
        }
        for col in 0..band.width {
            let raw = band.get(col, row, b);
            let v = f64::from(raw);
            #[allow(clippy::cast_possible_truncation)]
            let valid = v.is_finite() && nodata.is_none_or(|nd| raw != nd as f32);
            if !valid {
                continue;
            }
            let v = v.max(0.0);
            if v > 0.0 {
                rep.positive_pixels += 1;
                rep.raster_total += v;
            }
            let west = geo.x0 + f64::from(band.x + col) * geo.dx;
            let east = west + geo.dx;
            let c0 = ((west - g.lon_min) / step).floor().max(0.0);
            let c1 = ((east - g.lon_min) / step).ceil().min(f64::from(g.cols));
            if c1 <= c0 {
                continue;
            }
            for r in (r0 as u32)..(r1 as u32) {
                let cn = g.lat_max - f64::from(r) * step;
                let cs = cn - step;
                let h = north.min(cn) - south.max(cs);
                if h <= 0.0 {
                    continue;
                }
                let base = r as usize * g.cols as usize;
                for c in (c0 as u32)..(c1 as u32) {
                    let cw = g.lon_min + f64::from(c) * step;
                    let w = east.min(cw + step) - west.max(cw);
                    if w <= 0.0 {
                        continue;
                    }
                    let part = v * (w * h) / pixel_area;
                    let a = &mut acc[base + c as usize];
                    *a = if a.is_nan() { part } else { *a + part };
                }
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    for a in acc {
        if a.is_finite() {
            rep.distributed_total += a;
        }
        #[allow(clippy::cast_possible_truncation)]
        out.push(a as f32);
    }
    (out, rep)
}

fn for_each_cell(g: &Grid, r: CellRange, mut f: impl FnMut(usize)) {
    for row in r.row0..r.row1 {
        let base = row as usize * g.cols as usize;
        for col in r.col0..r.col1 {
            f(base + col as usize);
        }
    }
}

/// The cell whose area contains the centre of a source pixel.
fn cell_of(g: &Grid, lon: f64, lat: f64) -> Option<usize> {
    g.cell_id(lat, lon).map(|c| c as usize)
}

/// Maximum of the valid fine pixels whose centre lies in the cell (flood
/// depth: the deepest water anywhere in the cell).
#[derive(Debug, Clone)]
pub struct CentreMax {
    grid: Grid,
    max: Vec<f32>,
}

impl CentreMax {
    #[must_use]
    pub fn new(grid: Grid) -> Self {
        Self {
            grid,
            max: vec![f32::NAN; crate::cells(&grid)],
        }
    }

    /// Adds every valid sample of band `b` of a block.
    pub fn add_block(
        &mut self,
        geo: &GeoRef,
        block: &Block,
        b: u16,
        is_nodata: impl Fn(f32) -> bool,
    ) {
        for row in 0..block.height {
            for col in 0..block.width {
                let v = block.get(col, row, b);
                if is_nodata(v) || v.is_nan() {
                    continue;
                }
                let (lon, lat) = geo.centre(block.x + col, block.y + row);
                if let Some(i) = cell_of(&self.grid, lon, lat) {
                    let m = &mut self.max[i];
                    if m.is_nan() || v > *m {
                        *m = v;
                    }
                }
            }
        }
    }

    /// Maxima (NaN where no valid pixel fell).
    #[must_use]
    pub fn finish(self) -> Vec<f32> {
        self.max
    }
}

/// Share of the fine pixels whose centre lies in the cell that are valid
/// and above a threshold; nodata pixels count as "not above" (rasters that
/// store only the wet area, like flood depth maps). NaN where no pixel of
/// the raster fell in the cell (outside its footprint).
#[derive(Debug, Clone)]
pub struct CentreShare {
    grid: Grid,
    threshold: f64,
    hit: Vec<u32>,
    n: Vec<u32>,
}

impl CentreShare {
    #[must_use]
    pub fn new(grid: Grid, threshold: f64) -> Self {
        let n = crate::cells(&grid);
        Self {
            grid,
            threshold,
            hit: vec![0; n],
            n: vec![0; n],
        }
    }

    /// Adds every pixel of band `b` of a block.
    pub fn add_block(
        &mut self,
        geo: &GeoRef,
        block: &Block,
        b: u16,
        is_nodata: impl Fn(f32) -> bool,
    ) {
        for row in 0..block.height {
            for col in 0..block.width {
                let (lon, lat) = geo.centre(block.x + col, block.y + row);
                let Some(i) = cell_of(&self.grid, lon, lat) else {
                    continue;
                };
                self.n[i] += 1;
                let v = block.get(col, row, b);
                if !is_nodata(v) && !v.is_nan() && f64::from(v) > self.threshold {
                    self.hit[i] += 1;
                }
            }
        }
    }

    /// Shares 0–1 (NaN where no pixel fell).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn finish(&self) -> Vec<f32> {
        self.hit
            .iter()
            .zip(&self.n)
            .map(|(&h, &n)| {
                if n == 0 {
                    f32::NAN
                } else {
                    (f64::from(h) / f64::from(n)) as f32
                }
            })
            .collect()
    }
}

/// Accumulates means of fine pixels per cell, block by block.
#[derive(Debug, Clone)]
pub struct CentreMean {
    grid: Grid,
    sum: Vec<f64>,
    n: Vec<u32>,
}

impl CentreMean {
    #[must_use]
    pub fn new(grid: Grid) -> Self {
        let n = crate::cells(&grid);
        Self {
            grid,
            sum: vec![0.0; n],
            n: vec![0; n],
        }
    }

    /// Adds one value for the pixel centred at `(lon, lat)`.
    pub fn add(&mut self, lon: f64, lat: f64, v: f64) {
        if let Some(i) = cell_of(&self.grid, lon, lat) {
            self.sum[i] += v;
            self.n[i] += 1;
        }
    }

    /// Adds every valid sample of band `b` of a block.
    pub fn add_block(
        &mut self,
        geo: &GeoRef,
        block: &Block,
        b: u16,
        is_nodata: impl Fn(f32) -> bool,
    ) {
        for row in 0..block.height {
            for col in 0..block.width {
                let v = block.get(col, row, b);
                if is_nodata(v) {
                    continue;
                }
                let (lon, lat) = geo.centre(block.x + col, block.y + row);
                self.add(lon, lat, f64::from(v));
            }
        }
    }

    /// Means (NaN where no pixel fell).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn finish(&self) -> Vec<f32> {
        self.sum
            .iter()
            .zip(&self.n)
            .map(|(&s, &n)| {
                if n == 0 {
                    f32::NAN
                } else {
                    (s / f64::from(n)) as f32
                }
            })
            .collect()
    }

    /// Pixel counts per cell.
    #[must_use]
    pub fn counts(&self) -> &[u32] {
        &self.n
    }
}

/// Per-class pixel counts per cell (u8 saturating: 10 m pixels per 0.001°
/// cell are exactly 144).
#[derive(Debug, Clone)]
pub struct CentreClasses {
    grid: Grid,
    /// class value → slot.
    slot: [Option<u8>; 256],
    pub classes: Vec<u8>,
    /// `counts[slot][cell]`.
    pub counts: Vec<Vec<u8>>,
    /// Valid pixels per cell (any class).
    pub valid: Vec<u8>,
}

impl CentreClasses {
    #[must_use]
    pub fn new(grid: Grid, classes: &[u8]) -> Self {
        let n = crate::cells(&grid);
        let mut slot = [None; 256];
        for (i, &c) in classes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            {
                slot[c as usize] = Some(i as u8);
            }
        }
        Self {
            grid,
            slot,
            classes: classes.to_vec(),
            counts: vec![vec![0; n]; classes.len()],
            valid: vec![0; n],
        }
    }

    /// Adds every pixel of band 0 of a block whose value is a listed class.
    pub fn add_block(&mut self, geo: &GeoRef, block: &Block) {
        let g = self.grid;
        for row in 0..block.height {
            let (_, lat) = geo.centre(0, block.y + row);
            for col in 0..block.width {
                let v = block.get(col, row, 0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let Some(s) = (0.0..256.0)
                    .contains(&v)
                    .then(|| self.slot[v as usize])
                    .flatten()
                else {
                    continue;
                };
                let (lon, _) = geo.centre(block.x + col, 0);
                if let Some(i) = cell_of(&g, lon, lat) {
                    let c = &mut self.counts[s as usize][i];
                    *c = c.saturating_add(1);
                    self.valid[i] = self.valid[i].saturating_add(1);
                }
            }
        }
    }

    /// The most frequent class per cell (ties → the lower class value), 0 = none.
    #[must_use]
    pub fn mode(&self) -> Vec<u8> {
        let n = self.valid.len();
        let mut out = vec![0u8; n];
        for (i, o) in out.iter_mut().enumerate() {
            let mut best = (0u8, 0u8);
            for (s, cnt) in self.counts.iter().enumerate() {
                if cnt[i] > best.1 {
                    best = (self.classes[s], cnt[i]);
                }
            }
            *o = best.0;
        }
        out
    }
}

/// Each cell takes the pixel containing its centre (NaN outside the raster
/// or on nodata).
#[must_use]
pub fn centre_sample(
    g: &Grid,
    geo: &GeoRef,
    band: &Block,
    b: u16,
    is_nodata: impl Fn(f32) -> bool,
) -> Vec<f32> {
    let mut out = vec![f32::NAN; crate::cells(g)];
    for row in 0..g.rows {
        let lat = g.lat_max - (f64::from(row) + 0.5) * g.step;
        for col in 0..g.cols {
            let lon = g.lon_min + (f64::from(col) + 0.5) * g.step;
            let Some((pc, pr)) = geo.pixel_at(lon, lat) else {
                continue;
            };
            if pc < band.x || pr < band.y || pc >= band.x + band.width || pr >= band.y + band.height
            {
                continue;
            }
            let v = band.get(pc - band.x, pr - band.y, b);
            if !is_nodata(v) {
                out[row as usize * g.cols as usize + col as usize] = v;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> Grid {
        Grid::CANONICAL
    }

    #[test]
    fn a_30_arcsec_pixel_covers_whole_cells_by_centre() {
        // WorldPop-like pixel at the grid origin: 0.00833… wide.
        let dx = 0.008_333_333_333_333_333;
        let r = cells_for_bounds(&g(), 79.4, 10.0 - dx, 79.4 + dx, 10.0);
        // Centres at 0.0005, 0.0015, … 0.0075 → 8 columns; 0.0085 > 0.00833.
        assert_eq!((r.col0, r.col1, r.row0, r.row1), (0, 8, 0, 8));
        assert_eq!(r.count(), 64);
        // A pixel entirely outside the bbox.
        assert_eq!(cells_for_bounds(&g(), 70.0, 1.0, 70.1, 1.1).count(), 0);
    }

    #[test]
    fn split_preserves_the_total() {
        let geo = GeoRef {
            x0: 79.5,
            dx: 0.008_333_333_333_333_333,
            y0: 9.9,
            dy: 0.008_333_333_333_333_333,
            width: 3,
            height: 2,
        };
        let block = Block {
            x: 0,
            y: 0,
            width: 3,
            height: 2,
            stride: 3,
            bands: 1,
            data: vec![10.0, 0.0, -99999.0, 1.5, 7.25, f32::NAN],
        };
        let (out, rep) = split_sum(&g(), &geo, &block, 0, Some(-99999.0));
        assert_eq!(rep.positive_pixels, 3);
        assert!((rep.raster_total - 18.75).abs() < 1e-9);
        assert!((rep.distributed_total - rep.raster_total).abs() < 1e-4);
        let zeros = out.iter().filter(|v| **v == 0.0).count();
        assert!(zeros > 0, "the zero pixel's cells are 0, not nodata");
        assert!(out.iter().filter(|v| v.is_finite()).count() < 6 * 81);
    }

    #[test]
    fn area_sum_conserves_and_resolves_100m_pixels() {
        // A 3-arc-second (WorldPop 100 m) raster of 12 × 12 pixels = 0.01°:
        // exactly 10 × 10 cells of 0.001°.
        let grid = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 10,
            cols: 10,
        };
        let dx = 1.0 / 1200.0;
        let geo = GeoRef {
            x0: 79.4,
            dx,
            y0: 10.0,
            dy: dx,
            width: 12,
            height: 12,
        };
        // Uniform density except one hot pixel (top-left corner).
        let mut data = vec![1.0f32; 144];
        data[0] = 145.0;
        let block = Block {
            x: 0,
            y: 0,
            width: 12,
            height: 12,
            stride: 12,
            bands: 1,
            data,
        };
        let (out, rep) = area_sum(&grid, &geo, &block, 0, Some(-99999.0));
        assert!((rep.raster_total - 288.0).abs() < 1e-9);
        assert!((rep.distributed_total - 288.0).abs() < 1e-3);
        // A cell away from the hot pixel holds 1.44 pixels' worth.
        assert!((f64::from(out[55]) - 1.44).abs() < 1e-4, "{}", out[55]);
        // The hot pixel (0.000833°) sits inside the first cell: its value
        // stays there, it is not smeared over a 1 km block.
        assert!(out[0] > 140.0, "{}", out[0]);
        assert!(out[1] < 3.0, "{}", out[1]);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn area_sum_matches_split_sum_on_aligned_coarse_pixels() {
        // A 0.002° pixel covering exactly 2 × 2 cells: both rules give v / 4.
        let grid = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 2,
            cols: 2,
        };
        let geo = GeoRef {
            x0: 79.4,
            dx: 0.002,
            y0: 10.0,
            dy: 0.002,
            width: 1,
            height: 1,
        };
        let block = Block {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            stride: 1,
            bands: 1,
            data: vec![8.0],
        };
        let (a, _) = area_sum(&grid, &geo, &block, 0, None);
        let (s, _) = split_sum(&grid, &geo, &block, 0, None);
        for (x, y) in a.iter().zip(&s) {
            assert!((x - y).abs() < 1e-5 && (x - 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn classes_and_mode() {
        let grid = Grid {
            lat_max: 10.0,
            lon_min: 79.4,
            step: 0.001,
            rows: 1,
            cols: 2,
        };
        // 10 m pixels: 12 × 12 per cell; two cells side by side.
        let geo = GeoRef {
            x0: 79.4,
            dx: 0.001 / 12.0,
            y0: 10.0,
            dy: 0.001 / 12.0,
            width: 24,
            height: 12,
        };
        let mut data = vec![10.0f32; 24 * 12];
        for r in 0..12 {
            for c in 12..24 {
                data[r * 24 + c] = if c < 15 { 80.0 } else { 40.0 };
            }
        }
        let block = Block {
            x: 0,
            y: 0,
            width: 24,
            height: 12,
            stride: 24,
            bands: 1,
            data,
        };
        let mut cc = CentreClasses::new(grid, &[10, 40, 80]);
        cc.add_block(&geo, &block);
        assert_eq!(cc.valid, vec![144, 144]);
        assert_eq!(cc.counts[0], vec![144, 0]);
        assert_eq!(cc.counts[1], vec![0, 108]);
        assert_eq!(cc.counts[2], vec![0, 36]);
        assert_eq!(cc.mode(), vec![10, 40]);
    }
}
