//! Value tiles in encoding `ldl-v16` (10 §3.2, ADR 0019) and the overview
//! pyramid behind them.
//!
//! A value tile is an opaque 256 × 256 RGBA PNG: `R × 256 + G` is the
//! quantised value `q` (u16), physical value = `q × scale + offset`,
//! `q = 65535` is no data; `B` is the share of valid source cells under the
//! pixel (0–255; 255 or 0 at the native level); `A` is always 255 so no
//! browser premultiplies the payload.
//!
//! - [`Quant`]: the per layer-period quantisation. Integer layers whose raw
//!   values fit 0…65534 pass their raw value through (u8, u16, i16 shifted by
//!   32768): decoding is exact. `u32` and `f32` layers are quantised linearly
//!   over the layer's value range (`max_error` = `scale / 2`).
//! - [`Pyramid`]: levels 1…[`MAX_LEVEL`] of 2ᵏ × 2ᵏ native cells reduced by
//!   the layer's overview rule — `mean` (valid cells), `sum` (served as the
//!   mean per valid cell, a density, so one ramp holds at every zoom, as the
//!   coloured tiles do), `max`, `mode` (ties → the smaller value). Every
//!   level is reduced from the native cells (mean/sum through exact
//!   `(Σ, n)` accumulators, mode by direct counting), never from a coarser
//!   level's rounded values.
//! - [`render`]: each output pixel takes the cell of level
//!   `k = ⌊log₂(pixel width / cell width)⌋` under its centre; at the native
//!   level (z ≥ 10 on the 0.001° grid) that is the source cell itself.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use lanka_core::grid::Grid;
use serde::Serialize;

use crate::CellValues;
use crate::array::{Dtype, LayerEncoding};
use crate::cog::Overview;
use crate::tile::SIZE;

/// Encoding name carried by the resolver and `/v2/grids/{id}`.
pub const ENCODING: &str = "ldl-v16";
/// `q` of a pixel without data.
pub const NODATA_Q: u16 = 65_535;
/// Coarsest pyramid level (2⁷ = 128 cells ≈ 0.128° on the canonical grid,
/// finer than a z4 pixel).
pub const MAX_LEVEL: u8 = 7;

/// Quantisation of one layer-period.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Quant {
    pub scale: f64,
    pub offset: f64,
    /// Largest |decoded − source| at the native level.
    pub max_error: f64,
    /// Raw values pass through unchanged (decoding is exact).
    pub exact: bool,
}

impl Quant {
    /// The quantisation of a layer: raw pass-through for integer dtypes whose
    /// raw range fits, else linear over `[min, max]` of the layer's values.
    #[must_use]
    pub fn for_layer(enc: &LayerEncoding, min: Option<f64>, max: Option<f64>) -> Self {
        let pass = |shift: f64| Self {
            scale: enc.scale,
            offset: shift.mul_add(-enc.scale, enc.offset),
            max_error: 0.0,
            exact: true,
        };
        match enc.dtype {
            Dtype::U8 | Dtype::U16 => pass(0.0),
            Dtype::I16 => pass(32_768.0),
            Dtype::U32 | Dtype::F32 => {
                let lo = min.filter(|v| v.is_finite()).unwrap_or(0.0);
                let hi = max.filter(|v| v.is_finite()).unwrap_or(lo);
                if hi > lo {
                    let scale = (hi - lo) / 65_534.0;
                    Self {
                        scale,
                        offset: lo,
                        max_error: scale / 2.0,
                        exact: false,
                    }
                } else {
                    Self {
                        scale: 1.0,
                        offset: lo,
                        max_error: 0.0,
                        exact: false,
                    }
                }
            }
        }
    }

    /// `q` of a physical value (clamped to 0…65534).
    #[must_use]
    pub fn q(&self, v: f64) -> u16 {
        let r = ((v - self.offset) / self.scale).round();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        if r.is_nan() {
            NODATA_Q
        } else {
            r.clamp(0.0, 65_534.0) as u16
        }
    }

