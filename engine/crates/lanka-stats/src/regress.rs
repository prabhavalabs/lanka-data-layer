//! Least squares: OLS with classical and HC3 standard errors, leverage,
//! Cook's distance, VIF, Breusch–Pagan (Koenker) and Huber's robust M
//! regression by IRLS.
//!
//! Householder QR in plain loops (no BLAS, no SIMD dispatch), so the
//! summation order — and therefore every bit — is the same on every
//! target. Agreement with statsmodels (`OLS`, `cov_type="HC3"`,
//! `OLSInfluence`, `variance_inflation_factor`, `het_breuschpagan`,
//! `RLM(M=HuberT())`) is checked by the reference fixtures.

// Index loops keep the summation order explicit (bit-identical targets).
#![allow(clippy::needless_range_loop)]

use libm::sqrt;

use crate::special::{chi2_sf, t_p2, t_ppf};

/// Gaussian consistency constant of the MAD: Φ⁻¹(3/4).
pub const MAD_CONSTANT: f64 = 0.674_489_750_196_081_7;
/// Huber's tuning constant (95 % efficiency at the normal).
pub const HUBER_T: f64 = 1.345;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegressError {
    /// Fewer observations than parameters + 1.
    TooFewRows { n: usize, p: usize },
    /// The design matrix is (numerically) rank deficient.
    Singular { column: usize },
}

impl std::fmt::Display for RegressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewRows { n, p } => write!(f, "{n} rows for {p} parameters"),
            Self::Singular { column } => write!(f, "design is rank deficient at column {column}"),
        }
    }
}

impl std::error::Error for RegressError {}

/// Householder QR of an n×p column-major matrix: R (p×p, upper) and the
/// reflectors to apply Qᵀ.
struct Qr {
    n: usize,
    p: usize,
    /// Column-major n×p: R in the upper triangle; reflector tails below.
    a: Vec<f64>,
    /// Reflector heads (v[k] of each column) and scales.
    vk: Vec<f64>,
    beta: Vec<f64>,
}

impl Qr {
    fn new(n: usize, p: usize, mut a: Vec<f64>) -> Result<Self, RegressError> {
        let mut vk = vec![0.0; p];
        let mut beta = vec![0.0; p];
        // scale for the rank tolerance
        let norm0: f64 = (0..p)
            .map(|j| sqrt((0..n).map(|i| a[j * n + i] * a[j * n + i]).sum::<f64>()))
            .fold(0.0, f64::max);
        for k in 0..p {
            let col = k * n;
            let mut norm = 0.0;
            for i in k..n {
                norm += a[col + i] * a[col + i];
            }
            let norm = sqrt(norm);
            #[allow(clippy::cast_precision_loss)]
            if norm <= 1e-12 * norm0.max(1.0) * (n as f64) {
                return Err(RegressError::Singular { column: k });
            }
            let alpha = if a[col + k] > 0.0 { -norm } else { norm };
            let v0 = a[col + k] - alpha;
            // v = (v0, a[k+1..n]); beta = 2 / vᵀv
            let mut vtv = v0 * v0;
            for i in (k + 1)..n {
                vtv += a[col + i] * a[col + i];
            }
            beta[k] = if vtv == 0.0 { 0.0 } else { 2.0 / vtv };
            vk[k] = v0;
            a[col + k] = alpha;
            for j in (k + 1)..p {
                let cj = j * n;
                let mut s = v0 * a[cj + k];
                for i in (k + 1)..n {
                    s += a[col + i] * a[cj + i];
                }
                let s = s * beta[k];
                a[cj + k] -= s * v0;
                for i in (k + 1)..n {
                    a[cj + i] -= s * a[col + i];
                }
            }
        }
        Ok(Self { n, p, a, vk, beta })
    }

    /// Qᵀ y (in place).
    fn qt(&self, y: &mut [f64]) {
        let n = self.n;
        for k in 0..self.p {
            let col = k * n;
            let mut s = self.vk[k] * y[k];
            for i in (k + 1)..n {
                s += self.a[col + i] * y[i];
            }
            let s = s * self.beta[k];
            y[k] -= s * self.vk[k];
            for i in (k + 1)..n {
                y[i] -= s * self.a[col + i];
            }
        }
    }

    fn r(&self, i: usize, j: usize) -> f64 {
        self.a[j * self.n + i]
    }

