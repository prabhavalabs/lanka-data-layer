//! Two-group comparisons: Welch's t, Mann–Whitney U (normal approximation
//! with tie and continuity corrections, scipy `method="asymptotic"`),
//! Hodges–Lehmann shift, Cohen's d and seeded bootstrap intervals.

use libm::sqrt;

use crate::describe::{finite, mean, median_sorted, quantile_sorted, ranks, tie_sizes, variance};
use crate::rng::Stream;
use crate::special::{norm_sf, t_p2, t_ppf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Welch {
    /// mean(a) − mean(b)
    pub diff: f64,
    pub t: f64,
    pub df: f64,
    pub p: f64,
    pub ci: [f64; 2],
}

/// Welch's unequal-variance t test of `a` against `b`.
#[must_use]
pub fn welch(a: &[f64], b: &[f64], level: f64) -> Welch {
    let (a, b) = (finite(a), finite(b));
    #[allow(clippy::cast_precision_loss)]
    let (na, nb) = (a.len() as f64, b.len() as f64);
    let (va, vb) = (variance(&a) / na, variance(&b) / nb);
    let diff = mean(&a) - mean(&b);
    let se = sqrt(va + vb);
    let t = diff / se;
    let df = (va + vb) * (va + vb) / (va * va / (na - 1.0) + vb * vb / (nb - 1.0));
    let q = t_ppf(1.0 - (1.0 - level) / 2.0, df);
    Welch {
        diff,
        t,
        df,
        p: t_p2(t, df),
        ci: [diff - q * se, diff + q * se],
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MannWhitney {
    /// U of the first sample.
    pub u: f64,
    pub z: f64,
    pub p: f64,
}

/// Mann–Whitney U, two-sided, normal approximation with tie correction
/// and continuity correction.
#[must_use]
pub fn mann_whitney(a: &[f64], b: &[f64]) -> MannWhitney {
    let (a, b) = (finite(a), finite(b));
    let (n1, n2) = (a.len(), b.len());
    let all: Vec<f64> = a.iter().chain(&b).copied().collect();
    let r = ranks(&all);
    let r1: f64 = r[..n1].iter().sum();
    #[allow(clippy::cast_precision_loss)]
    let (f1, f2) = (n1 as f64, n2 as f64);
    let u1 = r1 - f1 * (f1 + 1.0) / 2.0;
    let u2 = f1 * f2 - u1;
    let n = f1 + f2;
    #[allow(clippy::cast_precision_loss)]
    let ties: f64 = tie_sizes(&all)
        .iter()
        .map(|&t| {
            let t = t as f64;
            t * t * t - t
        })
        .sum();
    let s = sqrt(f1 * f2 / 12.0 * ((n + 1.0) - ties / (n * (n - 1.0))));
    let mu = f1 * f2 / 2.0;
    let u = u1.max(u2);
    let z = (u - mu - 0.5) / s;
    MannWhitney {
        u: u1,
        z,
        p: (2.0 * norm_sf(z)).min(1.0),
    }
}

/// Pairs above which [`hodges_lehmann`] uses a seeded sample of pairs.
pub const HL_MAX_PAIRS: usize = 4_000_000;

/// Hodges–Lehmann shift: median of all differences `a_i − b_j`. Above
/// [`HL_MAX_PAIRS`] pairs, the median of that many pairs drawn from
/// `(seed, stream 0x484c)`; the second value says whether it sampled.
#[must_use]
pub fn hodges_lehmann(a: &[f64], b: &[f64], seed: u64) -> (f64, bool) {
    let (a, b) = (finite(a), finite(b));
    if a.is_empty() || b.is_empty() {
        return (f64::NAN, false);
    }
    let total = a.len().saturating_mul(b.len());
    let mut d: Vec<f64> = if total <= HL_MAX_PAIRS {
        let mut d = Vec::with_capacity(total);
        for x in &a {
            for y in &b {
                d.push(x - y);
            }
        }
        d
    } else {
        let mut s = Stream::new(seed, 0x484c);
        (0..HL_MAX_PAIRS)
            .map(|_| a[s.below(a.len())] - b[s.below(b.len())])
            .collect()
    };
    d.sort_by(f64::total_cmp);
    (median_sorted(&d), total > HL_MAX_PAIRS)
}

/// Cohen's d with the pooled standard deviation.
#[must_use]
pub fn cohens_d(a: &[f64], b: &[f64]) -> f64 {
    let (a, b) = (finite(a), finite(b));
    #[allow(clippy::cast_precision_loss)]
    let (na, nb) = (a.len() as f64, b.len() as f64);
    let sp = sqrt(((na - 1.0) * variance(&a) + (nb - 1.0) * variance(&b)) / (na + nb - 2.0));
    (mean(&a) - mean(&b)) / sp
}

/// Statistic for [`bootstrap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    Mean,
    Median,
}

fn stat_of(v: &mut [f64], s: Stat) -> f64 {
    match s {
        Stat::Mean => mean(v),
        Stat::Median => {
            v.sort_by(f64::total_cmp);
            median_sorted(v)
        }
    }
}

/// Percentile bootstrap interval of `stat(a)` (one sample, `b = None`) or
/// `stat(a) − stat(b)`: `reps` resamples drawn from `(seed, stream)`
/// (within each group, `a` first), type-7 quantiles at (1 ± level)/2.
#[must_use]
pub fn bootstrap(
    a: &[f64],
    b: Option<&[f64]>,
    stat: Stat,
    reps: usize,
    level: f64,
    seed: u64,
    stream: u64,
) -> [f64; 2] {
    let a = finite(a);
    let b = b.map(finite);
    if a.is_empty() || b.as_ref().is_some_and(Vec::is_empty) || reps == 0 {
        return [f64::NAN, f64::NAN];
    }
    let mut s = Stream::new(seed, stream);
    let mut ra = vec![0.0; a.len()];
    let mut rb = vec![0.0; b.as_ref().map_or(0, Vec::len)];
    let mut out = Vec::with_capacity(reps);
    for _ in 0..reps {
        for v in &mut ra {
            *v = a[s.below(a.len())];
        }
        let mut x = stat_of(&mut ra, stat);
        if let Some(b) = &b {
            for v in &mut rb {
                *v = b[s.below(b.len())];
            }
            x -= stat_of(&mut rb, stat);
        }
        out.push(x);
    }
    out.sort_by(f64::total_cmp);
    let alpha = (1.0 - level) / 2.0;
    [
        quantile_sorted(&out, alpha),
        quantile_sorted(&out, 1.0 - alpha),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welch_textbook() {
        let a = [19.1, 21.0, 22.4, 20.3, 23.5, 21.8];
        let b = [17.2, 18.9, 16.4, 19.9, 18.1];
        let w = welch(&a, &b, 0.95);
        assert!(w.diff > 0.0 && w.p < 0.01, "{w:?}");
        assert!(w.ci[0] < w.diff && w.diff < w.ci[1]);
    }

    #[test]
    fn hodges_lehmann_of_a_shift() {
        let a: Vec<f64> = (0..50).map(|i| f64::from(i) + 3.0).collect();
        let b: Vec<f64> = (0..50).map(f64::from).collect();
        assert_eq!(hodges_lehmann(&a, &b, 1).0, 3.0);
    }

    #[test]
    fn bootstrap_is_seeded() {
        let a: Vec<f64> = (0..40).map(|i| f64::from(i % 7)).collect();
        let x = bootstrap(&a, None, Stat::Median, 500, 0.9, 99, 1);
        let y = bootstrap(&a, None, Stat::Median, 500, 0.9, 99, 1);
        assert_eq!(x, y);
        assert!(x[0] <= 3.0 && 3.0 <= x[1]);
    }
}
