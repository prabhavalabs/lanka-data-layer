//! Special functions and distributions used by the kernels.
//!
//! Every transcendental call goes through `libm` (pure Rust), never the
//! platform's maths library, so a WebAssembly build returns the same bits
//! as the native one. Accuracy targets ~1e-14 relative in the ranges the
//! kernels use (p-values down to 1e-300, quantiles at 0.5–0.9995).

use libm::{erfc, exp, fabs, lgamma, log, log1p, sqrt};

/// `ln Γ(x)` for `x > 0`.
#[must_use]
pub fn ln_gamma(x: f64) -> f64 {
    lgamma(x)
}

/// `ln B(a, b)`.
#[must_use]
pub fn ln_beta(a: f64, b: f64) -> f64 {
    lgamma(a) + lgamma(b) - lgamma(a + b)
}

/// Continued fraction of the incomplete beta (modified Lentz).
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    const EPS: f64 = 1e-16;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if fabs(d) < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=10_000 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if fabs(d) < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if fabs(c) < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if fabs(d) < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if fabs(c) < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if fabs(del - 1.0) < EPS {
            break;
        }
    }
    h
}

/// Regularised incomplete beta `I_x(a, b)`.
#[must_use]
pub fn beta_inc(a: f64, b: f64, x: f64) -> f64 {
    if x.is_nan() || a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_front = a * log(x) + b * log1p(-x) - ln_beta(a, b);
    if x < (a + 1.0) / (a + b + 2.0) {
        exp(ln_front) * betacf(a, b, x) / a
    } else {
        1.0 - exp(ln_front) * betacf(b, a, 1.0 - x) / b
    }
}

/// `1 − I_x(a, b)` without cancellation for small results.
#[must_use]
pub fn beta_inc_c(a: f64, b: f64, x: f64) -> f64 {
    if x.is_nan() || a <= 0.0 || b <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 1.0;
    }
    if x >= 1.0 {
        return 0.0;
    }
    let ln_front = a * log(x) + b * log1p(-x) - ln_beta(a, b);
    if x < (a + 1.0) / (a + b + 2.0) {
        1.0 - exp(ln_front) * betacf(a, b, x) / a
    } else {
        exp(ln_front) * betacf(b, a, 1.0 - x) / b
    }
}

/// Regularised lower incomplete gamma `P(a, x)` and upper `Q(a, x)`.
#[must_use]
pub fn gamma_inc(a: f64, x: f64) -> (f64, f64) {
    if x.is_nan() || a <= 0.0 {
        return (f64::NAN, f64::NAN);
    }
    if x <= 0.0 {
        return (0.0, 1.0);
    }
    let ln_front = a * log(x) - x - lgamma(a);
    if x < a + 1.0 {
        // series
        let mut ap = a;
        let mut sum = 1.0 / a;
        let mut del = sum;
        for _ in 0..100_000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if fabs(del) < fabs(sum) * 1e-17 {
                break;
            }
        }
        let p = sum * exp(ln_front);
        (p, 1.0 - p)
    } else {
        // continued fraction (Lentz)
        const TINY: f64 = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / TINY;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..=100_000 {
            let i = f64::from(i);
            let an = -i * (i - a);
            b += 2.0;
            d = an * d + b;
            if fabs(d) < TINY {
                d = TINY;
            }
            c = b + an / c;
            if fabs(c) < TINY {
                c = TINY;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if fabs(del - 1.0) < 1e-16 {
                break;
            }
        }
        let q = exp(ln_front) * h;
        (1.0 - q, q)
    }
}

/// Standard normal CDF.
#[must_use]
pub fn norm_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / core::f64::consts::SQRT_2)
}

/// Standard normal upper tail `1 − Φ(z)`.
#[must_use]
pub fn norm_sf(z: f64) -> f64 {
    0.5 * erfc(z / core::f64::consts::SQRT_2)
}

