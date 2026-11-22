//! Pearson and Spearman correlation with intervals and p-values.
//!
//! - Pearson *r*: *p* from *t* = r·√((n−2)/(1−r²)) on n − 2 df (equal to
//!   scipy's exact beta test); interval by Fisher's z, z ± z_{α/2}/√(n−3)
//!   (scipy `PearsonRResult.confidence_interval()`).
//! - Spearman ρ: Pearson on average ranks; *p* as above (scipy
//!   `spearmanr`); interval by Fisher's z with the Fieller–Hartley–Pearson
//!   (1957) variance 1.06/(n−3).

use libm::{atanh, sqrt, tanh};

use crate::describe::ranks;
use crate::special::{norm_ppf, t_p2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correlation {
    pub r: f64,
    pub n: usize,
    pub ci: [f64; 2],
    pub ci_level: f64,
    /// Two-sided p (independent observations).
    pub p: f64,
    pub t: f64,
}

/// Complete pairs of `x` and `y`.
#[must_use]
pub fn complete(x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>) {
    x.iter()
        .zip(y)
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .map(|(a, b)| (*a, *b))
        .unzip()
}

/// Pearson's r of complete pairs (NaN when undefined).
#[must_use]
pub fn pearson_r(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return f64::NAN;
    }
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f64;
    let mx = x[..n].iter().sum::<f64>() / nf;
    let my = y[..n].iter().sum::<f64>() / nf;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx == 0.0 || syy == 0.0 {
        return f64::NAN;
    }
    (sxy / sqrt(sxx * syy)).clamp(-1.0, 1.0)
}

fn finish(r: f64, n: usize, level: f64, var_factor: f64) -> Correlation {
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f64;
    let df = nf - 2.0;
    let t = if r.abs() >= 1.0 {
        f64::INFINITY.copysign(r)
    } else {
        r * sqrt(df / ((1.0 - r) * (1.0 + r)))
    };
    let p = if n > 2 && r.is_finite() {
        t_p2(t, df)
    } else {
        f64::NAN
    };
    let ci = if n > 3 && r.is_finite() && r.abs() < 1.0 {
        let z = atanh(r);
        let se = sqrt(var_factor / (nf - 3.0));
        let q = norm_ppf(1.0 - (1.0 - level) / 2.0);
        [tanh(z - q * se), tanh(z + q * se)]
    } else {
        [f64::NAN, f64::NAN]
    };
    Correlation {
        r,
        n,
        ci,
        ci_level: level,
        p,
        t,
    }
}

/// Pearson correlation of the complete pairs.
#[must_use]
pub fn pearson(x: &[f64], y: &[f64], level: f64) -> Correlation {
    let (a, b) = complete(x, y);
    finish(pearson_r(&a, &b), a.len(), level, 1.0)
}

/// Spearman's ρ of the complete pairs.
#[must_use]
pub fn spearman(x: &[f64], y: &[f64], level: f64) -> Correlation {
    let (a, b) = complete(x, y);
    let r = pearson_r(&ranks(&a), &ranks(&b));
    finish(r, a.len(), level, 1.06)
}

/// *p* of *r* with an effective sample size (Dutilleul 1993's modified
/// test: F = (M − 1)·r²/(1 − r²) on (1, M − 2) df).
#[must_use]
pub fn p_adjusted(r: f64, n_effective: f64) -> f64 {
    if n_effective.is_nan() || n_effective <= 2.0 || !r.is_finite() {
        return f64::NAN;
    }
    if r.abs() >= 1.0 {
        return 0.0;
    }
    let f = (n_effective - 1.0) * r * r / ((1.0 - r) * (1.0 + r));
    crate::special::f_sf(f, 1.0, n_effective - 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anscombe_one() {
        // Anscombe (1973) set I: r = 0.81642051634484
        let x = [10.0, 8.0, 13.0, 9.0, 11.0, 14.0, 6.0, 4.0, 12.0, 7.0, 5.0];
        let y = [
            8.04, 6.95, 7.58, 8.81, 8.33, 9.96, 7.24, 4.26, 10.84, 4.82, 5.68,
        ];
        let c = pearson(&x, &y, 0.95);
        assert!((c.r - 0.816_420_516_344_843_6).abs() < 1e-12, "{}", c.r);
        assert!((c.p - 0.002_169_628_873_078_8).abs() < 1e-9, "{}", c.p);
        let s = spearman(&x, &y, 0.95);
        assert!((s.r - 0.818_181_818_181_818_2).abs() < 1e-12);
    }

    #[test]
    fn symmetric_and_incomplete_pairs_dropped() {
        let x = [1.0, 2.0, f64::NAN, 4.0, 5.0, 7.0];
        let y = [2.0, 1.0, 3.0, 5.0, 4.0, 9.0];
        let a = pearson(&x, &y, 0.95);
        let b = pearson(&y, &x, 0.95);
        assert_eq!(a.n, 5);
        assert_eq!(a.r.to_bits(), b.r.to_bits());
    }

    #[test]
    fn adjusted_p_is_larger_with_fewer_effective_units() {
        let p_full = p_adjusted(0.3, 300.0);
        let p_eff = p_adjusted(0.3, 40.0);
        assert!(p_eff > p_full);
    }
}
