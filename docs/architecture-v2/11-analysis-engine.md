# 11 — Analysis engine

The question this serves: *"Flood-prone areas should have lower property prices — is that true here, where, and how sure are we?"* Generalised: take two or more layers of any kind (official statistics by district, a hazard raster, point samples such as price listings or rain gauges, a user's own file), bring them onto a common spatial support, quantify the relationship, show it on the map, and say honestly what the result can and cannot support.

It is a crate (`engine/crates/lanka-analysis`), a pure-maths crate under it (`lanka-stats`), one schema (`analysis`), REST and MCP endpoints, and UI patterns. Markers: **[A]** = assumption to verify in the named work package.

## 1. Position in the architecture

| | Enrichment ([04](04-enrichment.md)) | Analysis (this document) |
|---|---|---|
| Who starts it | maintainers, through a recipe in the registry | any user, through a request |
| When it runs | inside the release train | on demand |
| Output | a published, reviewed, versioned **dataset** | a fingerprinted, cached **result** that can be drawn as layers |
| Review | QA gates, `stable` promotion, method page | none — so guard rails are part of the result itself (§6) |
| Shared | the alignment methods (`rollup`, `grid_zonal`, `features_zonal`, `crosswalk_rebase`, `dasymetric`, `spatial_interp`, `exposure_overlay`, `composite_index`), the fingerprint rule, the uncertainty rules of [04 §5](04-enrichment.md#5-uncertainty-and-status-propagation), the labelling rules of [04 §8](04-enrichment.md#8-labelling) |

Analysis is the sandbox; recipes are the published products. `lanka analysis export-recipe <id>` writes a recipe skeleton from a result, which is how an analysis that proved useful becomes a curated `derived-*` dataset through the normal review path.

Analysis never writes to `data.*`. It reads through `lanka-query` backends and writes only to `analysis.*` and the blob store namespace `analysis/`.

## 2. Pipeline

```
 request (ldl-analysis/1)
    │  resolve: ids → pinned versions (release or dsv), access check, licence walk
    ▼
 PLAN      choose target support · choose an alignment method per variable · estimate cost → sync | async
    ▼
 ALIGN     every variable → one column on the target support, with lo/hi, basis, quality   → AlignedFrame
    ▼
 COMPUTE   method kernel(s) over the AlignedFrame (+ spatial weights)                       → estimates, diagnostics
    ▼
 GUARD     evaluate every guard against request, frame and estimates                        → block | warn | note
    ▼
 PRESENT   outputs (frames, grids, features, chart data) + suggested map layers + templated interpretation + citation
    ▼
 result (ldl-analysis-result/1), id = an_<fingerprint>, immutable, cached
```

## 3. Alignment

### 3.1 Supports

| Support | Is | Typical n |
|---|---|---|
| `admin` level 0–4 (or another scheme: `moh`, `la`, `ed`) | coded units, joined by code | 9 / 25 / 339 / 14,043 |
| `grid` at factor *k* ∈ {1, 5, 10, 50} | blocks of *k×k* canonical cells (0.001° … 0.05°). Always a multiple of the canonical grid; there is no second grid system. | up to 11.3 M (k=1) |
| `points` | the features of one point dataset; other variables are sampled at each point | up to 250,000 |
| `zones` (later, WP-37) | a user's polygon dataset used as units | — |

### 3.2 Planner rules (binding)

1. **Aggregate up, never down, by default.** `support: "auto"` is the coarsest native support among the areal inputs; grids and points aggregate to it. Moving a value to a finer support (district figure → GN divisions) happens only when the request names a disaggregation method, and the column is then `estimated` with a quality class.
2. **Prefer point support when one variable is a point sample.** If listings carry coordinates, the default is to sample the other layers *at the listings* and analyse properties, not areas. This is the only support that avoids the ecological fallacy for a question about properties.
3. **Extensive and intensive values are treated differently**, from the indicator's `measure_type` ([02 §3](02-standard-model.md#3-measures-dimensions-attributes)): `count`/`sum` are extensive (they add up and are split by weights); `rate`, `ratio`, `share`, `mean`, `median`, `index`, `score` are intensive (they are recomputed or weighted, never summed); `rank`/`category` are not aligned across supports at all.
4. **Every polygon overlay is done on the canonical grid.** Hazard zones, buffers, protected areas and isochrones are rasterised to 0.001° cells and then treated as grids. One overlay engine, exact reproducibility, no pairwise polygon intersection of 14,043 units with arbitrary shapes. The error is bounded by the cell size (≈ 111 m) and stated in the result.
5. **Metric operations use metric distances.** Buffers and distance-to are a Euclidean distance transform on the grid with per-row cell width (the grid is geographic, [02 §2.3](02-standard-model.md#23-geometry-and-crs-policy)).

### 3.3 Alignment matrix

| From → to | Method (crate) | Result basis | Uncertainty | Quality / flags |
|---|---|---|---|---|
| admin level *a* → same level | identity | unchanged | unchanged | — |
| admin finer → coarser | `rollup` by the indicator's `aggregation` | `computed` | variances add / delta method | `PARTIAL_COVERAGE` |
| admin coarser → finer, extensive | `dasymetric` (explicit only) | `estimated` | quality class A–D | `DISAGGREGATED`, `ANCILLARY_WEAK` |
| admin coarser → finer, intensive | `inherit` (explicit only): child takes the parent value | `estimated` | none; n effective = number of parents | `DISAGGREGATED`; guard `PSEUDO_REPLICATION` |
| other scheme → admin (MOH → DSD) | `crosswalk_rebase`, population weights | `estimated` where a weight < 1 | spread across weight kinds | `REBASED` |
| old boundary vintage → reference | `crosswalk_rebase` | as above | as above | `REBASED`, `SPLIT_UNIT` |
| grid → admin | `grid_zonal`: `sum` (extensive), `pop_weighted_mean` or `mean` (intensive), `share_where(expr)` (hazard: share of area or population where depth ≥ t), `max`, `pctl` | `computed` for observed grids, `estimated` for modelled | CV across models where several exist | `SMALL_N` below `min_cells` (10) |
| grid → grid factor *k* | block reduce with the layer's `overview` reducer | unchanged | — | — |
| grid → points | `sample`: cell value at each point; optional `window_m` mean | unchanged | — | point precision class recorded |
| points (events, facilities) → admin / grid | `features_zonal`: `count`, `sum(field)`, `density` | `computed` | none | `UNOFFICIAL_GEOM` passthrough |
| **points (samples of an intensive quantity) → admin** | `point_aggregate`: `median` (default), `trimmed_mean`, `mean`, with `n`, MAD and a bootstrap interval per unit | `estimated` | 90 % bootstrap interval | `SMALL_N` when n < 10; `status = suppressed` when n < `min_n` (default 5) |
| points (samples) → grid → admin | `spatial_interp` (IDW or ordinary/universal kriging), then `grid_zonal` | `estimated` | kriging variance, or leave-one-out RMSE for IDW | `INTERPOLATED_SPACE`; masked beyond `max_dist` |
| polygons / lines → grid | `rasterise` (coverage fraction per cell), `distance_to` | `computed` | — | cell-size note |
| time mismatch | `match_period: exact \| nearest≤k \| latest_common \| interp` (the `ratio` method's rule) | `interp` → `estimated` | — | guard `PERIOD_MISMATCH` |

### 3.4 Interpolation policy for point samples (amends [04 §3](04-enrichment.md#3-method-library-v1-of-the-library))

The binding rule "kriging and IDW only for continuous physical fields; never for counts or administrative statistics" stays for **published recipes**. Analysis adds one carefully fenced case, because the user's question needs it:

| Variable | Allowed alignment |
|---|---|
| Continuous physical field sampled at stations (rainfall, temperature, groundwater, air quality) | `spatial_interp` (kriging with elevation drift, or IDW); unchanged |
| **Socio-economic intensive quantity sampled at points** (asking price per perch, rent per m², wage quotes, food prices at markets) | default `point_aggregate` per unit. `spatial_interp` is allowed **only in analysis, only on request**, on a transformed scale (log by default), and only with all of: a mask to built-up cells and to `max_dist` from the nearest sample (default 2 km), a minimum of 30 samples, the leave-one-out error reported in the result, flag `INTERPOLATED_SPACE`, and the guard `INTERP_SOCIO` ("a price surface is a smoothed description of asking prices, not a valuation of any parcel"). |
| Counts, sums, shares of administrative totals | never interpolated; `dasymetric` or `crosswalk_rebase` |

An interpolated socio-economic surface cannot be promoted to a recipe without a method page and a validation score, like any other estimate.

### 3.5 The aligned frame

Every variable arrives as one column with its history attached; the compute stage never sees raw inputs.

```rust
// engine/crates/lanka-analysis/src/frame.rs
pub enum Support {
    Units  { scheme: SchemeId, level: u8, boundary_set: BoundarySetId, keys: Arc<[i32]> },
    Grid   { factor: u16, window: CellWindow },
    Points { dataset: DatasetRef, keys: Arc<[FeatureKey]>, lon: Arc<[f32]>, lat: Arc<[f32]> },
}

pub struct Column {
    pub role: Role,                       // X | Y | Control(i) | Weight | Group | Zone
    pub source: ResolvedInput,            // dataset, dsv, indicator, dims, period, licence tier, class
    pub transform: Transform,             // Identity | Log | Log1p | Sqrt | ZScore | Rank
    pub values: Vec<f64>,                 // NaN = missing
    pub lo: Option<Vec<f64>>, pub hi: Option<Vec<f64>>,
    pub basis: Vec<Basis>, pub status: Vec<Status>, pub flags: Vec<Flags>,
    pub n_support: Option<Vec<u32>>,      // cells or points behind each value
    pub alignment: AlignmentRecord,       // from-support, method@version, params, coverage, quality class, dropped counts
}

pub struct AlignedFrame { pub support: Support, pub columns: Vec<Column>, pub mask: BitVec /* complete cases */ }
```

## 4. Method catalogue

Each method is one `AnalysisMethod` (§7). Tier 1 ships first (WP-29), tier 2 next (WP-30), tier 3 when justified (WP-37).

| Tier | Method id | Does | Supports | Min n (block / warn) | Key outputs |
|---|---|---|---|---|---|
| 1 | `describe` | Univariate summary, histogram, missingness, Moran's I when weights exist | all | 1 / 30 | stats, histogram bins |
| 1 | `relate` | The headline method for two variables: Pearson *r* and Spearman *ρ* with intervals (partial *r*/*ρ* given the controls, as the headline, when controls are named); OLS and robust (Huber) fits on chosen transforms, HC3 standard errors; bivariate classes; scale-sensitivity table; with weights: Moran's I of residuals, spatially adjusted significance, global and local Lee's *L* | units, grid (k ≥ 5), points | 10 / 30 | `scatter` (points or hexbin counts), `bivariate` frame, `residuals` frame, `local_lee` frame, coefficients |
| 1 | `regress` | Multiple OLS / robust regression with controls and optional fixed effects by parent unit; VIF, Cook's distance, Breusch–Pagan, residual Moran's I | units, points | 10 + 10 per predictor / 30 | coefficients with intervals, `residuals` |
| 1 | `group_compare` | Difference between groups: inside vs outside a zone, with distance rings ("inside", "0–250 m", "250–500 m"), optionally within parent units (fixed effects). Welch *t*, Mann–Whitney, Hodges–Lehmann shift, bootstrap interval, Cohen's *d*; on areal support the spatially adjusted *p* and interval (effective n of the inside indicator and *y*) | units, points, grid | 10 per group / 30 | group table, effect size, `points`/`units` with group label |
| 1 | `exposure` | Hazard mask × exposed quantity for any area: population, buildings, facilities inside the mask, by threshold or return period (wraps `exposure_overlay`) | grid → units | — | per-unit counts and shares with intervals |
| 1 | `zonal` | Any grid or feature layer summarised to any units (the alignment step exposed as a result) | → units | — | frame |
| 1 | `classify` / `bivariate` | Class breaks for one or two columns (display support) | all | 2 / — | breaks, class column |
| 2 | `autocorr` | Global Moran's I, Geary's C, correlogram by contiguity order | units, grid (k ≥ 10) | 30 / 100 | I, *z*, permutation *p* |
| 2 | `local_clusters` | Local Moran (LISA) and bivariate local Moran / local Lee's *L*, conditional permutation *p*, Benjamini–Hochberg FDR | units | 30 / 100 | `clusters` frame: HH/HL/LH/LL, *p*, *p_fdr* |
| 2 | `hotspots` | Getis–Ord Gi* with FDR | units, grid | 30 / 100 | `hotspots` frame: *z*, confidence band |
| 2 | `spatial_regress` | Lagrange-multiplier tests on OLS residuals (LM-lag, LM-error, robust forms) choose between spatial lag (spatial 2SLS, Kelejian–Prucha) and spatial error (GMM); reports direct/indirect impacts for the lag model | units | 50 / 100 | coefficients, impacts, test table |
| 2 | `change` | Change between two periods (absolute, relative, standardised by the series' own variability); trend over many periods (Theil–Sen slope, Mann–Kendall); before/after an event date | units, grid | 2 periods; trend ≥ 8 / 12 | `change` frame, significance |
| 2 | `composite` | Ad hoc composite index (wraps `composite_index`: normalise, weight, aggregate, Monte Carlo rank intervals) | units | 20 / 50 | index + sub-indices + rank interval |
| 2 | `interpolate` | IDW / kriging surface from point samples under §3.4 | points → grid | 30 / 100 | grid + variance grid + cross-validation |
| 2 | `isochrone` | Travel-time area from a point: friction-surface cost-distance on the grid (seconds; labelled approximate) | grid | — | polygon per threshold, population inside |
| 3 | `gwr` | Geographically weighted regression, adaptive bisquare kernel, AICc bandwidth | units ≤ 5,000, points ≤ 5,000 | 100 / 300 | local coefficients, local R², condition numbers |
| 3 | `isochrone_network` | Valhalla isochrones (needs the routing service up; on by default only in the on-prem profile) | features | — | polygons |

Decisions inside the catalogue:

- **Lee's *L* is the bivariate spatial statistic**, not bivariate Moran's I: *L* integrates the point-to-point association (Pearson) with spatial autocorrelation of both variables and is symmetric; bivariate Moran's I measures association between *x* here and *y* next door, which answers a different question and is routinely misread. Bivariate local Moran is available in `local_clusters` for users who ask for it by name.
- **Spatial 2SLS / GMM rather than maximum likelihood** for spatial regression: no log-determinant of a 14,043 × 14,043 matrix, robust to non-normal errors, sparse products only.
- **GWR is tier 3 and capped.** It is exploratory, prone to local multicollinearity, and expensive. It runs only asynchronously, only when the global model shows significant non-stationarity (a Koenker–Bassett test on residuals by region is reported first), and its result is flagged `EXPLORATORY`.
- **The headline significance for areal data is the spatially adjusted one** (effective sample size after Clifford–Richardson–Hémon / Dutilleul), because neighbouring units are not independent. The naive *p* is reported beside it, never instead of it. With controls, the effective n is estimated on the residuals of *x* and *y* on the controls and each control costs one more degree of freedom; `group_compare` tests the point-biserial correlation of *y* with the inside indicator the same way.
- **Permutation tests are seeded** by the first 8 bytes of the fingerprint ([04 §2](04-enrichment.md#2-method-contract)), so a result is bit-reproducible.

Spatial weights: queen contiguity per boundary set and level, *k*-nearest (k = 4, 6, 8) and distance band, row-standardised. Contiguity is precomputed by a job when a boundary set changes and stored as a CSR blob (`analysis.spatial_weights`); islands (units with no neighbours, e.g. Delft) get their nearest neighbour and are listed in the result.

## 5. Requests and results

### 5.1 Request — `ldl-analysis/1`

```jsonc
{
  "spec": "ldl-analysis/1",
  "method": "relate",
  "pin": "latest",                                   // or {"release": "20270315.1"}; inherited from the map when embedded
  "support": { "kind": "admin", "level": 4, "within": "LK1" },      // or "auto"
  "variables": {
    "x": { "indicator": "exp.flood_pop_share", "dims": { "return_period": "RP100" }, "period": "latest" },
    "y": { "indicator": "@demo/re.asking_price_per_perch_median", "period": "2026", "transform": "log",
           "align": { "method": "point_aggregate", "stat": "median", "min_n": 5 } },
    "controls": [ { "indicator": "est.pop_density", "transform": "log" },
                  { "indicator": "acc.travel_time_city", "dims": { "city": "colombo", "mode": "car" } } ]
  },
  "params": {
    "correlation": ["pearson", "spearman"], "regression": "ols", "robust": true,
    "spatial": { "weights": "queen", "local": "lee", "permutations": 999 },
    "bivariate": { "classes": 3, "method": "quantile" },
    "scale_sensitivity": [3, 4],
    "uncertainty": "analytic"                        // or "mc" (200 draws from input intervals)
  }
}
```

A variable is an `indicator`, a `grid`, a `features` field, or (inside a map document) a `source` key. `align` is optional; the planner fills it by §3.2 and echoes its choice in the result.

### 5.2 Result — `ldl-analysis-result/1`

```jsonc
{
  "spec": "ldl-analysis-result/1",
  "id": "an_5e1f0c22ab904d71",
  "state": "ready",                                  // queued | running | ready | failed | blocked
  "method": { "id": "relate", "version": "1.0.0",
              "citations": ["Lee 2001, J. Geogr. Syst. 3", "Clifford, Richardson & Hémon 1989, Biometrics 45"] },
  "request": { … the request with every "latest", "auto" and default made explicit … },
  "inputs": [
    { "role": "x", "dataset": "derived-flood-exposure", "dsv": "dsv_41c0a9e77b1d2f08", "indicator": "exp.flood_pop_share",
      "period": "2026", "class": "estimated", "licence": "cc-by-4.0", "visibility": "public" },
    { "role": "y", "dataset": "@demo/property-asking-prices-sample", "dsv": "dsv_3a91f0e2c7d45b10",
      "field": "price_per_perch", "class": "user", "licence": "cc0-1.0", "visibility": "public", "synthetic": true }
  ],
  "alignment": [
    { "role": "x", "from": "admin:4", "to": "admin:4", "method": "identity", "coverage": 1.0 },
    { "role": "y", "from": "points", "to": "admin:4", "method": "point_aggregate@1.0.0",
      "params": { "stat": "median", "min_n": 5 }, "coverage": 0.61, "quality": "B",
      "dropped": { "units_below_min_n": 977, "points_outside_extent": 12 } }
  ],
  "n": { "units": 2505, "used": 1528, "dropped": { "missing_y": 977 } },
  "estimates": {
    "pearson":  { "r": -0.31, "ci": [-0.36, -0.26], "ci_level": 0.95, "p": 2.1e-35, "p_adjusted": 0.004, "n_effective": 84 },
    "spearman": { "rho": -0.28, "ci": [-0.33, -0.23], "p_adjusted": 0.009 },
    "ols":      { "formula": "log(y) ~ x + log(pop_density) + travel_time_city",
                  "coef": [ { "term": "x", "b": -0.0091, "se_hc3": 0.0017, "ci": [-0.0124, -0.0058] }, … ],
                  "r2": 0.57, "adj_r2": 0.57,
                  "reading": "one percentage point more of the population in the flood zone is associated with 0.9 % lower median asking price, other terms held constant" },
    "robust":   { "coef": [ { "term": "x", "b": -0.0084 }, … ] },
    "lee_l":    { "L": -0.22, "p_perm": 0.001 },
    "residual_moran": { "I": 0.34, "p_perm": 0.001 },
    "scale_sensitivity": [ { "level": 3, "n": 40, "pearson_r": -0.44 }, { "level": 4, "n": 1528, "pearson_r": -0.31 } ]
  },
  "guards": [
    { "code": "NOT_CAUSAL",        "severity": "note", "message": "This is an association between areas. It does not show that flood risk lowers prices." },
    { "code": "ECOLOGICAL",        "severity": "warn", "message": "Results describe GN divisions, not individual properties. A point-level comparison is available for this data.", "action": { "method": "group_compare", "support": "points" } },
    { "code": "RESIDUAL_AUTOCORR", "severity": "warn", "message": "Residuals are spatially clustered (Moran's I 0.34). Standard errors of the plain regression are too small; use the adjusted figures or a spatial model.", "action": { "method": "spatial_regress" } },
    { "code": "LOW_COVERAGE",      "severity": "warn", "message": "39 % of GN divisions have fewer than 5 listings and are excluded; they are mostly rural.", "detail": { "coverage": 0.61 } },
    { "code": "MAUP",              "severity": "note", "message": "The correlation is −0.44 at DS-division level and −0.31 at GN level. Direction is stable; strength depends on the units." },
    { "code": "ESTIMATED_INPUT",   "severity": "note", "message": "Flood exposure is a model estimate (JRC river-flood hazard × WorldPop)." },
    { "code": "SYNTHETIC_INPUT",   "severity": "warn", "message": "The price data is a synthetic sample for demonstration." }
  ],
  "interpretation": {
    "summary": "Across 1,528 GN divisions of Western Province with at least 5 listings, a higher share of population in the 1-in-100-year flood zone is associated with lower median asking prices (r = −0.31, adjusted p = 0.004).",
    "caveats": ["…one line per warn-level guard…"]
  },
  "outputs": [
    { "id": "bivariate", "kind": "frame", "href": "…/outputs/bivariate", "columns": ["x", "y", "class", "n_listings", "basis"],
      "suggested_layer": { "type": "bivariate", "style": { … } } },
    { "id": "local_lee", "kind": "frame", "href": "…/outputs/local_lee", "columns": ["L", "p", "p_fdr", "cluster"],
      "suggested_layer": { "type": "polygon", "style": { … } } },
    { "id": "residuals", "kind": "frame", "href": "…/outputs/residuals",
      "suggested_layer": { "type": "choropleth", "style": { "fill": { "field": "residual", "scale": { "type": "diverging", "mid": 0 } } } } },
    { "id": "scatter",   "kind": "chart", "href": "…/outputs/scatter", "encoding": "hexbin" }
  ],
  "basis": "estimated",
  "licence": { "id": "cc-by-4.0", "redistribution": "bulk" },
  "attribution": [ … ],
  "provenance": { "fingerprint": "5e1f0c22ab904d71…", "seed": 6782119904115, "engine": "lanka 2.3.0",
                  "git_rev": "3b1c9e0", "data_version": "20270315.1", "computed_at": "2027-03-16T04:11:52Z", "elapsed_ms": 840 },
  "citation": { "text": "Lanka Data Layer (2027). Analysis an_5e1f0c22ab904d71: relate(exp.flood_pop_share, @demo/re.asking_price_per_perch_median), Western Province, GN level. Data release 20270315.1.",
                "url": "https://lanka-data-layer.prabhavalabs.com/id/analysis/an_5e1f0c22ab904d71" }
}
```

Rules:

- **Reproducible**: the result contains the fully resolved request; `POST`ing it again to any conforming deployment holding the same dataset versions returns the same `id` and the same numbers.
- **Citable**: `/id/analysis/{id}` is a persistent URI ([02 §1](02-standard-model.md#1-identifiers)); results referenced by a saved map are never evicted.
- `basis` of a result is the maximum over input columns and alignment steps ([04 §5](04-enrichment.md#5-uncertainty-and-status-propagation)); `licence` is the computed licence of the inputs ([02 §8](02-standard-model.md#8-licences-attribution-redistribution)). A result with any non-public input is readable only by those who can read that input.
- `interpretation` is assembled from templates with a fixed vocabulary. No free-text generation, no language model.

## 6. Guard rails

Guards are data: a registry (`lanka-analysis/src/guards.rs`) of `{code, severity, trigger, message template, action}`. A result always carries its guards; clients MUST show `warn` and `block` guards next to any number, chart or layer from the result (the embed route and MCP text do this from the result, so they cannot be dropped).

| Code | Trigger | Severity |
|---|---|---|
| `NOT_CAUSAL` | every relationship method | note, always first |
| `MIN_N` | n used below the method's block threshold | **block** (no estimates returned; the result says what n was) |
| `SMALL_N` | n between block and warn thresholds | warn; intervals shown, *p* hidden |
| `MAUP` | any areal support | note; plus the scale-sensitivity table when adjacent levels are available at no extra alignment cost. Sign flip or \|Δr\| > 0.2 across levels → `MAUP_UNSTABLE` warn |
| `ECOLOGICAL` | areal support and a variable that describes people, households or properties | warn; with an action when point support is possible |
| `RESIDUAL_AUTOCORR` | permutation *p* of residual Moran's I < 0.05 | warn; adjusted significance becomes the headline |
| `SIZE_CONFOUND` | both variables extensive (two counts both scale with population) | warn; action: normalise by population or area |
| `SHARED_LINEAGE` | the two variables share an upstream input in the derivation graph ([04 §7](04-enrichment.md#7-dag-execution-and-incremental-recompute)), e.g. flood-exposed population and population density both built on WorldPop | warn: "part of this correlation is built in" |
| `ESTIMATED_INPUT` | any column with `basis ∈ {estimated, forecast}` | note; result `basis = estimated` |
| `MODEL_ON_MODEL` | both sides estimated | warn |
| `INPUT_UNCERTAINTY_IGNORED` | inputs have intervals and `uncertainty = analytic` | note; action: rerun with `mc` |
| `PERIOD_MISMATCH` | aligned periods differ by more than the request's tolerance (default 2 years) | warn; > 5 years between time-varying inputs block unless overridden (the block's `action` carries `{"override": ["PERIOD_MISMATCH"]}`). Inputs whose indicator the registry declares `time_invariant` (terrain, static hazard maps, climate normals) count towards the reported gap but never block: elevation dated 2015 against 2024 data is a warn |
| `LOW_COVERAGE` | complete cases < 80 % of the support | warn; < 30 % block. The result lists what was dropped and, for units, how the dropped set differs (urban share, population) |
| `PSEUDO_REPLICATION` | a column aligned by `inherit` | warn; effective n is the number of parent units |
| `DISAGGREGATED_INPUT` | a column aligned by `dasymetric` | warn; quality class shown |
| `INTERP_SOCIO` | §3.4 | warn |
| `OUTLIER_LEVERAGE` | robust and OLS slopes differ by > 25 %, or max Cook's D > 1 | warn; robust fit becomes the headline |
| `NO_CONTROLS` | a regression with one predictor | note: lists available candidate controls |
| `MULTICOLLINEAR` | any VIF > 10 | warn |
| `MULTIPLE_TESTING` | local statistics | note; FDR-adjusted *p* is the only one used for map classes |
| `SUPPRESSED_CELLS` | units dropped because inputs were suppressed | note |
| `SYNTHETIC_INPUT` · `COMMUNITY_INPUT` | an input is flagged synthetic / is unreviewed workspace data | warn |
| `EXPLORATORY` | tier 3 methods | warn |

Structural rules beyond the registry:

- A `block` guard yields a result with `state: "blocked"`, no estimates and no outputs. There is an `override: ["PERIOD_MISMATCH"]` list in the request for the guards documented as overridable; an override is recorded in the result and repeated in the citation.
- Hidden *p*-values under `SMALL_N` are not returned at all, so a client cannot show them.
- Message templates never contain "causes", "leads to", "due to", "because of", "effect of" or "impact of" for association methods; a unit test scans the registry.
- Results on point data from a private workspace that are shared publicly must have at least 5 points behind every published cell ([12 §5](12-enterprise-and-byod.md#5-bring-your-own-data)).

## 7. Rust sketch

```rust
// engine/crates/lanka-analysis/src/method.rs
pub struct AnalysisInfo {
    pub id: &'static str,                        // "relate"
    pub version: semver::Version,
    pub tier: u8,
    pub citations: &'static [&'static str],
    pub roles: &'static [RoleSpec],              // which variables are required, their kinds
    pub supports: &'static [SupportKind],
    pub params: schemars::Schema,
    pub min_n: MinN,                             // { block, warn }
    pub needs_weights: bool,
    pub guards: &'static [GuardCode],            // guards this method can raise in addition to the global ones
}

pub trait AnalysisMethod: Send + Sync {
    fn info(&self) -> &AnalysisInfo;
    /// Cost in abstract units from n, columns, permutations, grid cells. Drives sync/async and quota charging.
    fn cost(&self, shape: &FrameShape, params: &serde_json::Value) -> Cost;
    /// Pure and deterministic: frame + weights + params + seed → output. No database, no clock, no network.
    fn run(&self, frame: &AlignedFrame, weights: Option<&SpatialWeights>, params: &serde_json::Value, seed: u64)
        -> Result<MethodOutput>;
}

pub struct MethodOutput {
    pub estimates: serde_json::Value,            // validated against the method's result schema
    pub outputs: Vec<Output>,                    // Frame | Grid | Features | Chart
    pub raised: Vec<Guard>,
}

// engine/crates/lanka-analysis/src/engine.rs
pub struct Engine { backend: Arc<dyn Backend>, grids: GridStore, enrich: MethodRegistry, methods: AnalysisRegistry,
                    store: ResultStore, weights: WeightsCache }

impl Engine {
    pub async fn resolve(&self, req: &Request, who: &Principal) -> Result<Resolved>;      // pins versions, checks access
    pub fn fingerprint(&self, r: &Resolved) -> Fingerprint;                                // §9
    pub async fn plan(&self, r: &Resolved) -> Result<Plan>;                                // support, alignments, cost
    pub async fn align(&self, plan: &Plan) -> Result<AlignedFrame>;                        // SQL + lanka-grid + lanka-enrich methods
    pub async fn run(&self, r: Resolved, who: &Principal) -> Result<RunOutcome>;           // Ready(result) | Queued(job)
}

// engine/crates/lanka-stats — no I/O, no async, no Postgres; compiles to native and to wasm32
pub mod classify;    // quantile, equal, jenks, stddev, headtail, pretty
pub mod describe;    pub mod correlate;   pub mod regress;    // OLS, Huber, HC3, diagnostics (faer)
pub mod compare;     // Welch, Mann–Whitney, Hodges–Lehmann, bootstrap
pub mod spatial;     // weights (CSR), Moran, Geary, Lee, local statistics, Gi*, effective n, permutation engine
pub mod spreg;       // LM tests, spatial 2SLS, GMM error
pub mod trend;       pub mod interp;      // IDW, variogram fit, kriging
pub mod fdr;         pub mod rng;         // counter-based RNG keyed by (seed, stream)
```

Plug-in point: a new method is one type implementing `AnalysisMethod`, registered in `AnalysisRegistry`, with a result JSON Schema, a method page, unit tests on known answers and a reference test against R (`spdep`, `spatialreg`, `sandwich`, `GWmodel`) fixtures — the testing rule of [04 §9](04-enrichment.md#9-testing-methods). Out-of-tree methods are added by a distribution crate ([12 §8](12-enterprise-and-byod.md#8-plugin-points)).

## 8. Where the computation runs

| Work | Runs in | Why |
|---|---|---|
| Frame assembly, roll-ups, counts of features per unit, crosswalk joins | **SQL** (Postgres, read-only role, integer-key joins on the `_map` index and `serve.*`) | the data is there; ≤ 14,043 rows × a few columns |
| Building contiguity weights (`ST_Touches` / `ST_Relate` on `geo.unit_geoms`) | **SQL/PostGIS**, batch job per boundary set | once per boundary vintage |
| Display buffers and isochrone polygons (vectorising a grid mask) | Rust (`lanka-grid` contour tracing); PostGIS only for simplification | keep the API role's 5 s timeout irrelevant |
| Zonal statistics, sampling grids at points, rasterising polygons, distance transform, overlay, cost-distance isochrones, IDW/kriging, server-side density grids | **Rust** `lanka-grid` kernels over mmap arrays, `rayon` | 11.3 M cells; tens of ms per pass ([05 §8](05-storage-and-performance.md#8-grid-store)) |
| All statistics, permutation tests, regressions | **Rust** `lanka-stats` | one implementation; small dense or sparse problems |
| Network isochrones | Valhalla | tier 3 only |
| Classification for styling; live preview of *r*, the fit line and group means while the user changes transforms or brushes the scatter; hexbin of the scatter | **Browser**, the same `lanka-stats` compiled to WebAssembly, in a worker, on the frame already loaded | no round trip; and still one implementation of every number |
| Drawing: bivariate colours, hexbin, heatmap, client bins | Browser (MapLibre / deck.gl) | [10 §4](10-map-composition.md#4-client-rendering) |

A browser preview is labelled **preview** and has no id. Only a server result has a fingerprint, guards evaluated against the derivation graph, a citation, and can be saved in a map. Spatial statistics, anything needing weights or the grid store, and anything on more than 20,000 rows are server-only. If WebAssembly is unavailable the client calls the server for classification too.

**Sync or async.** `cost()` decides: estimated ≤ 2 s → computed inside `lanka serve` on a bounded blocking pool (2 threads, queue of 8, shed with 503 beyond) and returned directly; otherwise the request becomes a job and the response is `202` with the result URL in `state: "queued"`, polled with `Retry-After` (or `GET …/events` as server-sent events). Typical placement **[A, measured in WP-29/30]**:

| Request | n | Expected | Mode |
|---|---|---|---|
| `relate` at ADM3, island | 339 | 30 ms | sync |
| `relate` at ADM4, one province, 999 permutations | ~2,500 | 0.5–1 s | sync |
| `relate` at ADM4, island, 999 permutations | 14,043 | 4–8 s | async |
| `group_compare` on 50,000 points with rings | 50,000 | 0.3 s | sync |
| `exposure` for one district | grid window | 50 ms | sync |
| `hotspots` at ADM4, island | 14,043 | 2–4 s | async |
| `interpolate` kriging, 5,000 samples, one district | ~0.5 M cells | 10–40 s | async |
| `gwr` | 5,000 | minutes | async |

**The interactive lane.** Async analyses must not queue behind a two-hour routing recipe. `ops.jobs` gains a `lane` column (`batch` default, `interactive`) and the kind `analysis`; a second worker process, `lanka worker --lane interactive` (compose service `analyst`, `mem_limit: 1.5g`, `cpus: 2`, database role `lanka_analysis`), claims only that lane. It runs up to 2 jobs at once, each with a wall-clock limit (120 s anonymous, 15 min with a key) and a row/cell budget checked before allocation. The capacity plan of [05 §12](05-storage-and-performance.md#12-capacity-plan--one-vps-8-vcpu--15-gb-ram--150-gb-nvme) rises from 12.3 GB to 13.8 GB peak; `analyst` is the first service to move when scale-out step S2 (worker box) fires.

## 9. Caching and materialisation

The recipe fingerprint rule ([04 §1](04-enrichment.md#1-vocabulary)) is reused unchanged:

```
fingerprint = sha256( canonical JSON of the resolved request − pin    -- "latest"/"auto"/defaults made explicit
                      ‖ method id@version+r<revision> ‖ engine revision ‖ alignment method ids@versions
                      ‖ sorted input dsvs (and grid layer sha256s, spatial-weights sha256)
                      ‖ lanka-stats version ‖ tool versions if any )
id = "an_" ‖ first 16 hex
```

- `pin: "latest"` is resolved to the current release **before** fingerprinting — the release current in the database at request time, not a host's catalogue snapshot — so a new release that changed an input produces a new id (stale results are never served) and one that did not produces the same id (the cache survives the release). The pin itself is left out of the fingerprint; the input versions it resolved to are in. This is the staleness rule of [04 §7](04-enrichment.md#7-dag-execution-and-incremental-recompute).
- Code changes invalidate too: each method carries an implementation **revision** (`relate@1.0.0+r2`), bumped whenever the same request on the same inputs would give a different document (new estimates such as `partial`, changed numbers or wording) without a contract change, and the engine carries one for the shared pipeline (alignment, guards, presentation). Both enter the fingerprint and are reported (`method.revision`, `provenance.engine_revision`), so a deployment that changes results never serves documents computed by the old code for new requests; results stored under old ids stay readable by id.
- `POST /v2/analyses` → `303 See Other` to `GET /v2/analyses/{id}`, which is `immutable` and cached by nginx. Identical requests from different users cost one computation.
- Alignment is cached separately (`analysis.aligned`, keyed by variable + support + alignment + dsv): changing the transform or the method reuses the aligned columns. The frequently needed alignments (every `serve: true` grid layer × `grid_zonal` × ADM2–ADM4; feature counts per unit) are already published observations from recipes, so alignment is usually a frame read.

```sql
CREATE SCHEMA analysis;
CREATE TABLE analysis.results (
  id           text PRIMARY KEY,                    -- 'an_…'
  fingerprint  bytea NOT NULL UNIQUE,
  method       text NOT NULL, method_version text NOT NULL,
  request      jsonb NOT NULL,                      -- resolved
  state        text NOT NULL CHECK (state IN ('queued','running','ready','failed','blocked')),
  result       jsonb,                               -- the result document without bulky outputs
  outputs      jsonb,                               -- [{id, kind, blob key, bytes, sha256}]  → store/analysis/<id>/…
  basis        int2, licence_id text, redistribution text,
  workspace_id bigint,                              -- NULL = all inputs public; else row-level security as in ws.*
  data_version text NOT NULL,
  cost         int4, elapsed_ms int4, job_id bigint,
  created_at   timestamptz NOT NULL DEFAULT now(), last_read_at date NOT NULL DEFAULT current_date,
  pinned       boolean NOT NULL DEFAULT false       -- referenced by a saved map version or exported as a recipe
);
CREATE TABLE analysis.result_inputs (               -- "which results used this dataset version": takedown, licence changes
  result_id text NOT NULL REFERENCES analysis.results (id) ON DELETE CASCADE,
  dataset_ref text NOT NULL, dsv text NOT NULL, role text NOT NULL,
  PRIMARY KEY (result_id, dataset_ref, role)
);
CREATE TABLE analysis.aligned (
  key bytea PRIMARY KEY, support jsonb NOT NULL, blob_key text NOT NULL, bytes int8 NOT NULL,
  workspace_id bigint, created_at timestamptz NOT NULL DEFAULT now(), last_read_at date NOT NULL
);
CREATE TABLE analysis.spatial_weights (
  boundary_set text NOT NULL, scheme_id text NOT NULL, level int2 NOT NULL, kind text NOT NULL,   -- queen | knn6 | band_5km
  n int4 NOT NULL, nnz int8 NOT NULL, islands int4[] NOT NULL, blob_key text NOT NULL, sha256 bytea NOT NULL,
  PRIMARY KEY (boundary_set, scheme_id, level, kind)
);
```

Outputs are Arrow/Parquet frames, grid arrays in the `LKGRID01` format, or GeoParquet features under `store/analysis/<id>/` (new blob namespace `Analysis`; never under `/dl`, always served through the application so access checks apply). Eviction: unpinned results unread for 30 days and aligned columns unread for 14 days are deleted by the maintenance job; the namespace has a size cap (10 GB) with least-recently-read eviction. Takedown of a dataset deletes its dependent results through `analysis.result_inputs`.

## 10. Data reality: property prices

There is no open, sub-district property price dataset for Sri Lanka. The registry already says so. What exists, and what the platform does about each:

| Source | What it is | Geography | Access and licence status | Decision |
|---|---|---|---|---|
| CBSL Land Valuation Indicator (`cbsl-land-valuation-indicator`, registered) | Semi-annual index of land values by use | Colombo district **[A: whether DS-division sub-indices are published; check in WP-38]** | Public news releases; licence unstated → `permission_unclear`, tier `api` | Ingest as a time series (class B). Context for trends; not usable for spatial analysis. |
| CBSL condominium survey and asking-price indices (`cbsl-condominium-prices`, registered) | Quarterly indices | Colombo district / national | as above | Ingest as a time series. |
| Government Valuation Department | Valuations for stamp duty, rating and acquisition | parcel / local authority | Not published. Obtainable only by agreement or a Right to Information request for aggregates **[A]** | Register as `blocked`; request aggregated median assessed value per local authority ward. Never parcel-level (personal data). |
| Registrar General's Department (deeds) | Transactions with declared consideration | parcel | Not open; declared values are known to understate **[A]** | Not pursued. |
| Local authority assessment rolls | Annual rateable value per property | parcel | Held per council, not open; contains owner names | Not pursued except as aggregates supplied by a council through BYOD. |
| Listing portals (ikman.lk, LankaPropertyWeb, house.lk and similar) | Asking prices with location text, sometimes coordinates | point / town | Proprietary. Terms of use restrict automated collection **[A: read each portal's current terms in WP-38]**; listings contain seller contact details (personal data under the Personal Data Protection Act No. 9 of 2022) | **The platform does not scrape listing portals, and scraped listings are not accepted into the public layer.** Reasons: contract (terms of use), copyright in the compilation, exposure under the Computer Crime Act No. 24 of 2007 where access controls are circumvented **[A]**, personal data, and the project's licence-honesty rule ([08 §2](08-openness-and-governance.md#2-licence-honesty-for-data)). Route: ask portals for an aggregated product (median asking price per perch by DS division or GN, by quarter, with n) under a written licence; publish that at the tier the licence allows. |
| Commercial research (portal price indices, consultancy reports) | Indices and reports | city / district | Proprietary | Link-tier metadata only. |
| Banks, valuers, developers, researchers holding their own data | Valuations, transactions, survey samples | point / parcel | Theirs | **Bring your own data** ([12 §5](12-enterprise-and-byod.md#5-bring-your-own-data)): private by default; may be published as aggregates with a minimum cell count. |
| Open proxies already in the plan | building density and height, night lights, travel time to Colombo, census housing type and tenure, HIES housing expenditure by district | grid / ADM2–ADM4 | open or `permission_unclear` | Published as what they are. **No "estimated land price" indicator is built from proxies** — it would be a price in name only. |

Consequences for the product:

- The flood-versus-price example ships against `@demo/property-asking-prices-sample`: a **synthetic** point dataset (generated from a documented hedonic formula plus noise over real settlement locations; licence CC0; flagged `synthetic: true`, which raises `SYNTHETIC_INPUT` on every result). It exists for documentation, tests and the front-end's mock data, and is labelled as synthetic everywhere it appears.
- The same analysis pattern runs on fully open data from day one — flood exposure against night-light intensity, building density, population density or (once published) small-area poverty — so the public site shows real results.
- Real price analysis is a BYOD use case until a licensed aggregate exists. WP-38 is the outreach package: written requests to two portals, CBSL and the Valuation Department, with the aggregated schema above.

## 11. Interfaces

### 11.1 REST

| Call | Notes |
|---|---|
| `GET /v2/analysis/methods` · `/v2/analysis/methods/{id}` | catalogue: roles, supports, parameter schema, result schema, min n, guards, citations, method page |
| `POST /v2/analyses` | body `ldl-analysis/1`; public inputs only. `303` → result (ready), `202` (queued), `400` with `problems` (JSON pointers) and `hints` (unknown indicator, no common support, level unavailable → `available_levels`, a requested period without data → `problems[].path` `/variables/<role>/period` and `available_periods`; the plan answers the same 400 — an input that resolves to no data is never a blocked result or a 500, and an override cannot run it), `429` over quota |
| `POST /v2/analyses/plan` | dry run: chosen support, alignment per variable, expected n, cost, sync/async, guards already known. Used by the UI before the user commits. |
| `GET /v2/analyses/{id}` · `/outputs/{output}?format=json\|arrow\|csv\|parquet\|geojson` · `/outputs/{output}/values/{z}/{x}/{y}.png` (grid outputs) · `/events` | immutable once `ready` |
| `GET /v2/analyses/{id}/bundle` | zip: `result.json`, outputs as Parquet/COG, `README.md` (method, guards, citation), `prov.jsonld`, `datapackage.json` |
| `GET /v2/analyses/{id}/recipe` | recipe skeleton for promotion (operators) |
| `POST /app/v1/ws/{ws}/analyses` and the same sub-resources | when any input is workspace data |

### 11.2 MCP

Added to the tool list of [06 §4](06-serving.md#4-mcp-server), all read-only except `create_map_link`:

| Tool | Arguments | Returns |
|---|---|---|
| `list_analysis_methods` | `tier?` | methods with roles and when to use each |
| `analyze_relationship` | `x, y, controls?, level \| support, within?, transforms?` | `relate` result: summary, estimates, **guards in the text block before the numbers**, result URL |
| `compare_groups` | `y, zone, rings_m?, within?, support?` | `group_compare` result |
| `find_clusters` | `indicator, level, within?, kind: hotspots \| lisa` | cluster table + result URL |
| `exposure_summary` | `hazard, threshold \| return_period, exposed[], geo` | counts and shares with intervals |
| `detect_change` | `indicator, geo \| level, from, to` | change table with significance |
| `get_analysis` | `id` | a stored result |
| `create_map_link` | `layers \| analysis_id, title` | a snapshot permalink ([10 §6](10-map-composition.md#6-permalinks-embeds-forks-export)); annotated `readOnlyHint: false, idempotentHint: true` (content-addressed) |

Async results return `state: "queued"` and the `get_analysis` call to repeat. Because a language model will paraphrase, the text rendering leads with `NOT_CAUSAL` and every `warn` guard, and never includes a hidden *p*-value.

### 11.3 CLI

`lanka analyze <request.json> [--pack …] [-o result.json]` runs the same engine; against an offline pack it supports every method whose inputs are in the pack (tabular supports; grid methods need the grid arrays). `lanka analysis methods | export-recipe <id> | weights build`.

### 11.4 UI patterns

| Pattern | Entry | Screen |
|---|---|---|
| **Relate two layers** | select two layers in the panel → "Relate" | plan sheet (support chosen, what will be dropped, guards known in advance) → bivariate layer + scatter panel with linked brushing (brush the scatter → units highlight; select units → points highlight) + stats card whose first block is the guards |
| **Inside vs outside** | a polygon or raster layer → "Compare inside / outside" → pick the measured layer | ring diagram (inside, 0–250 m, 250–500 m), dot-and-interval plot of group medians, effect size, map of groups |
| **Find clusters** | a choropleth layer → "Hot spots" | cluster layer (FDR-significant only), count per class, toggle to show non-significant units greyed |
| **Exposure** | a hazard layer → "Who is exposed?" | threshold slider (client-side mask preview on value tiles), exposed population/buildings/facilities per unit with intervals |
| **Change** | a layer with a time series → "Change between…" | diverging choropleth + list of significant changes |
| **Explain this result** | any stats card | method, inputs with versions and licences, alignment steps with what was dropped, guards, citation, download bundle |

Rules for every pattern: the plan is shown before the run; guards are rendered above estimates; numbers render with their intervals; a blocked analysis explains what n was and what would unblock it; "Add to map" inserts the result's `suggested_layer` and stores the request in the document.

## 12. Limits

| | Anonymous | Free key | Workspace plans / on-prem |
|---|---|---|---|
| Sync analyses | 30 / hour / IP | 300 / hour | configurable |
| Async jobs | 3 / hour, 1 at a time, 120 s each | 30 / hour, 2 at a time, 15 min | configurable |
| Rows per frame | 20,000 (the whole island at ADM4 fits) | 250,000 | configurable |
| Grid window | one province at k = 1; island at k ≥ 5 | island at k = 1 | — |
| Permutations | ≤ 999 | ≤ 9,999 | — |
| Tier 3 methods | no | yes | yes |
| Result retention when unpinned | 30 days | 30 days | 180 days |

Quotas are enforced in the application per IP or per key ([12 §6](12-enterprise-and-byod.md#6-api-keys-quotas-plans)); reading a cached result is free and unlimited.

## 13. Testing

| Layer | What |
|---|---|
| Unit | every kernel on textbook data with published answers (Anscombe, Columbus crime, Boston housing subsets) |
| Reference | agreement with R `spdep` / `spatialreg` / `sandwich` / `gstat` / `GWmodel` on committed fixtures to 1e-8 for deterministic statistics and exact for seeded permutations after porting the RNG stream |
| Property | invariance to row order; `relate(x, y)` and `relate(y, x)` agree on *r* and *L*; alignment preserves totals for extensive variables; the same request yields the same fingerprint and bytes on native and WASM for the kernels both run |
| Guards | one positive and one negative fixture per guard code; the vocabulary scan of §6 |
| Golden | the Western Province example end to end: request → result JSON compared with a committed file |
| Corpus | analysis requests are added to the conformance corpus of [06 §1](06-serving.md#1-one-query-core-two-backends) and run on both backends |
| Performance | the table of §8 becomes a benchmark with the usual 30 % regression gate |
