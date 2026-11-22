# Method: `regress@1.0.0` (analysis, tier 1)

Multiple regression of `y` on `x` and `controls` on one admin support: OLS by Householder QR with HC3 intervals,
Huber robust fit, optional **fixed effects by parent unit** (`fixed_effects: "parent"`: one dummy per parent but
the first), diagnostics (VIF, Cook's D, leverage, studentised Breusch–Pagan) and residual Moran's I on kNN
weights with seeded permutations. Code: `lanka-analysis/src/methods/regress.rs` (+ `methods/fit.rs`).

- **Min n**: blocked below 10 + 10 per predictor beyond the first; `SMALL_N` (no p) below 30.
- **Reading**: templated by the transforms of x and y ("one percentage point more of … is associated with …
  % higher …, other terms held constant"); never causal wording.
- **Outputs**: `residuals` frame. **Estimates**: `ols {formula, coef[term, b, se, se_hc3, ci, p?, vif], r2,
  adj_r2, fixed_effects, reading, diagnostics}`, `robust {coef}`, `residual_moran`, `headline`.
- **Guards**: as `relate` minus the correlation-only ones (`MAUP_UNSTABLE`); `MULTICOLLINEAR` when any VIF > 10,
  `NO_CONTROLS` with a single predictor, `OUTLIER_LEVERAGE` when robust and OLS slopes differ by > 25 % or
  max Cook's D > 1 (the robust fit becomes the headline).
- **Validation**: statsmodels `OLS(cov_type="HC3")`, `OLSInfluence`, `variance_inflation_factor`,
  `het_breuschpagan(robust=True)`, `RLM(HuberT)` on Stack loss, Longley, Anscombe I and a synthetic set (1e-8).
- **Limits**: no spatial lag/error models (WP-30); fixed effects only by the parent level.
