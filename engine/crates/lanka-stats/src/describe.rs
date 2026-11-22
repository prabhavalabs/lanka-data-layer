//! Univariate summaries, quantiles, ranks, histograms and hexbins.

use libm::{ceil, floor, log2, round, sqrt};

/// The finite values of `v`, in order.
#[must_use]
pub fn finite(v: &[f64]) -> Vec<f64> {
    v.iter().copied().filter(|x| x.is_finite()).collect()
}

/// Ascending copy of the finite values (total order; NaN never present).
#[must_use]
pub fn sorted_finite(v: &[f64]) -> Vec<f64> {
    let mut s = finite(v);
    s.sort_by(f64::total_cmp);
    s
}

/// Quantile of sorted data, linear interpolation (R type 7, numpy default).
#[must_use]
pub fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    match sorted.len() {
        0 => f64::NAN,
        1 => sorted[0],
        n => {
            #[allow(clippy::cast_precision_loss)]
            let h = (n - 1) as f64 * p.clamp(0.0, 1.0);
            let f = floor(h);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let lo = f as usize;
            let hi = (lo + 1).min(n - 1);
            sorted[lo] + (h - f) * (sorted[hi] - sorted[lo])
        }
    }
}

/// Median of sorted data.
#[must_use]
pub fn median_sorted(sorted: &[f64]) -> f64 {
    quantile_sorted(sorted, 0.5)
}

/// Arithmetic mean (NaN for no values).
#[must_use]
pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    #[allow(clippy::cast_precision_loss)]
    let n = v.len() as f64;
    v.iter().sum::<f64>() / n
}

/// Sample variance (n − 1), two-pass.
#[must_use]
pub fn variance(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return f64::NAN;
    }
    let m = mean(v);
    #[allow(clippy::cast_precision_loss)]
    let d = (v.len() - 1) as f64;
    v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / d
}

/// Average ranks (1-based, ties get the mean of their positions; scipy
/// `rankdata(method="average")`).
#[must_use]
pub fn ranks(v: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]).then(a.cmp(&b)));
    let mut out = vec![0.0; v.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
            j += 1;
        }
        #[allow(clippy::cast_precision_loss)]
        let r = (i + j) as f64 / 2.0 + 1.0;
        for &k in &idx[i..=j] {
            out[k] = r;
        }
        i = j + 1;
    }
    out
}

/// Sizes of the tie groups of `v` (only groups of 2 or more).
#[must_use]
pub fn tie_sizes(v: &[f64]) -> Vec<usize> {
    let s = sorted_finite(v);
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let mut j = i;
        while j + 1 < s.len() && s[j + 1] == s[i] {
            j += 1;
        }
        if j > i {
            out.push(j - i + 1);
        }
        i = j + 1;
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// `bins + 1` edges.
    pub edges: Vec<f64>,
    pub counts: Vec<u32>,
}

