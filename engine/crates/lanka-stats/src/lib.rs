//! Lanka Data Layer statistics (11 §7): pure kernels with no I/O, no
//! async and no Postgres, compiled natively into the engine and to
//! `wasm32-unknown-unknown` for the browser (`lanka-stats-wasm`).
//!
//! Determinism is a contract: the same inputs give the same bits on every
//! target (plain IEEE arithmetic in a fixed order, transcendental functions
//! from the pure-Rust `libm`, resampling from the counter-based [`rng`]).
//!
//! - [`classify`]: quantile, equal, jenks, stddev, headtail, pretty; bivariate.
//! - [`describe`]: summaries, quantiles, ranks, histogram, hexbin.
//! - [`correlate`]: Pearson / Spearman with intervals; adjusted p.
//! - [`regress`]: OLS (HC3, leverage, Cook's D, VIF, Breusch–Pagan), Huber.
//! - [`compare`]: Welch, Mann–Whitney, Hodges–Lehmann, Cohen's d, bootstrap.
//! - [`effn`]: effective sample size under spatial autocorrelation.
//! - [`spatial`]: k-nearest-neighbour weights, Moran's I with seeded permutations.
//! - [`rng`], [`special`], [`transform`].

pub mod classify;
pub mod compare;
pub mod correlate;
pub mod describe;
pub mod effn;
pub mod regress;
pub mod rng;
pub mod spatial;
pub mod special;
pub mod transform;

/// Enters every analysis fingerprint (11 §9): bump when any kernel's
/// output changes for the same input.
pub const VERSION: &str = "1.0.0";

/// The browser's live preview (UI contract §7 `previewRelate`): Pearson,
/// Spearman and the OLS line on transformed complete pairs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preview {
    pub n: usize,
    pub r: f64,
    pub rho: f64,
    pub intercept: f64,
    pub slope: f64,
}

#[must_use]
pub fn preview_relate(
    x: &[f64],
    y: &[f64],
    tx: transform::Transform,
    ty: transform::Transform,
) -> Preview {
    let (a, b) = correlate::complete(&tx.apply(x), &ty.apply(y));
    let r = correlate::pearson_r(&a, &b);
    let rho = correlate::pearson_r(&describe::ranks(&a), &describe::ranks(&b));
    let (intercept, slope) = if a.len() > 2 {
        let d = regress::Design::with_intercept(&[&a]);
        match regress::ols(&d, &b, 0.95) {
            Ok(f) => (f.coef[0], f.coef[1]),
            Err(_) => (f64::NAN, f64::NAN),
        }
    } else {
        (f64::NAN, f64::NAN)
    };
    Preview {
        n: a.len(),
        r,
        rho,
        intercept,
        slope,
    }
}
