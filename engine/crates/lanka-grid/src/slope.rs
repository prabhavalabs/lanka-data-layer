//! Terrain slope (Horn 1981, the `gdaldem slope` kernel) on a geographic
//! raster: the pixel width in metres follows the latitude of each row
//! (sphere R = 6,371,007.2 m), so no `-s 111120` scale approximation.
//! Edge pixels replicate their neighbours (one-sided at the block border).

use crate::geotiff::{Block, GeoRef};

const R: f64 = 6_371_007.2;

/// Slope in degrees for band 0 of `elev` (NaN where the pixel or a
/// neighbour is nodata).
#[must_use]
pub fn horn_slope_deg(geo: &GeoRef, elev: &Block, is_nodata: impl Fn(f32) -> bool) -> Vec<f32> {
    let (w, h) = (elev.width as usize, elev.height as usize);
    let mut out = vec![f32::NAN; w * h];
    if w == 0 || h == 0 {
        return out;
    }
    let at = |c: usize, r: usize| -> Option<f64> {
        #[allow(clippy::cast_possible_truncation)]
        let v = elev.get(c as u32, r as u32, 0);
        (!is_nodata(v)).then_some(f64::from(v))
    };
    let dy_m = geo.dy.to_radians() * R;
    for r in 0..h {
        #[allow(clippy::cast_possible_truncation)]
        let (_, lat) = geo.centre(0, elev.y + r as u32);
        let dx_m = geo.dx.to_radians() * R * lat.to_radians().cos();
        let (rn, rs) = (r.saturating_sub(1), (r + 1).min(h - 1));
        for c in 0..w {
            let (cw, ce) = (c.saturating_sub(1), (c + 1).min(w - 1));
            let (Some(a), Some(b), Some(cc), Some(d), Some(e), Some(f), Some(g), Some(hh), Some(i)) = (
                at(cw, rn),
                at(c, rn),
                at(ce, rn),
                at(cw, r),
                at(c, r),
                at(ce, r),
                at(cw, rs),
                at(c, rs),
                at(ce, rs),
            ) else {
                continue;
            };
            let _ = e;
            // Horizontal spacing across the 3×3 window (shrinks at borders).
            #[allow(clippy::cast_precision_loss)]
            let sx = (ce - cw) as f64 / 2.0;
            #[allow(clippy::cast_precision_loss)]
            let sy = (rs - rn) as f64 / 2.0;
            if sx == 0.0 || sy == 0.0 {
                continue;
            }
            let dzdx = ((cc + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * dx_m * sx);
            let dzdy = ((g + 2.0 * hh + i) - (a + 2.0 * b + cc)) / (8.0 * dy_m * sy);
            #[allow(clippy::cast_possible_truncation)]
            {
                out[r * w + c] = dzdx.hypot(dzdy).atan().to_degrees() as f32;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_has_its_slope() {
        // 1 arc-second pixels at 7° N; elevation rises 1 m per metre eastwards → 45°.
        let dx = 1.0 / 3600.0;
        let geo = GeoRef {
            x0: 80.0,
            dx,
            y0: 7.0,
            dy: dx,
            width: 5,
            height: 5,
        };
        let lat = geo.centre(0, 2).1;
        let dx_m = dx.to_radians() * R * lat.to_radians().cos();
        let mut data = Vec::new();
        for _r in 0..5 {
            for c in 0..5 {
                #[allow(clippy::cast_possible_truncation)]
                data.push((f64::from(c) * dx_m) as f32);
            }
        }
        let b = Block {
            x: 0,
            y: 0,
            width: 5,
            height: 5,
            stride: 5,
            bands: 1,
            data,
        };
        let s = horn_slope_deg(&geo, &b, f32::is_nan);
        assert!((f64::from(s[2 * 5 + 2]) - 45.0).abs() < 0.05, "{}", s[12]);
        // Flat ground.
        let flat = Block {
            data: vec![10.0; 25],
            ..b
        };
        assert_eq!(horn_slope_deg(&geo, &flat, f32::is_nan)[12], 0.0);
    }
}