/// Standard normal quantile (Acklam's rational approximation refined by
/// two Halley steps on `erfc`; ~1e-15 relative).
#[must_use]
pub fn norm_ppf(p: f64) -> f64 {
    if p.is_nan() || !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    if p == 0.0 {
        return f64::NEG_INFINITY;
    }
    if p == 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    let lo = 0.02425;
    let mut x = if p < lo {
        let q = sqrt(-2.0 * log(p));
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - lo {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = sqrt(-2.0 * log1p(-p));
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    for _ in 0..2 {
        // Halley on whichever tail keeps precision
        let e = if p < 0.5 {
            norm_cdf(x) - p
        } else {
            (1.0 - p) - norm_sf(x)
        };
        let u = e * sqrt(2.0 * core::f64::consts::PI) * exp(x * x / 2.0);
        x -= u / (1.0 + x * u / 2.0);
    }
    x
}

/// Student t CDF.
#[must_use]
pub fn t_cdf(t: f64, df: f64) -> f64 {
    if t.is_nan() || df.is_nan() || df <= 0.0 {
        return f64::NAN;
    }
    if t.is_infinite() {
        return if t > 0.0 { 1.0 } else { 0.0 };
    }
    let x = df / (df + t * t);
    let tail = 0.5 * beta_inc(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - tail } else { tail }
}

/// Two-sided p-value of a t statistic.
#[must_use]
pub fn t_p2(t: f64, df: f64) -> f64 {
    if t.is_nan() || df.is_nan() || df <= 0.0 {
        return f64::NAN;
    }
    if t.is_infinite() {
        return 0.0;
    }
    beta_inc(df / 2.0, 0.5, df / (df + t * t)).min(1.0)
}

/// Student t quantile (Newton on the CDF from the normal start, guarded by
/// bisection; converged to the last representable step).
#[must_use]
pub fn t_ppf(p: f64, df: f64) -> f64 {
    if p.is_nan() || df.is_nan() || df <= 0.0 || !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    if p == 0.5 {
        return 0.0;
    }
    if p < 0.5 {
        return -t_ppf(1.0 - p, df);
    }
    if p == 1.0 {
        return f64::INFINITY;
    }
    // upper-tail probability q = 1 - p, solved on the tail for precision
    let q = 1.0 - p;
    let tail = |t: f64| 0.5 * beta_inc(df / 2.0, 0.5, df / (df + t * t));
    let ln_c = lgamma((df + 1.0) / 2.0) - lgamma(df / 2.0) - 0.5 * log(df * core::f64::consts::PI);
    let pdf = |t: f64| exp(ln_c - (df + 1.0) / 2.0 * log1p(t * t / df));
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    while tail(hi) > q {
        lo = hi;
        hi *= 2.0;
        if hi > 1e300 {
            return f64::INFINITY;
        }
    }
    let mut t = norm_ppf(p).clamp(lo, hi);
    for _ in 0..200 {
        let f = tail(t) - q; // decreasing in t
        if f > 0.0 {
            lo = t;
        } else {
            hi = t;
        }
        let step = f / pdf(t);
        let mut next = t + step;
        if !(next > lo && next < hi) || !next.is_finite() {
            next = 0.5 * (lo + hi);
        }
        if fabs(next - t) <= 4.0 * f64::EPSILON * fabs(t) {
            t = next;
            break;
        }
        t = next;
    }
    t
}

/// Chi-squared upper tail.
#[must_use]
pub fn chi2_sf(x: f64, df: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    gamma_inc(df / 2.0, x / 2.0).1
}

/// F distribution upper tail.
#[must_use]
pub fn f_sf(f: f64, d1: f64, d2: f64) -> f64 {
    if f.is_nan() || d1 <= 0.0 || d2 <= 0.0 {
        return f64::NAN;
    }
    if f <= 0.0 {
        return 1.0;
    }
    beta_inc(d2 / 2.0, d1 / 2.0, d2 / (d2 + d1 * f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-300)
    }

    #[test]
    fn textbook_values() {
        assert!(close(norm_ppf(0.975), 1.959_963_984_540_054, 1e-14));
        assert!(close(norm_ppf(0.05), -1.644_853_626_951_472_2, 1e-14));
        assert!(close(norm_cdf(1.96), 0.975_002_104_851_780, 1e-13));
        // t(10) 97.5 % = 2.228138851986273
        assert!(close(t_ppf(0.975, 10.0), 2.228_138_851_986_273, 1e-13));
        assert!(close(t_ppf(0.975, 1.0), 12.706_204_736_174_7, 1e-12));
        assert!(close(t_p2(2.228_138_851_986_273, 10.0), 0.05, 1e-12));
        assert!(close(chi2_sf(3.841_458_820_694_124, 1.0), 0.05, 1e-12));
        assert!(close(f_sf(4.964_602_743_730_711, 1.0, 10.0), 0.05, 1e-11));
        assert!(close(beta_inc(2.0, 3.0, 0.4), 0.5248, 1e-13));
    }

    #[test]
    fn tails_stay_accurate() {
        // t = 20, df = 300 → p ≈ 6.6e-58 (scipy: 2*t.sf(20, 300))
        let p = t_p2(20.0, 300.0);
        assert!(p > 0.0 && p < 1e-50, "{p}");
        assert!(close(norm_sf(10.0), 7.619_853_024_160_527e-24, 1e-12));
    }
}
