//! Flat-array kernels behind the WebAssembly exports (safe Rust; the
//! native parity test calls these directly).

use lanka_stats::classify::{self, Method};
use lanka_stats::correlate;
use lanka_stats::describe;
use lanka_stats::regress::{Design, RegressError, ols};
use lanka_stats::transform::Transform;

fn method(code: u32) -> Method {
    Method::from_code(code).unwrap_or(Method::Quantile)
}

#[must_use]
pub fn classify(values: &[f64], method_code: u32, k: u32) -> Vec<f64> {
    classify::breaks(values, method(method_code), k as usize)
}

#[must_use]
pub fn bivariate(x: &[f64], y: &[f64], method_code: u32, k: u32) -> Vec<f64> {
    let b = classify::bivariate(x, y, method(method_code), k as usize);
    #[allow(clippy::cast_precision_loss)]
    let mut out = vec![b.x_breaks.len() as f64, b.y_breaks.len() as f64];
    out.extend(&b.x_breaks);
    out.extend(&b.y_breaks);
    out.extend(b.class.iter().map(|&c| f64::from(c)));
    out
}

#[must_use]
pub fn correlate(x: &[f64], y: &[f64]) -> Vec<f64> {
    let p = correlate::pearson(x, y, 0.95);
    let s = correlate::spearman(x, y, 0.95);
    #[allow(clippy::cast_precision_loss)]
    let n = p.n as f64;
    vec![n, p.r, p.ci[0], p.ci[1], p.p, s.r, s.ci[0], s.ci[1], s.p]
}

#[must_use]
pub fn regress(y: &[f64], xs: &[f64], k: usize) -> Vec<f64> {
    let n = y.len();
    let cols: Vec<&[f64]> = (0..k).map(|j| &xs[j * n..(j + 1) * n]).collect();
    let d = Design::with_intercept(&cols);
    #[allow(clippy::cast_precision_loss)]
    match ols(&d, y, 0.95) {
        Ok(f) => {
            let mut out = vec![0.0, f.n as f64, f.p as f64];
            out.extend(&f.coef);
            out.extend(&f.se_hc3);
            out.extend(&f.p_hc3);
            out.extend([f.r2, f.adj_r2, f.sigma]);
            out
        }
        Err(RegressError::TooFewRows { .. }) => vec![1.0],
        Err(RegressError::Singular { .. }) => vec![2.0],
    }
}

#[must_use]
pub fn preview(x: &[f64], y: &[f64], tx: u32, ty: u32) -> Vec<f64> {
    let t = |c| Transform::from_code(c).unwrap_or_default();
    let p = lanka_stats::preview_relate(x, y, t(tx), t(ty));
    #[allow(clippy::cast_precision_loss)]
    let n = p.n as f64;
    vec![n, p.r, p.rho, p.intercept, p.slope]
}

#[must_use]
pub fn describe(values: &[f64]) -> Vec<f64> {
    let s = describe::summary(values);
    #[allow(clippy::cast_precision_loss)]
    let v = vec![
        s.n as f64,
        s.missing as f64,
        s.mean,
        s.sd,
        s.min,
        s.max,
        s.q[1],
        s.q[2],
        s.q[3],
        s.q[4],
        s.q[5],
        s.mad,
        s.skewness,
        s.kurtosis,
        s.non_positive as f64,
    ];
    v
}

#[must_use]
pub fn hexbin(x: &[f64], y: &[f64], radius: f64) -> Vec<f64> {
    let h = describe::hexbin(x, y, radius);
    #[allow(clippy::cast_precision_loss)]
    let mut out = vec![h.count.len() as f64, h.radius];
    out.extend(&h.cx);
    out.extend(&h.cy);
    out.extend(h.count.iter().map(|&c| f64::from(c)));
    out
}
