# Method: `relate@1.0.0` (analysis, tier 1)

**What it answers.** Do two variables go together across the units of one support, how strongly, in which
direction, and how sure can we be — given that neighbouring areas are not independent? It never says that one
variable changes the other (`NOT_CAUSAL` is on every result).

Implementation: `engine/crates/lanka-analysis/src/methods/relate.rs` over the kernels of `lanka-stats`
(`correlate`, `regress`, `effn`, `spatial`, `classify`). Request/result contract: `spec/analysis/1/`.

## Inputs and support

| Role | Required | Notes |
|---|---|---|
| `x` | yes | indicator, grid layer (`grid_zonal`) or features dataset (`features_zonal`) |
| `y` | yes | as `x` |
| `controls` | no | up to 8; enter the OLS/Huber fits and the partial correlations |

Support: admin units at one level (`support: "auto"` = the coarsest native level among the indicators, 11 §3.2
rule 1). Transforms per variable: `identity | log | log1p | sqrt | zscore | rank`; values a transform cannot take
(log of ≤ 0) are dropped and counted in `n.dropped` (`transform_<role>`).

## Computation

1. **Complete cases** over x, y and controls after transforms.
2. **Pearson r** with *p* from *t* on n − 2 df and a 95 % interval by Fisher's z; **Spearman ρ** (average ranks)
   with *p* as above and the Fieller–Hartley–Pearson interval (variance 1.06/(n − 3)).
3. **Effective sample size** (Clifford, Richardson & Hémon 1989; Dutilleul 1993): `M = 1 + tr(Bx)·tr(By)/tr(BxBy)`,
   `B = PΣ̂P`, Σ̂ the autocovariance per distance class — 13 equal-width classes of great-circle distance between
   unit centroids. `p_adjusted` from F = (M − 1) r²/(1 − r²) on (1, M − 2) df. **On areal support the adjusted
   p is the headline** (`significance: "adjusted"`); the naive p is reported beside it.
4. **Partial correlation** (with controls only): x and y (ranks for ρ) are residualised by OLS on
   `[1, controls]`; Pearson of the residuals with *t* on n − 2 − k df and a Fisher-z interval with √(n − 3 − k)
   (`pingouin.partial_corr`). Its effective n is estimated on the two residual vectors and each control costs
   one more degree of freedom: F = (M − 1 − k) r²/(1 − r²) on (1, M − 2 − k). **With controls the partial r is the
   headline** (`estimates.partial`, `correlation_headline: "partial"`, summary "… holding <controls> constant
   (partial r = …)"); the zero-order `pearson`/`spearman` stay beside it. Without controls neither field is
   present.
5. **OLS** `T(y) ~ T(x) [+ controls]` by Householder QR, classical and **HC3** standard errors (MacKinnon &
   White 1985), intervals b ± t₀.₉₇₅,ₙ₋ₚ · se_HC3, leverage, Cook's D, VIF, studentised Breusch–Pagan; **Huber**
   M-estimate (t = 1.345, MAD scale, IRLS to the fixed point). The robust fit becomes the headline when
   `OUTLIER_LEVERAGE` fires.
6. **Residual Moran's I** on k-nearest-neighbour weights (`knn4|6|8`, row-standardised; default `knn6`), pseudo-p
   from seeded permutations (`permutations`, default 999; seed = first 8 bytes of the fingerprint).
7. **Bivariate classes** (`classes` 3|4 by `quantile|equal|jenks`) on the transformed values; class code =
   `class_y · k + class_x`.
8. **Scale sensitivity**: the same pair at the adjacent levels where both are published (identity alignment),
   or at `scale_sensitivity: [levels]`.

`estimates.partial` = `{controls: [labels], k, pearson: {r, ci, ci_level, n, df, p?, n_effective?, p_adjusted?},
spearman: {rho, …}}`.

## Outputs

`bivariate` (frame: x, y raw; class; basis), `residuals` (frame: fitted, residual, Cook's D, leverage),
`scatter` (chart: points up to 2,000 rows, else hexbins; with the headline fit line).

Every `suggested_layer` is a valid `ldl-map/1` layer (tested against the map schema and `spec/map/1/ramps.json`). The bivariate one binds `x` and `y` with `{type: <method>, classes, breaks}` — breaks moved back to the raw scale of the `x`/`y` fields (exact inverse for pointwise transforms; for `zscore`/`rank` the smallest raw value in the upper class), so the classes are the ones computed — and uses palette `ldl.biv.blue-orange` (3×3). Residual layers use a `diverging` scale (`classes: 7, mid: 0`).

## Guards it can raise

`NOT_CAUSAL`, `MIN_N` (n < 10: blocked, no estimates), `SMALL_N` (n < 30: intervals only, **no p-values in the
result at all**), `MAUP`, `MAUP_UNSTABLE` (sign flip or |Δr| > 0.2 across levels), `ECOLOGICAL`,
`RESIDUAL_AUTOCORR`, `SIZE_CONFOUND`, `SHARED_LINEAGE`, `ESTIMATED_INPUT`, `MODEL_ON_MODEL`,
`INPUT_UNCERTAINTY_IGNORED`, `PERIOD_MISMATCH` (> 2 years warn, > 5 block, overridable), `LOW_COVERAGE`
(< 80 % warn, < 30 % block), `PSEUDO_REPLICATION`, `OUTLIER_LEVERAGE`, `NO_CONTROLS`, `MULTICOLLINEAR`,
`SUPPRESSED_CELLS`, `SYNTHETIC_INPUT`, `COMMUNITY_INPUT` (registry: `analysis-guards.md`).

## Validation

Reference fixtures (`lanka-stats/tests/fixtures/reference.json`, generator `gen_reference.py`; numpy 2.5.5,
scipy 1.18.1, statsmodels 0.15.0 — R is not available on the build machine) agree to 1e-8: Anscombe's quartet,
Stack loss, Longley (1e-5, condition number ~5e9), a 400-row t₃ sample; effective n against the explicit n×n
matrix formula; partial Pearson/Spearman (k = 1, 2) and the k-adjusted F test against numpy/scipy
(`fixtures/partial.json`, generator `gen_partial.py`); seeded permutations and bootstraps bit-exact against the ported counter RNG.
Golden test: built-up share vs population density, ADM3, island (`lanka-analysis/tests/golden/`):
r = 0.637 (0.569–0.696), n_effective 62.2, adjusted p 2.0e-8, ρ = 0.825, residual Moran's I 0.63.

## Limits

Areal associations only (point support with WP-27/WP-33); Lee's L, contiguity weights and spatial regression
are WP-30; `uncertainty: "mc"` is WP-30; the effective-n estimator is skipped above 20,000 units.
