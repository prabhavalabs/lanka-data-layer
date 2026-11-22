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
| `controls` | no | up to 8; enter the OLS/Huber fits only |

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
4. **OLS** `T(y) ~ T(x) [+ controls]` by Householder QR, classical and **HC3** standard errors (MacKinnon &
   White 1985), intervals b ± t₀.₉₇₅,ₙ₋ₚ · se_HC3, leverage, Cook's D, VIF, studentised Breusch–Pagan; **Huber**
   M-estimate (t = 1.345, MAD scale, IRLS to the fixed point). The robust fit becomes the headline when
   `OUTLIER_LEVERAGE` fires.
5. **Residual Moran's I** on k-nearest-neighbour weights (`knn4|6|8`, row-standardised; default `knn6`), pseudo-p
   from seeded permutations (`permutations`, default 999; seed = first 8 bytes of the fingerprint).
6. **Bivariate classes** (`classes` 3|4 by `quantile|equal|jenks`) on the transformed values; class code =
   `class_y · k + class_x`.
7. **Scale sensitivity**: the same pair at the adjacent levels where both are published (identity alignment),
   or at `scale_sensitivity: [levels]`.

## Outputs

`bivariate` (frame: x, y raw; class; basis), `residuals` (frame: fitted, residual, Cook's D, leverage),
`scatter` (chart: points up to 2,000 rows, else hexbins; with the headline fit line).

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
matrix formula; seeded permutations and bootstraps bit-exact against the ported counter RNG.
Golden test: built-up share vs population density, ADM3, island (`lanka-analysis/tests/golden/`):
r = 0.637 (0.569–0.696), n_effective 62.2, adjusted p 2.0e-8, ρ = 0.825, residual Moran's I 0.63.

## Limits

Areal associations only (point support with WP-27/WP-33); Lee's L, contiguity weights and spatial regression
are WP-30; `uncertainty: "mc"` is WP-30; the effective-n estimator is skipped above 20,000 units.