/// Equal-width histogram over `[min, max]`; the last bin is closed.
/// `bins = 0` chooses Sturges' rule clamped to 5–50.
#[must_use]
pub fn histogram(v: &[f64], bins: usize) -> Histogram {
    let s = sorted_finite(v);
    if s.is_empty() {
        return Histogram {
            edges: Vec::new(),
            counts: Vec::new(),
        };
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let k = if bins == 0 {
        (ceil(log2(s.len() as f64)) as usize + 1).clamp(5, 50)
    } else {
        bins
    };
    let (lo, hi) = (s[0], s[s.len() - 1]);
    let span = if hi > lo { hi - lo } else { 1.0 };
    #[allow(clippy::cast_precision_loss)]
    let edges: Vec<f64> = (0..=k)
        .map(|i| {
            if i == k {
                lo + span
            } else {
                lo + span * i as f64 / k as f64
            }
        })
        .collect();
    let mut counts = vec![0_u32; k];
    for &x in &s {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let b = (floor((x - lo) / span * k as f64) as usize).min(k - 1);
        counts[b] += 1;
    }
    Histogram { edges, counts }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub missing: usize,
    pub mean: f64,
    pub sd: f64,
    pub min: f64,
    pub max: f64,
    /// p0, p5, p25, p50, p75, p95, p100 (type 7).
    pub q: [f64; 7],
    /// Median absolute deviation (unscaled).
    pub mad: f64,
    /// Adjusted Fisher–Pearson skewness G1 (scipy `skew(bias=False)`).
    pub skewness: f64,
    /// Excess kurtosis G2 (scipy `kurtosis(bias=False)`).
    pub kurtosis: f64,
    /// Values ≤ 0 (decides whether a log transform is possible).
    pub non_positive: usize,
}

/// One pass of univariate statistics.
#[must_use]
pub fn summary(v: &[f64]) -> Summary {
    let s = sorted_finite(v);
    let n = s.len();
    let missing = v.len() - n;
    let m = mean(&s);
    let var = variance(&s);
    let q = [0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 1.0].map(|p| quantile_sorted(&s, p));
    let med = q[3];
    let mut dev: Vec<f64> = s.iter().map(|x| (x - med).abs()).collect();
    dev.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f64;
    let (m2, m3, m4) = s.iter().fold((0.0, 0.0, 0.0), |(a, b, c), x| {
        let d = x - m;
        (a + d * d, b + d * d * d, c + d * d * d * d)
    });
    let (m2, m3, m4) = (m2 / nf, m3 / nf, m4 / nf);
    let g1 = m3 / (m2 * sqrt(m2));
    let skewness = if n > 2 {
        g1 * sqrt(nf * (nf - 1.0)) / (nf - 2.0)
    } else {
        f64::NAN
    };
    let g2 = m4 / (m2 * m2) - 3.0;
    let kurtosis = if n > 3 {
        ((nf + 1.0) * g2 + 6.0) * (nf - 1.0) / ((nf - 2.0) * (nf - 3.0))
    } else {
        f64::NAN
    };
    Summary {
        n,
        missing,
        mean: m,
        sd: sqrt(var),
        min: q[0],
        max: q[6],
        q,
        mad: median_sorted(&dev),
        skewness,
        kurtosis,
        non_positive: s.iter().filter(|&&x| x <= 0.0).count(),
    }
}

/// Pointy-top hexagonal binning of a scatter (UI contract §6.3 `hexbin`).
#[derive(Debug, Clone, PartialEq)]
pub struct Hexbin {
    pub cx: Vec<f64>,
    pub cy: Vec<f64>,
    pub count: Vec<u32>,
    pub radius: f64,
}

/// Bins `(x, y)` into hexagons of `radius` (data units); cells sorted by
/// (row, column) so the output does not depend on point order.
#[must_use]
pub fn hexbin(x: &[f64], y: &[f64], radius: f64) -> Hexbin {
    use std::collections::BTreeMap;
    let w = radius * sqrt(3.0);
    let h = radius * 1.5;
    let mut cells: BTreeMap<(i64, i64), u32> = BTreeMap::new();
    for (&px, &py) in x.iter().zip(y) {
        if !(px.is_finite() && py.is_finite()) || radius <= 0.0 {
            continue;
        }
        // nearest of the two candidate lattices
        #[allow(clippy::cast_possible_truncation)]
        let r0 = round(py / h) as i64;
        let mut best = (0_i64, 0_i64, f64::INFINITY);
        for r in [r0 - 1, r0, r0 + 1] {
            #[allow(clippy::cast_precision_loss)]
            let off = if r.rem_euclid(2) == 1 { w / 2.0 } else { 0.0 };
            #[allow(clippy::cast_possible_truncation)]
            let c = round((px - off) / w) as i64;
            #[allow(clippy::cast_precision_loss)]
            let (ccx, ccy) = (c as f64 * w + off, r as f64 * h);
            let d = (px - ccx) * (px - ccx) + (py - ccy) * (py - ccy);
            if d < best.2 {
                best = (r, c, d);
            }
        }
        *cells.entry((best.0, best.1)).or_default() += 1;
    }
    let mut out = Hexbin {
        cx: Vec::new(),
        cy: Vec::new(),
        count: Vec::new(),
        radius,
    };
    for ((r, c), n) in cells {
        #[allow(clippy::cast_precision_loss)]
        let off = if r.rem_euclid(2) == 1 { w / 2.0 } else { 0.0 };
        #[allow(clippy::cast_precision_loss)]
        {
            out.cx.push(c as f64 * w + off);
            out.cy.push(r as f64 * h);
        }
        out.count.push(n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_ranks() {
        let s = [1.0, 2.0, 3.0, 4.0];
        assert!((quantile_sorted(&s, 0.25) - 1.75).abs() < 1e-15);
        assert_eq!(ranks(&[10.0, 20.0, 10.0, 30.0]), vec![1.5, 3.0, 1.5, 4.0]);
        assert_eq!(tie_sizes(&[1.0, 1.0, 2.0, 3.0, 3.0, 3.0]), vec![2, 3]);
    }

    #[test]
    fn summary_of_textbook_data() {
        let v = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0, f64::NAN];
        let s = summary(&v);
        assert_eq!((s.n, s.missing), (8, 1));
        assert!((s.mean - 5.0).abs() < 1e-15);
        assert!((s.sd - (32.0_f64 / 7.0).sqrt()).abs() < 1e-14);
        assert!((s.mad - 0.5).abs() < 1e-15);
    }

    #[test]
    fn histogram_counts_everything() {
        let v: Vec<f64> = (0..100).map(f64::from).collect();
        let h = histogram(&v, 10);
        assert_eq!(h.counts.iter().sum::<u32>(), 100);
        assert_eq!(h.counts, vec![10; 10]);
    }

    #[test]
    fn hexbin_counts_and_is_order_free() {
        let x = [0.0, 0.1, 5.0, 5.1, 10.0];
        let y = [0.0, 0.0, 5.0, 5.0, 0.0];
        let a = hexbin(&x, &y, 1.0);
        let xr: Vec<f64> = x.iter().rev().copied().collect();
        let yr: Vec<f64> = y.iter().rev().copied().collect();
        assert_eq!(a, hexbin(&xr, &yr, 1.0));
        assert_eq!(a.count.iter().sum::<u32>(), 5);
    }
}
