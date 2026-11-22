//! Web-mercator PNG tiles of a grid layer (`/v2/grids/{id}/tiles/{z}/{x}/{y}.png`).
//!
//! Each output pixel covers a lon/lat rectangle; when it is smaller than a
//! cell the cell under its centre is drawn, otherwise the covered cells are
//! aggregated by the layer's overview rule (at most 8 × 8 samples, strided)
//! — `sum` layers are coloured by their per-cell mean so one ramp domain
//! holds at every zoom.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use lanka_core::grid::Grid;
use serde::{Deserialize, Serialize};

use crate::CellValues;
use crate::cog::Overview;

pub const SIZE: usize = 256;
const MAX_SAMPLES: usize = 8;

/// Colouring of a layer (`geo.grid_layers.style`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Style {
    /// Ramp name (`spec/map/1/ramps.json`); resolved to `stops` by the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<String>,
    /// `#rrggbb` stops, low → high.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<String>,
    /// Value domain mapped onto the ramp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<[f64; 2]>,
    /// `linear` (default) or `log` (log10 of the value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<String>,
    /// Categorical colours by value (`"10": "#006400"`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub classes: BTreeMap<String, String>,
    /// Values ≤ this are drawn transparent (e.g. 0 people).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent_at_or_below: Option<f64>,
}

pub fn parse_hex(s: &str) -> Result<[u8; 3]> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 {
        bail!("colour {s:?} is not #rrggbb");
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16);
    Ok([p(0)?, p(2)?, p(4)?])
}

/// A style ready to paint.
#[derive(Debug, Clone)]
pub struct Painter {
    stops: Vec<[u8; 3]>,
    lo: f64,
    hi: f64,
    log: bool,
    classes: BTreeMap<i64, [u8; 3]>,
    floor: Option<f64>,
}

impl Painter {
    pub fn new(style: &Style, fallback_domain: [f64; 2]) -> Result<Self> {
        let stops = style
            .stops
            .iter()
            .map(|s| parse_hex(s))
            .collect::<Result<Vec<_>>>()?;
        let mut classes = BTreeMap::new();
        for (k, v) in &style.classes {
            classes.insert(k.parse::<i64>()?, parse_hex(v)?);
        }
        if stops.is_empty() && classes.is_empty() {
            bail!("style has neither stops nor classes");
        }
        let log = style.scale.as_deref() == Some("log");
        let [mut lo, mut hi] = style.domain.unwrap_or(fallback_domain);
        if log {
            lo = lo.max(1e-9).log10();
            hi = hi.max(1e-9).log10();
        }
        Ok(Self {
            stops,
            lo,
            hi,
            log,
            classes,
            floor: style.transparent_at_or_below,
        })
    }