    /// Solves R b = c (back substitution).
    fn solve_r(&self, c: &[f64]) -> Vec<f64> {
        let p = self.p;
        let mut b = vec![0.0; p];
        for i in (0..p).rev() {
            let mut s = c[i];
            for j in (i + 1)..p {
                s -= self.r(i, j) * b[j];
            }
            b[i] = s / self.r(i, i);
        }
        b
    }

    /// R⁻¹ (upper triangular, row-major p×p).
    fn r_inv(&self) -> Vec<f64> {
        let p = self.p;
        let mut inv = vec![0.0; p * p];
        for j in 0..p {
            // column j of R⁻¹: solve R x = e_j
            let mut e = vec![0.0; p];
            e[j] = 1.0;
            let x = self.solve_r(&e);
            for i in 0..p {
                inv[i * p + j] = x[i];
            }
        }
        inv
    }
}

/// A design: column-major n×p with an intercept in column 0 when asked.
#[derive(Debug, Clone)]
pub struct Design {
    pub n: usize,
    pub p: usize,
    pub x: Vec<f64>,
}

impl Design {
    /// `[1, x_1, …, x_k]` from predictor columns of equal length.
    #[must_use]
    pub fn with_intercept(predictors: &[&[f64]]) -> Self {
        let n = predictors.first().map_or(0, |c| c.len());
        let p = predictors.len() + 1;
        let mut x = vec![1.0; n];
        for c in predictors {
            x.extend_from_slice(&c[..n]);
        }
        Self { n, p, x }
    }

    fn col(&self, j: usize) -> &[f64] {
        &self.x[j * self.n..(j + 1) * self.n]
    }

    fn row_dot(&self, i: usize, b: &[f64]) -> f64 {
        (0..self.p).map(|j| self.x[j * self.n + i] * b[j]).sum()
    }
}

/// Weighted least-squares core: coefficients, R⁻¹ and W = √w·X·R⁻¹.
struct Fit {
    coef: Vec<f64>,
    r_inv: Vec<f64>,
    /// row-major n×p: rows of √w·X·R⁻¹ (leverage = row norms²).
    w: Vec<f64>,
}

fn wls(d: &Design, y: &[f64], weights: Option<&[f64]>) -> Result<Fit, RegressError> {
    let (n, p) = (d.n, d.p);
    if n <= p {
        return Err(RegressError::TooFewRows { n, p });
    }
    let sw: Vec<f64> = match weights {
        Some(w) => w.iter().map(|v| sqrt(*v)).collect(),
        None => vec![1.0; n],
    };
    let mut a = d.x.clone();
    for j in 0..p {
        for i in 0..n {
            a[j * n + i] *= sw[i];
        }
    }
    let qr = Qr::new(n, p, a)?;
    let mut qy: Vec<f64> = y.iter().zip(&sw).map(|(v, s)| v * s).collect();
    qr.qt(&mut qy);
    let coef = qr.solve_r(&qy[..p]);
    let r_inv = qr.r_inv();
    let mut w = vec![0.0; n * p];
    for i in 0..n {
        for j in 0..p {
            // (√w·x_i)ᵀ R⁻¹ column j; R⁻¹ is upper triangular
            let mut s = 0.0;
            for k in 0..=j {
                s += d.x[k * n + i] * sw[i] * r_inv[k * p + j];
            }
            w[i * p + j] = s;
        }
    }
    Ok(Fit { coef, r_inv, w })
}

/// `R⁻¹ M R⁻ᵀ` for a symmetric p×p `M` (row-major).
fn sandwich(r_inv: &[f64], m: &[f64], p: usize) -> Vec<f64> {
    let mut t = vec![0.0; p * p];
    for i in 0..p {
        for j in 0..p {
            let mut s = 0.0;
            for k in i..p {
                s += r_inv[i * p + k] * m[k * p + j];
            }
            t[i * p + j] = s;
        }
    }
    let mut out = vec![0.0; p * p];
    for i in 0..p {
        for j in 0..p {
            let mut s = 0.0;
            for k in j..p {
                s += t[i * p + k] * r_inv[j * p + k];
            }
            out[i * p + j] = s;
        }
    }
    out
}

