//! Minimal spatial weights and Moran's I for tier 1 (WP-29): k-nearest-
//! neighbour weights from unit representative points, row-standardised, and
//! Moran's I with a seeded permutation test — enough for `relate`/`regress`
//! to test their residuals (guard `RESIDUAL_AUTOCORR`). Contiguity weights,
//! Geary, Lee's L and local statistics arrive with WP-30.
//!
//! Neighbours are found on an equirectangular projection (x = lon·cos φ̄,
//! y = lat) with a bucket grid; ties are broken by index, so the weights do
//! not depend on input order beyond the order of the rows themselves.

use libm::{cos, floor, sqrt};

use crate::rng::Stream;

/// Row-standardised sparse weights (CSR): row `i` holds `k` neighbours of
/// weight `1/k`.
#[derive(Debug, Clone, PartialEq)]
pub struct Weights {
    pub n: usize,
    pub k: usize,
    /// `n·k` neighbour indices, row-major, nearest first.
    pub neighbours: Vec<u32>,
}

/// k nearest neighbours of every point (excluding itself).
///
/// # Panics
/// When `lon` and `lat` differ in length.
#[must_use]
pub fn knn(lon: &[f64], lat: &[f64], k: usize) -> Weights {
    assert_eq!(lon.len(), lat.len());
    let n = lon.len();
    let k = k.min(n.saturating_sub(1));
    if n == 0 || k == 0 {
        return Weights {
            n,
            k,
            neighbours: Vec::new(),
        };
    }
    #[allow(clippy::cast_precision_loss)]
    let lat0 = lat.iter().sum::<f64>() / n as f64;
    let c = cos(lat0 * core::f64::consts::PI / 180.0);
    let xs: Vec<f64> = lon.iter().map(|v| v * c).collect();
    let ys = lat;
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for i in 0..n {
        x0 = x0.min(xs[i]);
        x1 = x1.max(xs[i]);
        y0 = y0.min(ys[i]);
        y1 = y1.max(ys[i]);
    }
    // ~4 points per bucket
    #[allow(clippy::cast_precision_loss)]
    let cell = (((x1 - x0).max(1e-9) * (y1 - y0).max(1e-9)) / (n as f64 / 4.0)).max(1e-18);
    let cell = sqrt(cell);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let gx = (floor((x1 - x0) / cell) as usize) + 1;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let gy = (floor((y1 - y0) / cell) as usize) + 1;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bucket_of = |i: usize| -> (usize, usize) {
        (
            (floor((xs[i] - x0) / cell) as usize).min(gx - 1),
            (floor((ys[i] - y0) / cell) as usize).min(gy - 1),
        )
    };
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); gx * gy];
    for i in 0..n {
        let (bx, by) = bucket_of(i);
        buckets[by * gx + bx].push(u32::try_from(i).unwrap_or(u32::MAX));
    }
    let mut neighbours = Vec::with_capacity(n * k);
    let mut cand: Vec<(f64, u32)> = Vec::new();
    for i in 0..n {
        let (bx, by) = bucket_of(i);
        let mut ring = 0_usize;
        cand.clear();
        loop {
            // visit the ring at Chebyshev distance `ring`
            let (lx, hx) = (bx.saturating_sub(ring), (bx + ring).min(gx - 1));
            let (ly, hy) = (by.saturating_sub(ring), (by + ring).min(gy - 1));
            for yy in ly..=hy {
                for xx in lx..=hx {
                    let on_ring =
                        xx + ring == bx || xx == bx + ring || yy + ring == by || yy == by + ring;
                    if ring > 0 && !on_ring {
                        continue;
                    }
                    for &j in &buckets[yy * gx + xx] {
                        let j_us = j as usize;
                        if j_us == i {
                            continue;
                        }
                        let dx = xs[j_us] - xs[i];
                        let dy = ys[j_us] - ys[i];
                        cand.push((dx * dx + dy * dy, j));
                    }
                }
            }
            let covered = lx == 0 && ly == 0 && hx == gx - 1 && hy == gy - 1;
            if cand.len() >= k {
                cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                // every unvisited point is at least `ring · cell` away
                #[allow(clippy::cast_precision_loss)]
                let safe = ring as f64 * cell;
                if covered || cand[k - 1].0 <= safe * safe {
                    break;
                }
            } else if covered {
                break;
            }
            ring += 1;
        }
        cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        neighbours.extend(cand.iter().take(k).map(|c| c.1));
    }
    Weights { n, k, neighbours }
}