    /// RGBA of a value.
    #[must_use]
    pub fn paint(&self, v: f64) -> [u8; 4] {
        if self.floor.is_some_and(|f| v <= f) {
            return [0; 4];
        }
        if !self.classes.is_empty() {
            #[allow(clippy::cast_possible_truncation)]
            return self
                .classes
                .get(&(v.round() as i64))
                .map_or([0; 4], |c| [c[0], c[1], c[2], 255]);
        }
        let x = if self.log { v.max(1e-9).log10() } else { v };
        let t = if self.hi > self.lo {
            ((x - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        #[allow(clippy::cast_precision_loss)]
        let pos = t * (self.stops.len() - 1) as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (pos.floor() as usize).min(self.stops.len() - 1);
        let j = (i + 1).min(self.stops.len() - 1);
        let f = pos - pos.floor();
        let mut c = [0u8; 4];
        for (k, ck) in c.iter_mut().take(3).enumerate() {
            let a = f64::from(self.stops[i][k]);
            let b = f64::from(self.stops[j][k]);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                *ck = (a + (b - a) * f).round() as u8;
            }
        }
        c[3] = 255;
        c
    }
}

/// `[west, south, east, north]` of tile `z/x/y`.
#[must_use]
pub fn tile_bounds(z: u8, x: u32, y: u32) -> [f64; 4] {
    let n = f64::from(1u32 << z);
    let lon = |x: f64| x / n * 360.0 - 180.0;
    let lat = |y: f64| {
        (std::f64::consts::PI * (1.0 - 2.0 * y / n))
            .sinh()
            .atan()
            .to_degrees()
    };
    [
        lon(f64::from(x)),
        lat(f64::from(y) + 1.0),
        lon(f64::from(x) + 1.0),
        lat(f64::from(y)),
    ]
}

/// Does tile `z/x/y` touch the grid?
#[must_use]
pub fn intersects(g: &Grid, z: u8, x: u32, y: u32) -> bool {
    let [w, s, e, n] = tile_bounds(z, x, y);
    e > g.lon_min && w < g.lon_max() && n > g.lat_min() && s < g.lat_max
}

/// Renders one 256 × 256 RGBA PNG.
pub fn render_png<V: CellValues + ?Sized>(
    values: &V,
    g: &Grid,
    z: u8,
    x: u32,
    y: u32,
    ov: Overview,
    painter: &Painter,
) -> Result<Vec<u8>> {
    if z > 22 || x >= (1u32 << z) || y >= (1u32 << z) {
        bail!("tile {z}/{x}/{y} out of range");
    }
    let n = f64::from(1u32 << z);
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    #[allow(clippy::cast_precision_loss)]
    let sz = SIZE as f64;
    let lat_of = |py: f64| {
        (std::f64::consts::PI * (1.0 - 2.0 * (f64::from(y) + py / sz) / n))
            .sinh()
            .atan()
            .to_degrees()
    };
    let lon_of = |px: f64| (f64::from(x) + px / sz) / n * 360.0 - 180.0;
    let (rows, cols) = (i64::from(g.rows), i64::from(g.cols));
    #[allow(clippy::cast_possible_truncation)]
    let to_col = |lon: f64| ((lon - g.lon_min) / g.step).floor() as i64;
    #[allow(clippy::cast_possible_truncation)]
    let to_row = |lat: f64| ((g.lat_max - lat) / g.step).floor() as i64;
    let mut buf = Vec::with_capacity(MAX_SAMPLES * MAX_SAMPLES);
    for py in 0..SIZE {
        #[allow(clippy::cast_precision_loss)]
        let (lat_n, lat_s) = (lat_of(py as f64), lat_of(py as f64 + 1.0));
        let (r0, r1) = (to_row(lat_n), to_row(lat_s));
        if r1 < 0 || r0 >= rows {
            continue;
        }
        for px in 0..SIZE {
            #[allow(clippy::cast_precision_loss)]
            let (lon_w, lon_e) = (lon_of(px as f64), lon_of(px as f64 + 1.0));
            let (c0, c1) = (to_col(lon_w), to_col(lon_e));
            if c1 < 0 || c0 >= cols {
                continue;
            }
            let v = if r0 == r1 && c0 == c1 || (r1 - r0 <= 1 && c1 - c0 <= 1) {
                #[allow(clippy::cast_precision_loss)]
                let (lat_c, lon_c) = (lat_of(py as f64 + 0.5), lon_of(px as f64 + 0.5));
                let (r, c) = (to_row(lat_c), to_col(lon_c));
                if !(0..rows).contains(&r) || !(0..cols).contains(&c) {
                    continue;
                }
                #[allow(clippy::cast_sign_loss)]
                values.value((r * cols + c) as usize)
            } else {
                buf.clear();
                let (rs, re) = (r0.max(0), r1.min(rows - 1));
                let (cs, ce) = (c0.max(0), c1.min(cols - 1));
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let sr = ((re - rs + 1) as usize).div_ceil(MAX_SAMPLES).max(1);
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let sc = ((ce - cs + 1) as usize).div_ceil(MAX_SAMPLES).max(1);
                let mut r = rs;
                while r <= re {
                    let mut c = cs;
                    while c <= ce {
                        #[allow(clippy::cast_sign_loss)]
                        if let Some(v) = values.value((r * cols + c) as usize) {
                            buf.push(v);
                        }
                        #[allow(clippy::cast_possible_wrap)]
                        {
                            c += sc as i64;
                        }
                    }
                    #[allow(clippy::cast_possible_wrap)]
                    {
                        r += sr as i64;
                    }
                }
                aggregate(&mut buf, ov)
            };
            if let Some(v) = v {
                let i = (py * SIZE + px) * 4;
                rgba[i..i + 4].copy_from_slice(&painter.paint(v));
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 256, 256);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(&rgba)?;
    }
    Ok(out)
}

fn aggregate(v: &mut [f64], ov: Overview) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    Some(match ov {
        Overview::Mean | Overview::Sum => v.iter().sum::<f64>() / v.len() as f64,
        Overview::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Overview::Mode => {
            v.sort_by(f64::total_cmp);
            let (mut best, mut n, mut i) = (v[0], 0, 0);
            while i < v.len() {
                let mut j = i;
                while j < v.len() && v[j] == v[i] {
                    j += 1;
                }
                if j - i > n {
                    best = v[i];
                    n = j - i;
                }
                i = j;
            }
            best
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_png_over_colombo() {
        let g = Grid::CANONICAL;
        #[allow(clippy::cast_precision_loss)]
        let vals: Vec<f32> = (0..crate::cells(&g)).map(|i| (i % 2700) as f32).collect();
        let style = Style {
            stops: vec!["#000000".into(), "#ffffff".into()],
            domain: Some([0.0, 2700.0]),
            ..Style::default()
        };
        let p = Painter::new(&style, [0.0, 1.0]).expect("painter");
        // z=10 tile over Colombo (79.85 E, 6.93 N): x = 739, y = 492.
        assert!(intersects(&g, 10, 739, 492));
        assert!(!intersects(&g, 10, 0, 0));
        let png = render_png(vals.as_slice(), &g, 10, 739, 492, Overview::Mean, &p).expect("png");
        assert_eq!(&png[1..4], b"PNG");
        let low = render_png(vals.as_slice(), &g, 6, 46, 30, Overview::Mean, &p).expect("png");
        assert_eq!(&low[1..4], b"PNG");
        assert_eq!(p.paint(0.0), [0, 0, 0, 255]);
        assert_eq!(p.paint(2700.0), [255, 255, 255, 255]);
    }
}
