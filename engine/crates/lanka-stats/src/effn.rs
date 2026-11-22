//! Effective sample size of a correlation between two spatially
//! autocorrelated variables (Clifford, Richardson & Hémon 1989; Dutilleul
//! 1993): `M = 1 + tr(Bx)·tr(By) / tr(Bx·By)` with `B = P Σ̂ P`, `P` the
//! centring matrix and `Σ̂` the autocovariance estimated per distance class.
//!
//! Distances are great-circle (km) between unit representative points;
//! `classes` equal-width classes over `(0, max distance]` (13 by default,
//! SpatialPack's choice). The covariance of class *k* is the mean of
//! `x_i·x_j` over its ordered pairs (centred values). One O(n²) pass over
//! the pairs collects per-class sums and per-unit class counts; the traces
//! follow in closed form from them, so no n×n matrix is stored.
//! `M` is clamped to `[2, n]`.

use libm::{asin, cos, floor, sin, sqrt};

/// Default number of distance classes.
pub const DEFAULT_CLASSES: usize = 13;

const EARTH_KM: f64 = 6_371.008_8;

/// Great-circle distance (km) by the haversine formula.
#[must_use]
pub fn haversine_km(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let r = core::f64::consts::PI / 180.0;
    let (p1, p2) = (lat1 * r, lat2 * r);
    let dp = p2 - p1;
    let dl = (lon2 - lon1) * r;
    let s1 = sin(dp / 2.0);
    let s2 = sin(dl / 2.0);
    let a = s1 * s1 + cos(p1) * cos(p2) * s2 * s2;
    2.0 * EARTH_KM * asin(sqrt(a.min(1.0)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveN {
    pub n: usize,
    /// Effective sample size, in `[2, n]`.
    pub m: f64,
    pub classes: usize,
    pub max_km: f64,
}

/// Effective sample size of `(x, y)` at points `(lon, lat)`; rows with a
/// non-finite value are dropped. `None` below 4 complete rows.
#[must_use]
pub fn effective_n(
    x: &[f64],
    y: &[f64],
    lon: &[f64],
    lat: &[f64],
    classes: usize,
) -> Option<EffectiveN> {
    let rows: Vec<usize> = (0..x.len())
        .filter(|&i| {
            x[i].is_finite() && y[i].is_finite() && lon[i].is_finite() && lat[i].is_finite()
        })
        .collect();
    let n = rows.len();
    if n < 4 || classes == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f64;
    let mx = rows.iter().map(|&i| x[i]).sum::<f64>() / nf;
    let my = rows.iter().map(|&i| y[i]).sum::<f64>() / nf;
    let xs: Vec<f64> = rows.iter().map(|&i| x[i] - mx).collect();
    let ys: Vec<f64> = rows.iter().map(|&i| y[i] - my).collect();
    let lo: Vec<f64> = rows.iter().map(|&i| lon[i]).collect();
    let la: Vec<f64> = rows.iter().map(|&i| lat[i]).collect();

    let mut max_km = 0.0_f64;
    for i in 0..n {
        for j in (i + 1)..n {
            max_km = max_km.max(haversine_km(lo[i], la[i], lo[j], la[j]));
        }
    }
    let width = if max_km > 0.0 {
        #[allow(clippy::cast_precision_loss)]
        let w = max_km / classes as f64;
        w
    } else {
        1.0
    };
    let k = classes;
    // per-class ordered-pair counts and Σ x_i x_j; per-unit class counts
    let mut npairs = vec![0.0_f64; k];
    let mut sxx = vec![0.0_f64; k];
    let mut syy = vec![0.0_f64; k];
    let mut cnt = vec![0_u32; n * k];
    for i in 0..n {
        for j in (i + 1)..n {
            let d = haversine_km(lo[i], la[i], lo[j], la[j]);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let c = (floor(d / width) as usize).min(k - 1);
            npairs[c] += 2.0;
            sxx[c] += 2.0 * xs[i] * xs[j];
            syy[c] += 2.0 * ys[i] * ys[j];
            cnt[i * k + c] += 1;
            cnt[j * k + c] += 1;
        }
    }
    let cx: Vec<f64> = (0..k)
        .map(|c| {
            if npairs[c] > 0.0 {
                sxx[c] / npairs[c]
            } else {
                0.0
            }
        })
        .collect();
    let cy: Vec<f64> = (0..k)
        .map(|c| {
            if npairs[c] > 0.0 {
                syy[c] / npairs[c]
            } else {
                0.0
            }
        })
        .collect();
    let vx = xs.iter().map(|v| v * v).sum::<f64>() / nf;
    let vy = ys.iter().map(|v| v * v).sum::<f64>() / nf;
    // r_i = (1/n) Σ_j Σ_ij ; g = (1/n) Σ_i r_i
    let row = |c: &[f64], v: f64, i: usize| -> f64 {
        let mut s = v;
        for (kk, ck) in c.iter().enumerate() {
            s += f64::from(cnt[i * k + kk]) * ck;
        }
        s / nf
    };
    let rx: Vec<f64> = (0..n).map(|i| row(&cx, vx, i)).collect();
    let ry: Vec<f64> = (0..n).map(|i| row(&cy, vy, i)).collect();
    let gx = rx.iter().sum::<f64>() / nf;
    let gy = ry.iter().sum::<f64>() / nf;
    let tr_bx = nf * vx - nf * gx;
    let tr_by = nf * vy - nf * gy;
    // a_i = r_i − g/2 so that B_ij = Σ_ij − a_i − a_j
    let ax: Vec<f64> = rx.iter().map(|r| r - gx / 2.0).collect();
    let ay: Vec<f64> = ry.iter().map(|r| r - gy / 2.0).collect();
    let mut s_sxsy = nf * vx * vy;
    for c in 0..k {
        s_sxsy += npairs[c] * cx[c] * cy[c];
    }
    let s_ay_rx: f64 = (0..n).map(|i| ay[i] * rx[i]).sum();
    let s_ax_ry: f64 = (0..n).map(|i| ax[i] * ry[i]).sum();
    let s_axay: f64 = (0..n).map(|i| ax[i] * ay[i]).sum();
    let (sax, say): (f64, f64) = (ax.iter().sum(), ay.iter().sum());
    let tr_bxby =
        s_sxsy - 2.0 * nf * s_ay_rx - 2.0 * nf * s_ax_ry + 2.0 * nf * s_axay + 2.0 * sax * say;
    if tr_bxby.is_nan() || tr_bxby <= 0.0 {
        return None;
    }
    let m = (1.0 + tr_bx * tr_by / tr_bxby).clamp(2.0, nf);
    Some(EffectiveN {
        n,
        m,
        classes: k,
        max_km,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Stream;

    fn points(n: usize) -> (Vec<f64>, Vec<f64>) {
        let mut s = Stream::new(5, 0);
        (0..n)
            .map(|_| (79.8 + s.unit() * 2.0, 6.0 + s.unit() * 3.5))
            .unzip()
    }

    #[test]
    fn independent_noise_keeps_most_units() {
        let (lon, lat) = points(200);
        let mut s = Stream::new(9, 1);
        let x: Vec<f64> = (0..200).map(|_| s.unit()).collect();
        let y: Vec<f64> = (0..200).map(|_| s.unit()).collect();
        let e = effective_n(&x, &y, &lon, &lat, DEFAULT_CLASSES).expect("m");
        assert!(e.m > 120.0, "{e:?}");
    }

    #[test]
    fn smooth_fields_have_far_fewer_effective_units() {
        let (lon, lat) = points(200);
        let x: Vec<f64> = lat.iter().map(|v| v * 2.0).collect();
        let y: Vec<f64> = lat.iter().zip(&lon).map(|(a, b)| a + 0.1 * b).collect();
        let e = effective_n(&x, &y, &lon, &lat, DEFAULT_CLASSES).expect("m");
        assert!(e.m < 40.0, "{e:?}");
    }
}