/// An OLS fit.
#[derive(Debug, Clone, PartialEq)]
pub struct Ols {
    pub n: usize,
    pub p: usize,
    pub coef: Vec<f64>,
    /// Classical standard errors.
    pub se: Vec<f64>,
    /// HC3 (MacKinnon & White 1985) standard errors.
    pub se_hc3: Vec<f64>,
    /// t = b / se_hc3 and its two-sided p on n − p df.
    pub t_hc3: Vec<f64>,
    pub p_hc3: Vec<f64>,
    /// b ± t_{1−α/2, n−p} · se_hc3.
    pub ci_hc3: Vec<[f64; 2]>,
    pub ci_level: f64,
    pub sigma: f64,
    pub r2: f64,
    pub adj_r2: f64,
    pub df_resid: f64,
    pub fitted: Vec<f64>,
    pub resid: Vec<f64>,
    pub leverage: Vec<f64>,
    pub cooks: Vec<f64>,
}

/// Ordinary least squares of `y` on the design.
pub fn ols(d: &Design, y: &[f64], level: f64) -> Result<Ols, RegressError> {
    let (n, p) = (d.n, d.p);
    let f = wls(d, y, None)?;
    let fitted: Vec<f64> = (0..n).map(|i| d.row_dot(i, &f.coef)).collect();
    let resid: Vec<f64> = y.iter().zip(&fitted).map(|(a, b)| a - b).collect();
    #[allow(clippy::cast_precision_loss)]
    let (nf, pf) = (n as f64, p as f64);
    let df = nf - pf;
    let rss: f64 = resid.iter().map(|e| e * e).sum();
    let s2 = rss / df;
    let leverage: Vec<f64> = (0..n)
        .map(|i| (0..p).map(|j| f.w[i * p + j] * f.w[i * p + j]).sum())
        .collect();
    // (XᵀX)⁻¹ = R⁻¹R⁻ᵀ
    let ident: Vec<f64> = (0..p * p)
        .map(|k| if k / p == k % p { 1.0 } else { 0.0 })
        .collect();
    let xtx_inv = sandwich(&f.r_inv, &ident, p);
    let se: Vec<f64> = (0..p).map(|j| sqrt(s2 * xtx_inv[j * p + j])).collect();
    // HC3 meat in the W basis: Wᵀ diag(e²/(1−h)²) W
    let mut meat = vec![0.0; p * p];
    for i in 0..n {
        let om = resid[i] * resid[i] / ((1.0 - leverage[i]) * (1.0 - leverage[i]));
        for a in 0..p {
            for b in 0..p {
                meat[a * p + b] += f.w[i * p + a] * f.w[i * p + b] * om;
            }
        }
    }
    let cov3 = sandwich(&f.r_inv, &meat, p);
    let se_hc3: Vec<f64> = (0..p).map(|j| sqrt(cov3[j * p + j])).collect();
    let q = t_ppf(1.0 - (1.0 - level) / 2.0, df);
    let t_hc3: Vec<f64> = f.coef.iter().zip(&se_hc3).map(|(b, s)| b / s).collect();
    let p_hc3: Vec<f64> = t_hc3.iter().map(|t| t_p2(*t, df)).collect();
    let ci_hc3 = f
        .coef
        .iter()
        .zip(&se_hc3)
        .map(|(b, s)| [b - q * s, b + q * s])
        .collect();
    let has_const = (0..n).all(|i| d.x[i] == 1.0);
    let ybar = if has_const {
        y.iter().sum::<f64>() / nf
    } else {
        0.0
    };
    let tss: f64 = y.iter().map(|v| (v - ybar) * (v - ybar)).sum();
    let r2 = 1.0 - rss / tss;
    let adj_r2 = if has_const {
        1.0 - (1.0 - r2) * (nf - 1.0) / df
    } else {
        1.0 - (1.0 - r2) * nf / df
    };
    let cooks = (0..n)
        .map(|i| {
            let h = leverage[i];
            resid[i] * resid[i] / (s2 * (1.0 - h)) * h / (1.0 - h) / pf
        })
        .collect();
    Ok(Ols {
        n,
        p,
        coef: f.coef,
        se,
        se_hc3,
        t_hc3,
        p_hc3,
        ci_hc3,
        ci_level: level,
        sigma: sqrt(s2),
        r2,
        adj_r2,
        df_resid: df,
        fitted,
        resid,
        leverage,
        cooks,
    })
}

/// Variance inflation factor of every non-intercept column (column `j`
/// regressed on all other columns, which include the intercept).
pub fn vif(d: &Design) -> Result<Vec<f64>, RegressError> {
    let mut out = Vec::with_capacity(d.p.saturating_sub(1));
    for j in 1..d.p {
        let others: Vec<&[f64]> = (1..d.p).filter(|&k| k != j).map(|k| d.col(k)).collect();
        let dj = Design::with_intercept(&others);
        let fit = ols(&dj, d.col(j), 0.95)?;
        out.push(1.0 / (1.0 - fit.r2));
    }
    Ok(out)
}