    /// Physical value of `q`.
    #[must_use]
    pub fn value(&self, q: u16) -> Option<f64> {
        (q != NODATA_Q).then(|| f64::from(q).mul_add(self.scale, self.offset))
    }
}

/// One level: `w × h` cells of 2ᵏ × 2ᵏ native cells.
#[derive(Debug, Clone)]
struct Level {
    w: usize,
    h: usize,
    q: Vec<u16>,
    share: Vec<u8>,
}

/// The overview pyramid of a layer-period, quantised (3 bytes per cell,
/// ≈ one third of the native cell count in all).
#[derive(Debug, Clone)]
pub struct Pyramid {
    pub grid: Grid,
    pub quant: Quant,
    pub reducer: Overview,
    levels: Vec<Level>,
}

/// Reducer of a set of valid values (the definition the pyramid must equal).
#[must_use]
pub fn reduce(vals: &mut [f64], ov: Overview) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    Some(match ov {
        Overview::Mean | Overview::Sum => vals.iter().sum::<f64>() / vals.len() as f64,
        Overview::Max => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Overview::Mode => {
            vals.sort_by(f64::total_cmp);
            let (mut best, mut n, mut i) = (vals[0], 0, 0);
            while i < vals.len() {
                let mut j = i;
                while j < vals.len() && vals[j] == vals[i] {
                    j += 1;
                }
                if j - i > n {
                    best = vals[i];
                    n = j - i;
                }
                i = j;
            }
            best
        }
    })
}

fn share_byte(valid: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let b = (valid as f64 * 255.0 / total as f64).round() as u8;
    b
}

impl Pyramid {
    /// Builds levels 1…`max_level` from the native values.
    pub fn build<V: CellValues + ?Sized>(
        values: &V,
        grid: Grid,
        quant: Quant,
        reducer: Overview,
        max_level: u8,
    ) -> Self {
        let (rows, cols) = (grid.rows as usize, grid.cols as usize);
        let mut levels = Vec::new();
        match reducer {
            Overview::Mean | Overview::Sum | Overview::Max => {
                // Level-1 accumulators straight from the native cells, then
                // exact merges: (Σ, n) for means, max for max.
                let is_max = reducer == Overview::Max;
                let (mut w, mut h) = (cols.div_ceil(2), rows.div_ceil(2));
                let mut acc = vec![if is_max { f64::NEG_INFINITY } else { 0.0 }; w * h];
                let mut n = vec![0u64; w * h];
                let mut tot = vec![0u64; w * h];
                for r in 0..rows {
                    for c in 0..cols {
                        let j = (r / 2) * w + c / 2;
                        tot[j] += 1;
                        if let Some(v) = values.value(r * cols + c) {
                            n[j] += 1;
                            if is_max {
                                acc[j] = acc[j].max(v);
                            } else {
                                acc[j] += v;
                            }
                        }
                    }
                }
                for k in 1..=max_level {
                    #[allow(clippy::cast_precision_loss)]
                    let q = acc
                        .iter()
                        .zip(&n)
                        .map(|(&a, &m)| match m {
                            0 => NODATA_Q,
                            _ if is_max => quant.q(a),
                            _ => quant.q(a / m as f64),
                        })
                        .collect();
                    let share = n
                        .iter()
                        .zip(&tot)
                        .map(|(&a, &t)| share_byte(a, t))
                        .collect();
                    levels.push(Level { w, h, q, share });
                    if k == max_level {
                        break;
                    }
                    let (w2, h2) = (w.div_ceil(2), h.div_ceil(2));
                    let mut acc2 = vec![if is_max { f64::NEG_INFINITY } else { 0.0 }; w2 * h2];
                    let mut n2 = vec![0u64; w2 * h2];
                    let mut tot2 = vec![0u64; w2 * h2];
                    for y in 0..h {
                        for x in 0..w {
                            let (i, j) = (y * w + x, (y / 2) * w2 + x / 2);
                            tot2[j] += tot[i];
                            n2[j] += n[i];
                            if is_max {
                                acc2[j] = acc2[j].max(acc[i]);
                            } else {
                                acc2[j] += acc[i];
                            }
                        }
                    }
                    (w, h, acc, n, tot) = (w2, h2, acc2, n2, tot2);
                }
            }
            Overview::Mode => {
                // Distinct values are few (classes); count them per block.
                for k in 1..=max_level {
                    let f = 1usize << k;
                    let (w, h) = (cols.div_ceil(f), rows.div_ceil(f));
                    let mut q = vec![NODATA_Q; w * h];
                    let mut share = vec![0u8; w * h];
                    let mut counts: BTreeMap<u16, u64> = BTreeMap::new();
                    for y in 0..h {
                        for x in 0..w {
                            counts.clear();
                            let (mut valid, mut total) = (0u64, 0u64);
                            for r in y * f..((y + 1) * f).min(rows) {
                                for c in x * f..((x + 1) * f).min(cols) {
                                    total += 1;
                                    if let Some(v) = values.value(r * cols + c) {
                                        valid += 1;
                                        *counts.entry(quant.q(v)).or_default() += 1;
                                    }
                                }
                            }
                            // Ascending keys: the first maximum is the smaller value.
                            let mut best: Option<(u16, u64)> = None;
                            for (&k, &m) in &counts {
                                if best.is_none_or(|b| m > b.1) {
                                    best = Some((k, m));
                                }
                            }
                            q[y * w + x] = best.map_or(NODATA_Q, |b| b.0);
                            share[y * w + x] = share_byte(valid, total);
                        }
                    }
                    levels.push(Level { w, h, q, share });
                }
            }
        }
        Self {
            grid,
            quant,
            reducer,
            levels,
        }
    }

