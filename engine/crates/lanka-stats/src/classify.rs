//! Class breaks for styling (10 §4, 11 §4 `classify`): quantile, equal,
//! jenks, stddev, headtail, pretty — and bivariate classes.
//!
//! Breaks are the `k − 1` inner thresholds, strictly ascending (ties
//! collapse classes rather than leave empty ones); a value belongs to the
//! first class whose break it is below (`class_of`). The same contract as
//! the web client's `classBreaks`/`classIndex`, so a legend drawn in the
//! browser and one served by the API agree by construction.

use libm::{ceil, fabs, floor, log10, pow, sqrt};

use crate::describe::{mean, quantile_sorted, sorted_finite, variance};

/// Classification method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Quantile,
    Equal,
    Jenks,
    StdDev,
    HeadTail,
    Pretty,
}

impl Method {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "quantile" => Self::Quantile,
            "equal" => Self::Equal,
            "jenks" => Self::Jenks,
            "stddev" => Self::StdDev,
            "headtail" => Self::HeadTail,
            "pretty" => Self::Pretty,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quantile => "quantile",
            Self::Equal => "equal",
            Self::Jenks => "jenks",
            Self::StdDev => "stddev",
            Self::HeadTail => "headtail",
            Self::Pretty => "pretty",
        }
    }

    /// Numeric code for the WebAssembly interface.
    #[must_use]
    pub const fn from_code(c: u32) -> Option<Self> {
        Some(match c {
            0 => Self::Quantile,
            1 => Self::Equal,
            2 => Self::Jenks,
            3 => Self::StdDev,
            4 => Self::HeadTail,
            5 => Self::Pretty,
            _ => return None,
        })
    }
}

/// Above this many values Jenks runs on an evenly spaced sample of the
/// sorted data (`round(i / (cap − 1) · (n − 1))`), as the web client does.
pub const JENKS_CAP: usize = 2000;

fn strictly_ascending(b: Vec<f64>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(b.len());
    for v in b {
        if v.is_finite() && out.last().is_none_or(|&l| v > l) {
            out.push(v);
        }
    }
    out
}

/// Fisher–Jenks optimal breaks (minimum within-class sum of squares) on
/// sorted data.
fn jenks(sorted_all: &[f64], k: usize) -> Vec<f64> {
    let s: Vec<f64> = if sorted_all.len() > JENKS_CAP {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        (0..JENKS_CAP)
            .map(|i| {
                let t = i as f64 / (JENKS_CAP - 1) as f64 * (sorted_all.len() - 1) as f64;
                sorted_all[floor(t + 0.5) as usize]
            })
            .collect()
    } else {
        sorted_all.to_vec()
    };
    let n = s.len();
    if n <= k {
        return s.get(1..).map(<[f64]>::to_vec).unwrap_or_default();
    }
    let mut p1 = vec![0.0; n + 1];
    let mut p2 = vec![0.0; n + 1];
    for i in 0..n {
        p1[i + 1] = p1[i] + s[i];
        p2[i + 1] = p2[i] + s[i] * s[i];
    }
    #[allow(clippy::cast_precision_loss)]
    let ssd = |a: usize, b: usize| {
        let m = (b - a) as f64;
        let sum = p1[b] - p1[a];
        p2[b] - p2[a] - sum * sum / m
    };
    let mut cost = vec![vec![f64::INFINITY; k + 1]; n + 1];
    let mut cut = vec![vec![0_usize; k + 1]; n + 1];
    for (i, row) in cost.iter_mut().enumerate().skip(1) {
        row[1] = ssd(0, i);
    }
    for c in 2..=k {
        for i in c..=n {
            for j in (c - 1)..i {
                let v = cost[j][c - 1] + ssd(j, i);
                if v < cost[i][c] {
                    cost[i][c] = v;
                    cut[i][c] = j;
                }
            }
        }
    }
    let mut breaks = Vec::with_capacity(k - 1);
    let mut i = n;
    for c in (2..=k).rev() {
        i = cut[i][c];
        breaks.push(s[i]);
    }
    breaks.reverse();
    breaks
}