/// Moran's I of `v` under row-standardised `w` (NaN-free input).
#[must_use]
pub fn moran_i(v: &[f64], w: &Weights) -> f64 {
    let n = w.n;
    if n < 3 || w.k == 0 {
        return f64::NAN;
    }
    #[allow(clippy::cast_precision_loss)]
    let m = v.iter().sum::<f64>() / n as f64;
    let z: Vec<f64> = v.iter().map(|x| x - m).collect();
    moran_centred(&z, w)
}

fn moran_centred(z: &[f64], w: &Weights) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let inv_k = 1.0 / w.k as f64;
    let mut num = 0.0;
    for i in 0..w.n {
        let mut lag = 0.0;
        for &j in &w.neighbours[i * w.k..(i + 1) * w.k] {
            lag += z[j as usize];
        }
        num += z[i] * lag * inv_k;
    }
    let den: f64 = z.iter().map(|x| x * x).sum();
    num / den
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoranTest {
    pub i: f64,
    /// E[I] = −1/(n − 1).
    pub expected: f64,
    /// Pseudo p, one-sided (greater): (1 + #{I_perm ≥ I}) / (R + 1).
    pub p_perm: f64,
    pub permutations: u32,
}

/// Moran's I with `permutations` Fisher–Yates shuffles of the values drawn
/// from `(seed, stream)`.
#[must_use]
pub fn moran_test(v: &[f64], w: &Weights, permutations: u32, seed: u64, stream: u64) -> MoranTest {
    let n = w.n;
    #[allow(clippy::cast_precision_loss)]
    let m = v.iter().sum::<f64>() / n as f64;
    let mut z: Vec<f64> = v.iter().map(|x| x - m).collect();
    let i_obs = moran_centred(&z, w);
    let mut s = Stream::new(seed, stream);
    let mut ge = 0_u32;
    for _ in 0..permutations {
        for a in (1..n).rev() {
            let b = s.below(a + 1);
            z.swap(a, b);
        }
        if moran_centred(&z, w) >= i_obs {
            ge += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    MoranTest {
        i: i_obs,
        expected: -1.0 / (n as f64 - 1.0),
        p_perm: f64::from(ge + 1) / f64::from(permutations + 1),
        permutations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lattice(side: usize) -> (Vec<f64>, Vec<f64>) {
        #[allow(clippy::cast_precision_loss)]
        (0..side * side)
            .map(|i| {
                (
                    80.0 + (i % side) as f64 * 0.01,
                    7.0 + (i / side) as f64 * 0.01,
                )
            })
            .unzip()
    }

    #[test]
    fn knn_matches_brute_force() {
        let mut s = Stream::new(1, 2);
        let lon: Vec<f64> = (0..300).map(|_| 79.7 + s.unit() * 2.0).collect();
        let lat: Vec<f64> = (0..300).map(|_| 5.9 + s.unit() * 3.9).collect();
        let w = knn(&lon, &lat, 6);
        let c = cos(lat.iter().sum::<f64>() / 300.0 * core::f64::consts::PI / 180.0);
        for i in 0..300 {
            let mut d: Vec<(f64, u32)> = (0..300)
                .filter(|&j| j != i)
                .map(|j| {
                    let dx = lon[j] * c - lon[i] * c;
                    let dy = lat[j] - lat[i];
                    (dx * dx + dy * dy, j as u32)
                })
                .collect();
            d.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let want: Vec<u32> = d.iter().take(6).map(|x| x.1).collect();
            assert_eq!(
                &w.neighbours[i * 6..i * 6 + 6],
                want.as_slice(),
                "point {i}"
            );
        }
    }

    #[test]
    fn moran_sign_on_a_lattice() {
        let (lon, lat) = lattice(20);
        let w = knn(&lon, &lat, 4);
        let smooth: Vec<f64> = lat.clone();
        let t = moran_test(&smooth, &w, 199, 7, 1);
        assert!(t.i > 0.8 && t.p_perm <= 0.01, "{t:?}");
        let chess: Vec<f64> = (0..400)
            .map(|i| f64::from(((i % 20) + (i / 20)) as u32 % 2))
            .collect();
        assert!(moran_i(&chess, &w) < -0.5);
        let again = moran_test(&smooth, &w, 199, 7, 1);
        assert_eq!(t, again);
    }
}