    /// Number of levels above the native one.
    #[must_use]
    pub fn depth(&self) -> u8 {
        u8::try_from(self.levels.len()).unwrap_or(u8::MAX)
    }

    /// `(q, share)` of cell `(row, col)` of level `k ≥ 1`.
    #[must_use]
    pub fn at(&self, k: u8, row: usize, col: usize) -> Option<(u16, u8)> {
        let l = self.levels.get(usize::from(k).checked_sub(1)?)?;
        (row < l.h && col < l.w).then(|| (l.q[row * l.w + col], l.share[row * l.w + col]))
    }

    /// Bytes held in memory.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(|l| l.q.len() * 3).sum()
    }
}

/// Pyramid level a pixel of zoom `z` samples (0 = native).
#[must_use]
pub fn level_for_zoom(g: &Grid, z: u8, depth: u8) -> u8 {
    #[allow(clippy::cast_precision_loss)]
    let px = 360.0 / ((1u64 << z) as f64 * SIZE as f64);
    let ratio = px / g.step;
    if ratio < 2.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k = ratio.log2().floor() as u8;
    k.min(depth)
}

/// Renders tile `z/x/y` as an `ldl-v16` PNG; `pyramid` is needed only when
/// the zoom samples an overview level (pass `None` at native zooms).
pub fn render<V: CellValues + ?Sized>(
    values: &V,
    g: &Grid,
    quant: &Quant,
    pyramid: Option<&Pyramid>,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Vec<u8>> {
    encode_png(&render_rgba(values, g, quant, pyramid, z, x, y)?)
}

/// The RGBA bytes of a value tile (before PNG encoding).
pub fn render_rgba<V: CellValues + ?Sized>(
    values: &V,
    g: &Grid,
    quant: &Quant,
    pyramid: Option<&Pyramid>,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Vec<u8>> {
    if z > 22 || x >= (1u32 << z) || y >= (1u32 << z) {
        bail!("tile {z}/{x}/{y} out of range");
    }
    let k = level_for_zoom(g, z, pyramid.map_or(0, Pyramid::depth));
    let n = f64::from(1u32 << z);
    #[allow(clippy::cast_precision_loss)]
    let sz = SIZE as f64;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        *px = [255, 255, 0, 255];
    }
    let (rows, cols) = (f64::from(g.rows), f64::from(g.cols));
    let lon_cols: Vec<Option<usize>> = (0..SIZE)
        .map(|px| {
            #[allow(clippy::cast_precision_loss)]
            let lon = (f64::from(x) + (px as f64 + 0.5) / sz) / n * 360.0 - 180.0;
            let c = ((lon - g.lon_min) / g.step).floor();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            (c >= 0.0 && c < cols).then_some(c as usize)
        })
        .collect();
    for py in 0..SIZE {
        #[allow(clippy::cast_precision_loss)]
        let lat = (std::f64::consts::PI
            * (1.0 - 2.0 * (f64::from(y) + (py as f64 + 0.5) / sz) / n))
            .sinh()
            .atan()
            .to_degrees();
        let r = ((g.lat_max - lat) / g.step).floor();
        if !(r >= 0.0 && r < rows) {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let r = r as usize;
        for (px, c) in lon_cols.iter().enumerate() {
            let Some(c) = *c else { continue };
            let (q, b) = if k == 0 {
                match values.value(r * g.cols as usize + c) {
                    Some(v) => (quant.q(v), 255),
                    None => (NODATA_Q, 0),
                }
            } else {
                // Level cells are aligned on the native ones: integer division.
                let f = 1usize << k;
                pyramid
                    .and_then(|p| p.at(k, r / f, c / f))
                    .unwrap_or((NODATA_Q, 0))
            };
            let i = (py * SIZE + px) * 4;
            rgba[i] = (q >> 8) as u8;
            rgba[i + 1] = (q & 0xff) as u8;
            rgba[i + 2] = b;
        }
    }
    Ok(rgba)
}

/// Encodes 256 × 256 RGBA bytes as PNG.
pub fn encode_png(rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(32 * 1024);
    {
        #[allow(clippy::cast_possible_truncation)]
        let mut enc = png::Encoder::new(&mut out, SIZE as u32, SIZE as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header()?;
        w.write_image_data(rgba)?;
    }
    Ok(out)
}

/// Decodes an `ldl-v16` PNG into `(q, share)` per pixel (tests, tools).
pub fn decode_png(png_bytes: &[u8]) -> Result<Vec<(u16, u8)>> {
    let dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut r = dec.read_info()?;
    let mut buf = vec![0u8; r.output_buffer_size().unwrap_or(SIZE * SIZE * 4)];
    let info = r.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("not an 8-bit RGBA PNG");
    }
    Ok(buf[..info.buffer_size()]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((u16::from(p[0]) << 8) | u16::from(p[1]), p[2]))
        .collect())
}

/// Centre `(lon, lat)` of pixel `(px, py)` of tile `z/x/y`.
#[must_use]
pub fn pixel_centre(z: u8, x: u32, y: u32, px: usize, py: usize) -> (f64, f64) {
    let n = f64::from(1u32 << z);
    #[allow(clippy::cast_precision_loss)]
    let (fx, fy) = (
        (px as f64 + 0.5) / SIZE as f64,
        (py as f64 + 0.5) / SIZE as f64,
    );
    let lon = (f64::from(x) + fx) / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + fy) / n))
        .sinh()
        .atan()
        .to_degrees();
    (lon, lat)
}