/// R's `pretty()` (base R `R_pretty`, `n = 5` style arguments): the
/// sequence `ns·unit … nu·unit` covering `[lo, hi]`.
#[must_use]
pub fn pretty(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    let h = 1.5;
    let h5 = 0.5 + 1.5 * h;
    let min_n = (n / 3).max(1);
    let shrink = 0.75;
    let dx = hi - lo;
    #[allow(clippy::cast_precision_loss)]
    let nf = n.max(1) as f64;
    let (mut cell, small) = if dx == 0.0 && hi == 0.0 {
        (1.0, true)
    } else {
        let cell = fabs(lo).max(fabs(hi));
        let u = 1.0
            + if h5 >= 1.5 * h + 0.5 {
                1.0 / (1.0 + h)
            } else {
                1.5 / (1.0 + h5)
            };
        let u = u * nf * f64::EPSILON;
        (cell, dx < cell * u * 3.0)
    };
    if small {
        if cell > 10.0 {
            cell = 9.0 + cell / 10.0;
        }
        cell *= shrink;
        #[allow(clippy::cast_precision_loss)]
        if min_n > 1 {
            cell /= min_n as f64;
        }
    } else {
        cell = dx;
        if n > 1 {
            cell /= nf;
        }
    }
    cell = cell.clamp(20.0 * f64::MIN_POSITIVE, 0.1 * f64::MAX);
    let base = pow(10.0, floor(log10(cell)));
    let mut unit = base;
    if 2.0 * base - cell < h * (cell - unit) {
        unit = 2.0 * base;
        if 5.0 * base - cell < h5 * (cell - unit) {
            unit = 5.0 * base;
            if 10.0 * base - cell < h * (cell - unit) {
                unit = 10.0 * base;
            }
        }
    }
    let eps = 1e-10;
    let mut ns = floor(lo / unit + eps);
    let mut nu = ceil(hi / unit - eps);
    while ns * unit > lo + eps * unit {
        ns -= 1.0;
    }
    while nu * unit < hi - eps * unit {
        nu += 1.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let k = floor(0.5 + nu - ns) as i64;
    #[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]
    if k < min_n as i64 {
        let d = min_n as i64 - k;
        if ns >= 0.0 {
            nu += (d / 2) as f64;
            ns -= (d / 2 + d % 2) as f64;
        } else {
            ns -= (d / 2) as f64;
            nu += (d / 2 + d % 2) as f64;
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let steps = floor(nu - ns + 0.5) as i64;
    #[allow(clippy::cast_precision_loss)]
    (0..=steps).map(|i| (ns + i as f64) * unit).collect()
}

/// The `k − 1` inner breaks of `values` (non-finite values ignored).
#[must_use]
pub fn breaks(values: &[f64], method: Method, k: usize) -> Vec<f64> {
    let s = sorted_finite(values);
    if s.is_empty() || k < 2 {
        return Vec::new();
    }
    let (lo, hi) = (s[0], s[s.len() - 1]);
    #[allow(clippy::cast_precision_loss)]
    let kf = k as f64;
    let raw: Vec<f64> = match method {
        #[allow(clippy::cast_precision_loss)]
        Method::Quantile => (1..k).map(|i| quantile_sorted(&s, i as f64 / kf)).collect(),
        #[allow(clippy::cast_precision_loss)]
        Method::Equal => (1..k).map(|i| lo + (hi - lo) * i as f64 / kf).collect(),
        Method::Jenks => jenks(&s, k),
        Method::StdDev => {
            let m = mean(&s);
            let sd = sqrt(variance(&s));
            if sd.is_nan() || sd <= 0.0 {
                return Vec::new();
            }
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
            let half = (k as i64 - 1) as f64 / 2.0;
            #[allow(clippy::cast_precision_loss)]
            (0..k - 1)
                .map(|i| m + (i as f64 - half + 0.5) * sd)
                .filter(|b| *b > lo && *b < hi)
                .collect()
        }
        Method::HeadTail => {
            let mut out = Vec::new();
            let mut head: Vec<f64> = s.clone();
            while out.len() < k - 1 && head.len() > 1 {
                let m = mean(&head);
                let next: Vec<f64> = head.iter().copied().filter(|v| *v > m).collect();
                if next.is_empty() || next.len() == head.len() {
                    break;
                }
                out.push(m);
                #[allow(clippy::cast_precision_loss)]
                let share = next.len() as f64 / head.len() as f64;
                head = next;
                if share > 0.4 {
                    break;
                }
            }
            out
        }
        Method::Pretty => pretty(lo, hi, k)
            .into_iter()
            .filter(|b| *b > lo && *b < hi)
            .collect(),
    };
    strictly_ascending(raw)
}

/// Class of `v` under `breaks`: the number of breaks `≤ v`.
#[must_use]
pub fn class_of(v: f64, breaks: &[f64]) -> u8 {
    let mut i = 0;
    while i < breaks.len() && v >= breaks[i] {
        i += 1;
    }
    u8::try_from(i).unwrap_or(u8::MAX)
}

/// Classes of every value (`u8::MAX` for non-finite).
#[must_use]
pub fn classes(values: &[f64], breaks: &[f64]) -> Vec<u8> {
    values
        .iter()
        .map(|&v| {
            if v.is_finite() {
                class_of(v, breaks)
            } else {
                u8::MAX
            }
        })
        .collect()
}

/// Bivariate classes: `class_y · k + class_x` (row-major, y as rows).
#[derive(Debug, Clone, PartialEq)]
pub struct Bivariate {
    pub x_breaks: Vec<f64>,
    pub y_breaks: Vec<f64>,
    pub class: Vec<u8>,
}

#[must_use]
pub fn bivariate(x: &[f64], y: &[f64], method: Method, k: usize) -> Bivariate {
    let bx = breaks(x, method, k);
    let by = breaks(y, method, k);
    let kk = u8::try_from(k).unwrap_or(u8::MAX);
    let class = x
        .iter()
        .zip(y)
        .map(|(&a, &b)| {
            if a.is_finite() && b.is_finite() {
                class_of(b, &by)
                    .saturating_mul(kk)
                    .saturating_add(class_of(a, &bx))
            } else {
                u8::MAX
            }
        })
        .collect();
    Bivariate {
        x_breaks: bx,
        y_breaks: by,
        class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_pretty_textbook() {
        assert_eq!(
            pretty(0.0, 1.0, 5),
            vec![0.0, 0.2, 0.4, 0.6000000000000001, 0.8, 1.0]
        );
        assert_eq!(
            pretty(1.0, 100.0, 5),
            vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]
        );
        assert_eq!(
            pretty(-3.7, 8.2, 5),
            vec![-4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
        );
        assert_eq!(
            pretty(0.0, 7.0, 5),
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
        );
    }

    #[test]
    fn quantile_equal_and_classes() {
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(breaks(&v, Method::Equal, 3), vec![4.0, 7.0]);
        assert_eq!(breaks(&v, Method::Quantile, 2), vec![5.5]);
        let b = breaks(&v, Method::Quantile, 4);
        assert_eq!(classes(&[1.0, 5.0, 10.0, f64::NAN], &b), vec![0, 1, 3, 255]);
    }

    #[test]
    fn jenks_finds_the_gaps() {
        let v = [1.0, 2.0, 3.0, 10.0, 11.0, 12.0, 30.0, 31.0, 32.0];
        assert_eq!(breaks(&v, Method::Jenks, 3), vec![10.0, 30.0]);
    }

    #[test]
    fn ties_collapse_and_bivariate_codes() {
        let v = [1.0, 1.0, 1.0, 1.0, 2.0];
        assert_eq!(breaks(&v, Method::Quantile, 4), vec![1.0]);
        let x = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [5.0, 4.0, 3.0, 2.0, 1.0, 0.0];
        let b = bivariate(&x, &y, Method::Quantile, 3);
        assert_eq!(b.class[0], 6); // x low, y high
        assert_eq!(b.class[5], 2); // x high, y low
    }

    #[test]
    fn headtail_and_stddev_are_inside_the_range() {
        let v: Vec<f64> = (1..=100).map(|i| f64::from(i * i)).collect();
        for m in [Method::HeadTail, Method::StdDev, Method::Pretty] {
            let b = breaks(&v, m, 5);
            assert!(!b.is_empty(), "{m:?}");
            assert!(b.iter().all(|x| *x > 1.0 && *x < 10_000.0), "{m:?} {b:?}");
        }
    }
}