/// Koenker's studentised Breusch–Pagan test: LM = n·R² of e² on X,
/// χ² on p − 1 df.
pub fn breusch_pagan(d: &Design, resid: &[f64]) -> Result<(f64, f64), RegressError> {
    let e2: Vec<f64> = resid.iter().map(|e| e * e).collect();
    let fit = ols(d, &e2, 0.95)?;
    #[allow(clippy::cast_precision_loss)]
    let lm = d.n as f64 * fit.r2;
    #[allow(clippy::cast_precision_loss)]
    Ok((lm, chi2_sf(lm, (d.p - 1) as f64)))
}

/// MAD about zero, scaled for consistency at the normal (statsmodels
/// `scale.mad(resid, center=0)`).
#[must_use]
pub fn mad0(resid: &[f64]) -> f64 {
    let mut a: Vec<f64> = resid.iter().map(|e| e.abs()).collect();
    a.sort_by(f64::total_cmp);
    crate::describe::median_sorted(&a) / MAD_CONSTANT
}

#[derive(Debug, Clone, PartialEq)]
pub struct Huber {
    pub coef: Vec<f64>,
    pub scale: f64,
    pub iterations: u32,
    pub converged: bool,
    pub weights: Vec<f64>,
}

/// Huber M-estimate by IRLS from the OLS start, MAD scale updated every
/// step, iterated to the fixed point (max |Δb| ≤ 1e-11·(1 + |b|)).
pub fn huber(d: &Design, y: &[f64]) -> Result<Huber, RegressError> {
    let n = d.n;
    let mut coef = wls(d, y, None)?.coef;
    let resid_of = |c: &[f64]| -> Vec<f64> { (0..n).map(|i| y[i] - d.row_dot(i, c)).collect() };
    let mut resid = resid_of(&coef);
    let mut scale = mad0(&resid);
    let mut weights = vec![1.0; n];
    let mut converged = false;
    let mut it = 0;
    while it < 500 {
        it += 1;
        if scale <= 0.0 {
            converged = true;
            break;
        }
        for i in 0..n {
            let z = (resid[i] / scale).abs();
            weights[i] = if z <= HUBER_T { 1.0 } else { HUBER_T / z };
        }
        let next = wls(d, y, Some(&weights))?.coef;
        let delta = next
            .iter()
            .zip(&coef)
            .map(|(a, b)| (a - b).abs() / (1.0 + b.abs()))
            .fold(0.0, f64::max);
        coef = next;
        resid = resid_of(&coef);
        scale = mad0(&resid);
        if delta <= 1e-11 {
            converged = true;
            break;
        }
    }
    Ok(Huber {
        coef,
        scale,
        iterations: it,
        converged,
        weights,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_line() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [3.0, 5.0, 7.0, 9.0, 11.1];
        let d = Design::with_intercept(&[&x]);
        let f = ols(&d, &y, 0.95).expect("ols");
        assert!((f.coef[1] - 2.02).abs() < 1e-12);
        assert!((f.coef[0] - 0.96).abs() < 1e-12);
        assert!(f.r2 > 0.999);
        let lev: f64 = f.leverage.iter().sum();
        assert!((lev - 2.0).abs() < 1e-12, "trace of the hat matrix = p");
    }

    #[test]
    fn singular_design_is_refused() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let x2 = [2.0, 4.0, 6.0, 8.0];
        let d = Design::with_intercept(&[&x, &x2]);
        assert!(matches!(
            ols(&d, &[1.0, 2.0, 3.0, 5.0], 0.95),
            Err(RegressError::Singular { .. })
        ));
    }

    #[test]
    fn huber_resists_an_outlier() {
        let x: Vec<f64> = (0..20).map(f64::from).collect();
        let mut y: Vec<f64> = x.iter().map(|v| 1.0 + 0.5 * v).collect();
        for (i, v) in y.iter_mut().enumerate() {
            *v += if i % 2 == 0 { 0.1 } else { -0.1 };
        }
        y[19] += 30.0;
        let d = Design::with_intercept(&[&x]);
        let o = ols(&d, &y, 0.95).expect("ols");
        let h = huber(&d, &y).expect("huber");
        assert!(h.converged);
        assert!((h.coef[1] - 0.5).abs() < (o.coef[1] - 0.5).abs());
    }
}