/// Tiles of zoom `z` that touch the grid: `(x0..=x1, y0..=y1)`.
#[must_use]
pub fn tile_range(g: &Grid, z: u8) -> (u32, u32, u32, u32) {
    let n = f64::from(1u32 << z);
    let xt = |lon: f64| ((lon + 180.0) / 360.0 * n).floor();
    let yt = |lat: f64| {
        let r = lat.to_radians();
        ((1.0 - (r.tan() + 1.0 / r.cos()).ln() / std::f64::consts::PI) / 2.0 * n).floor()
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let clamp = |v: f64| v.clamp(0.0, n - 1.0) as u32;
    (
        clamp(xt(g.lon_min)),
        clamp(xt(g.lon_max() - 1e-12)),
        clamp(yt(g.lat_max)),
        clamp(yt(g.lat_min() + 1e-12)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::GridArray;
    use proptest::prelude::*;

    fn small(rows: u32, cols: u32) -> Grid {
        // Over Colombo, inside z11 tile 1478/985 and z10 739/492.
        Grid {
            lat_max: 6.95,
            lon_min: 79.85,
            step: 0.001,
            rows,
            cols,
        }
    }

    fn native_tile(g: &Grid) -> (u8, u32, u32) {
        let (x0, _, y0, _) = tile_range(g, 11);
        (11, x0, y0)
    }

    fn enc_of(d: u8) -> LayerEncoding {
        match d {
            0 => LayerEncoding {
                dtype: Dtype::U8,
                scale: 0.004,
                offset: 0.0,
                nodata: Some(255.0),
            },
            1 => LayerEncoding {
                dtype: Dtype::U16,
                scale: 0.01,
                offset: 0.0,
                nodata: Some(65_535.0),
            },
            2 => LayerEncoding {
                dtype: Dtype::I16,
                scale: 0.1,
                offset: 0.0,
                nodata: Some(-32_768.0),
            },
            _ => LayerEncoding::f32(),
        }
    }

    /// Physical values representable in `enc` (raw drawn in range).
    fn values_for(enc: &LayerEncoding, raws: &[(bool, f64)]) -> Vec<f32> {
        raws.iter()
            .map(|&(nd, u)| {
                if nd {
                    return f32::NAN;
                }
                #[allow(clippy::cast_possible_truncation)]
                match enc.dtype {
                    Dtype::U8 => ((u * 254.0).round() * enc.scale) as f32,
                    Dtype::U16 => ((u * 65_534.0).round() * enc.scale) as f32,
                    Dtype::I16 => (((u * 65_533.0) - 32_767.0).round() * enc.scale) as f32,
                    _ => (u * 5000.0 - 100.0) as f32,
                }
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// Decoding a native-zoom value tile reproduces every source cell
        /// under a pixel centre exactly (integer layers: the raw value;
        /// f32: within the declared quantisation error).
        #[test]
        fn native_tiles_decode_to_the_source(
            d in 0u8..4,
            rows in 8u32..60, cols in 8u32..60,
            raws in proptest::collection::vec((proptest::bool::weighted(0.15), 0.0f64..1.0), 3600),
        ) {
            let g = small(rows, cols);
            let enc = enc_of(d);
            let vals = values_for(&enc, &raws[..(rows * cols) as usize]);
            let a = GridArray::quantise(g, enc, &vals).expect("quantise");
            let st = a.stats();
            let quant = Quant::for_layer(&enc, st.min, st.max);
            prop_assert_eq!(quant.exact, d < 3);
            let (z, x, y) = native_tile(&g);
            let png = render(&a, &g, &quant, None, z, x, y).expect("render");
            let px = decode_png(&png).expect("decode");
            let mut seen = 0;
            for py in 0..SIZE {
                for pxx in 0..SIZE {
                    let (lon, lat) = pixel_centre(z, x, y, pxx, py);
                    let (q, b) = px[py * SIZE + pxx];
                    match g.cell_id(lat, lon) {
                        None => prop_assert_eq!(q, NODATA_Q),
                        Some(cell) => {
                            let src = a.value(cell as usize);
                            let dec = quant.value(q);
                            match (src, dec) {
                                (None, None) => prop_assert_eq!(b, 0),
                                (Some(s), Some(v)) => {
                                    seen += 1;
                                    prop_assert_eq!(b, 255);
                                    if quant.exact {
                                        // the raw value itself
                                        let raw = a.data.raw(cell as usize);
                                        let shift = if enc.dtype == Dtype::I16 { 32_768.0 } else { 0.0 };
                                        prop_assert_eq!(f64::from(q), raw + shift);
                                        prop_assert!((v - s).abs() <= 1e-9 * s.abs().max(1.0));
                                    } else {
                                        prop_assert!((v - s).abs() <= quant.max_error * (1.0 + 1e-9) + 1e-12,
                                            "{v} vs {s} (err {})", quant.max_error);
                                    }
                                }
                                (s, v) => prop_assert!(false, "data mismatch {s:?} {v:?}"),
                            }
                        }
                    }
                }
            }
            prop_assert!(seen > 0 || vals.iter().all(|v| v.is_nan()));
        }

        /// Every overview cell equals the declared reducer over its block of
        /// native cells (mean/sum within the quantisation error, max and
        /// mode exactly), and its share is the valid fraction.
        #[test]
        fn overviews_equal_the_reducer(
            red in 0u8..4,
            rows in 5u32..70, cols in 5u32..70,
            raws in proptest::collection::vec((proptest::bool::weighted(0.2), 0u8..6), 4900),
        ) {
            let ov = [Overview::Mean, Overview::Sum, Overview::Max, Overview::Mode][red as usize];
            let g = small(rows, cols);
            let enc = if ov == Overview::Mode { enc_of(0) } else { enc_of(2) };
            let vals: Vec<f32> = raws[..(rows * cols) as usize].iter()
                .map(|&(nd, c)| if nd { f32::NAN } else { (f64::from(c) * 10.0 * enc.scale) as f32 })
                .collect();
            let a = GridArray::quantise(g, enc, &vals).expect("quantise");
            let st = a.stats();
            let quant = Quant::for_layer(&enc, st.min, st.max);
            let p = Pyramid::build(&a, g, quant, ov, 4);
            for k in 1..=4u8 {
                let f = 1usize << k;
                for row in 0..(rows as usize).div_ceil(f) {
                    for col in 0..(cols as usize).div_ceil(f) {
                        let mut block = Vec::new();
                        let mut total = 0u64;
                        for r in row * f..((row + 1) * f).min(rows as usize) {
                            for c in col * f..((col + 1) * f).min(cols as usize) {
                                total += 1;
                                if let Some(v) = a.value(r * cols as usize + c) {
                                    block.push(v);
                                }
                            }
                        }
                        let (q, share) = p.at(k, row, col).expect("cell");
                        prop_assert_eq!(share, share_byte(block.len() as u64, total));
                        match reduce(&mut block, ov) {
                            None => prop_assert_eq!(q, NODATA_Q),
                            Some(want) => {
                                let got = quant.value(q).expect("value");
                                match ov {
                                    Overview::Max | Overview::Mode => prop_assert_eq!(q, quant.q(want)),
                                    _ => prop_assert!((got - want).abs() <= quant.scale / 2.0 + 1e-9,
                                        "level {k}: {got} vs {want}"),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn overview_tiles_sample_the_pyramid() {
        let g = small(400, 400);
        let enc = enc_of(1);
        #[allow(clippy::cast_precision_loss)]
        let vals: Vec<f32> = (0..160_000).map(|i| ((i % 400) as f32) * 0.01).collect();
        let a = GridArray::quantise(g, enc, &vals).expect("q");
        let st = a.stats();
        let quant = Quant::for_layer(&enc, st.min, st.max);
        let p = Pyramid::build(&a, g, quant, Overview::Mean, MAX_LEVEL);
        assert_eq!(level_for_zoom(&g, 11, 7), 0);
        assert_eq!(level_for_zoom(&g, 10, 7), 0);
        assert_eq!(level_for_zoom(&g, 9, 7), 1);
        assert_eq!(level_for_zoom(&g, 5, 7), 5);
        let z = 8;
        let (x, _, y, _) = tile_range(&g, z);
        let k = level_for_zoom(&g, z, p.depth());
        let px = decode_png(&render(&a, &g, &quant, Some(&p), z, x, y).expect("r")).expect("d");
        let mut checked = 0;
        for py in 0..SIZE {
            for pxx in 0..SIZE {
                let (lon, lat) = pixel_centre(z, x, y, pxx, py);
                if let Some(cell) = g.cell_id(lat, lon) {
                    let (r, c) = (cell / g.cols, cell % g.cols);
                    let f = 1u32 << k;
                    let want = p.at(k, (r / f) as usize, (c / f) as usize).expect("cell");
                    assert_eq!(px[py * SIZE + pxx], want);
                    checked += 1;
                }
            }
        }
        assert!(checked > 100);
        assert!(p.bytes() < 160_000 * 3 / 2);
    }
}
