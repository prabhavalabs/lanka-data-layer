# 04 — The enrichment module

Enrichment turns standardised inputs into data nobody publishes: comparable boundaries over time, annual GN populations, GN-level allocations of district statistics, zonal statistics of rasters, per-capita rates, travel times, hazard exposure, small-area poverty, composite indices ([`docs/data/value-added.md`](../data/value-added.md), ~108 person-days for the top 20).

It is a crate (`engine/crates/lanka-enrich`), a registry directory (`registry/recipes/`), and four tables. It has no service of its own: recipes run as jobs in `lanka worker`.

## 1. Vocabulary

| Term | Is | Lives in |
|---|---|---|
| **Method** | A versioned, tested algorithm with typed inputs, parameters and outputs. Code. | `lanka-enrich/src/methods/<id>.rs`, registered in `catalog.methods` |
| **Recipe** | A declarative binding: method(s) + parameters + input datasets → one output dataset. Data. | `registry/recipes/<dataset_id>.yaml`, mirrored in `catalog.recipes` |
| **Derivation** | One execution of a recipe over pinned input versions, producing one dataset version | `data.derivations` |
| **Fingerprint** | sha256(recipe spec ‖ method ids@versions ‖ params ‖ tool versions ‖ sorted input dsvs) | `data.derivations.fingerprint` |

A derived dataset is an ordinary dataset: it has a registry file (`registry/datasets/derived-*.yaml` with description, method text, **limits**), indicators, versions, a dsv, a place in releases, a licence (computed from inputs, [02 §8](02-standard-model.md#8-licences-attribution-redistribution)).

## 2. Method contract

```rust
// engine/crates/lanka-enrich/src/method.rs
pub struct MethodInfo {
    pub id: &'static str,                 // "areal_weighting"
    pub version: semver::Version,         // bump MAJOR when outputs change for the same inputs
    pub summary: &'static str,
    pub citation: &'static str,           // "Tobler 1979; Logan, Xu & Stults 2014"
    pub inputs: &'static [PortSpec],      // name, kind (indicators|grid|features|boundaries|crosswalk), required
    pub params: schemars::Schema,         // JSON Schema; recipes are validated against it
    pub output_basis: Basis,              // computed | estimated | forecast — the *minimum*; see §5
    pub uncertainty: UncertaintyKind,     // None (computed only) | Interval | QualityClass | ModelSpread
    pub mandatory_flags: Flags,           // flags every output row carries
    pub tools: &'static [&'static str],   // ["gdal", "valhalla"] — versions enter the fingerprint
}

pub trait Method: Send + Sync {
    fn info(&self) -> &MethodInfo;
    /// Deterministic. No network. Reads only through `ctx.inputs` (pinned versions), writes only through `out`.
    fn run(&self, ctx: &MethodCtx<'_>, params: &serde_json::Value, out: &mut dyn OutputSink) -> Result<MethodReport>;
    /// For one output key, the input rows and weights that produced it (provenance drill-down).
    fn explain(&self, ctx: &MethodCtx<'_>, params: &serde_json::Value, key: &ObsKey) -> Result<Explanation>;
}

pub struct MethodCtx<'a> {
    pub inputs: &'a PinnedInputs,   // name → (dataset, dsv, version seq); typed readers: obs(), grid(), features(), units(), crosswalk()
    pub pg: &'a PgRead,             // read-only connection, search_path limited, statement_timeout per recipe
    pub grids: &'a GridStore,
    pub scratch: &'a ScratchSchema, // temp tables in a per-run schema, dropped afterwards
    pub tools: &'a ToolClient,      // sandbox sidecar (gdal) and batch services (valhalla)
    pub rng_seed: u64,              // first 8 bytes of the fingerprint
    pub as_of: NaiveDate,           // never the wall clock
}

pub struct MethodReport { pub qa: Vec<QaResult>, pub diagnostics: serde_json::Value, pub rows: u64 }
```

Every method ships with: unit tests on synthetic inputs with known answers; at least one **invariant property test** (§9); a golden-file test on a small real extract; a method page (`docs/methods/<id>.md`, generated header + hand-written body) with citation, assumptions, limits and validation results.

### Where the computation runs

| Kind of method | Engine | Why |
|---|---|---|
| Tabular (crosswalk, roll-up, per-capita, interpolation, composite) | Set-based SQL in Postgres, orchestrated from Rust, results streamed to the sink | Inputs are already there; joins on integer keys; 14k units × years is small |
| Grid (zonal, dasymetric, overlay, cost-distance) | Rust kernels over memory-mapped arrays (`lanka-grid`), `rayon` over rows | 11.34 M cells; a full pass is tens of ms per layer; no row store involved |
| Routing | Valhalla (C++), Docker Compose profile `batch`, started and stopped by the job | No Rust router matches it; batch-only so it costs no steady-state RAM |
| Statistics (Fay–Herriot, PCA, kriging, IPF) | Rust (`faer` for linear algebra) | Small dense problems (≤ 14k rows); golden outputs generated once with R (`sae`, `gstat`) and committed as fixtures — R is a test-fixture tool, not a runtime dependency |

No Python, no R, no notebook at runtime.

## 3. Method library (v1 of the library)

| Id | Does | Inputs | Key params | Output basis / uncertainty | Mandatory flags | Used by (value-added #) |
|---|---|---|---|---|---|---|
| `rollup` | Parent values from children by the indicator's `aggregation` rule | indicators, units | levels, `min_coverage` (default 1.0) | computed / none | `PARTIAL_COVERAGE` if < 1 | all; census ADM1 |
| `ratio` | Indicator arithmetic: ratio, share, density, per-N, change, growth | indicators (+ denominator) | formula (small expression grammar), `per`, `match_period: exact|nearest≤k|interp` | inherits worst input basis / interval by delta method when inputs have one | `DENOM_MISMATCH` when period differs | #4 per-capita |
| `crosswalk_rebase` | Re-base values between boundary vintages or schemes | indicators, crosswalk | `weight_kind: population|area|buildings`, count vs rate handling | estimated where any weight < 1, else computed / `lo`,`hi` from alternative weight kinds (`MODEL_SPREAD`) | `REBASED`, `SPLIT_UNIT` | #1 harmonised series |
| `build_crosswalk` | Compute overlap weights between two boundary sets | boundaries ×2, population grid, buildings | `min_sliver`, weight kinds | — (emits a crosswalk dataset) | — | #1 |
| `temporal_interp` | Fill periods between observations | indicators | `mode: linear|geometric|ratio_to_trend`, `controls` (parent totals to rake to) | estimated / interval = back-cast error at held-out anchors | `INTERPOLATED_TIME` | #2 intercensal |
| `temporal_project` | Extend beyond the last observation | indicators, control totals | `mode: ratio_to_trend|share_of_growth`, horizon | forecast / interval widening with horizon | `EXTRAPOLATED_TIME` | #2 |
| `rake` | Iterative proportional fitting to marginal controls | indicators, controls | dims, `tol`, `max_iter` | estimated / none added | — | #2, #5 |
| `grid_zonal` | Grid layer → units | grid, units (`gnd_idx` array) | `stat: sum|mean|pop_weighted_mean|class_share|max|pctl`, `min_cells` | computed when the grid is observed counts; estimated when the grid is itself modelled (WorldPop) / CV across models if several grids | `SMALL_N` below `min_cells` | #3, land cover, terrain, NTL |
| `features_zonal` | Point/line/polygon features → counts, lengths, areas per unit | features, units | `measure: count|length_km|area_km2`, filters | computed / none | `UNOFFICIAL_GEOM` passthrough | facility counts, road km |
| `dasymetric` | Disaggregate a unit total to finer units/cells using ancillary weights; totals preserved | indicators (coarse), ancillary grids | `ancillary: [{layer, weight}]`, `mask`, `target: adm4|grid`, `fit: fixed|regression` | estimated / quality class A–D from ancillary fit | `ANCILLARY_WEAK` | #5 |
| `exposure_overlay` | Hazard mask × exposed quantity | hazard grid, population/buildings/features | thresholds, return periods | estimated / interval from hazard class bounds where provided | hazard model + RP in params | #8, #9, #20 |
| `cost_distance` | Travel time over a friction surface to the nearest target | friction grid, targets | mode, max minutes | estimated / none; quality by road completeness | — | #6 fallback, #11 |
| `network_access` | Travel time to nearest facility / catchment populations by routing | routing graph, facilities, population grid | mode, thresholds (15/30/60), sample density | estimated / `lo`,`hi` from speed-profile variants | — | #6, #7, #15 |
| `walkshed` | Network isochrones around stops/stations; population by dasymetric weights | graph, stops, population grid | minutes/metres | estimated / uniform-vs-dasymetric spread | — | #15 |
| `spatial_interp` | Kriging/IDW for **station-sampled continuous fields only** | station observations, covariates (elevation) | variogram model, neighbours | estimated / kriging variance | `SMALL_N` by station density | #19 |
| `anomaly_index` | SPI, percent-of-normal, z-scores vs a baseline period | indicators or grids | baseline, window | computed (it is arithmetic on the series) / none | baseline in params | #19 |
| `small_area_fh` | Fay–Herriot area-level model, benchmarked to direct estimates | direct estimates + variances, covariates | covariates, benchmark level, `cv_suppress` (default 0.30) | estimated / 95 % interval + CV; rows above the CV limit → `status = suppressed` | `SMALL_N` | #10 |
| `composite_index` | Normalise → weight → aggregate; Monte Carlo uncertainty and sensitivity | indicators | normalisation, weights (`equal|pca|fixed`), aggregation (`arithmetic|geometric`), `mc_runs` (default 500) | estimated / 90 % rank interval + sub-indices as separate indicators | weights in params | #13, #14, #17 |
| `entity_resolution` | Conflate features from several sources into entities | features ×n, Wikidata | name similarity, distance (250 m), blocking | — (emits features + `geo.entities`) / match confidence per entity | — | #16 |
| `calibrate_elasticity` | Fit a log-log relation between a proxy and an official aggregate; apply at lower level | proxy indicators, official aggregate | level, robust fit | estimated / interval from coefficient CI | — | #12 |

**Interpolation policy (binding, from `value-added.md`).** `spatial_interp` refuses indicators whose `measure_type` is `count`/`sum`/`share` (counts and administrative statistics are never kriged; they use `dasymetric` or `crosswalk_rebase`). Population time interpolation uses `geometric` or `ratio_to_trend`, always raked to official controls. These are checks in the method's parameter validation, not guidance. On-demand analysis adds one fenced case for point samples of intensive socio-economic quantities (asking prices); it does not apply to recipes — see [11 §3.4](11-analysis-engine.md#34-interpolation-policy-for-point-samples-amends-04-3) and [ADR 0025](adr/0025-price-data-and-point-sample-policy.md).

## 4. Recipes

```yaml
# registry/recipes/derived-intercensal-population.yaml
id: derived-intercensal-population
output: derived-intercensal-population         # registry/datasets/<same id>.yaml holds title, limits, indicators
stability: experimental                         # experimental | stable   (auto-publish only when stable)
inputs:
  census12:  { dataset: derived-harmonised-boundary-series, indicators: [hbs.pop_total_2012] }
  census24:  { dataset: dcs-census-2024-gnd,                indicators: [census24.pop_total] }
  controls:  { dataset: dcs-midyear-population-district }
  wpp:       { dataset: un-wpp-lka, optional: true }
steps:
  - id: interp
    method: temporal_interp@^1
    with:   { from: census12, to: census24, mode: geometric, periods: "2013..2023", by: [sex] }
  - id: rake
    method: rake@^1
    with:   { input: interp, controls: controls, level: 2, dims: [sex], tol: 1e-6 }
  - id: project
    method: temporal_project@^1
    with:   { input: rake, mode: ratio_to_trend, horizon: 2030, controls: wpp }
  - id: up
    method: rollup@^1
    with:   { input: [rake, project], levels: [3, 2, 1, 0] }
emit:
  - { from: [rake, project, up], indicator: est.pop_total }
qa:                                             # gates: a failed gate blocks publication of this version
  - { check: sum_to_parent, indicator: est.pop_total, tol_pct: 0.5 }
  - { check: equals_input, at: "2024", input: census24, tol_pct: 0.0 }      # anchors reproduced exactly
  - { check: backcast, method: interp, holdout: "2012", max_mape: 0.08 }
  - { check: interval_present }
```

Rules:

- Steps form a small DAG inside the recipe (a step's `input` names an earlier step or a declared input). Intermediate results live in the run's scratch schema and are not published unless `emit` names them.
- Method versions are semver ranges; the resolved version is recorded in the derivation. A MAJOR bump of a method requires a recipe edit (deliberate).
- `registry/schema/recipe.schema.json` validates recipes; `lanka recipe check` also type-checks ports against `MethodInfo.inputs` and parameters against each method's schema, and proves the global DAG is acyclic.
- `lanka recipe run <id> [--inputs release:<v> | approved] [--dry-run] [--explain <key>]` runs one recipe locally with the same code path as the worker.

## 5. Uncertainty and status propagation

A derived value is never "more certain" than its inputs. The runner enforces this after every step, independent of what the method claims:

| Property | Rule |
|---|---|
| `basis` | `max` over inputs and the method's `output_basis` in the order `reported < computed < estimated < forecast` |
| `status` | `suppressed` if any required input is suppressed; `low_reliability` if any input is; `provisional` if any input is; `missing` if a required input is missing (no silent zero) |
| `flags` | bitwise OR of input flags that are marked *propagating* in the spec (`REBASED`, `SPLIT_UNIT`, `SMALL_N`, `OCR`, `LLM_READ`, `PARTIAL_COVERAGE`, `UNOFFICIAL_GEOM`) plus the method's mandatory flags |
| `lo`/`hi` | per method, using one of four documented techniques below; a method with `uncertainty = Interval` that returns a row without bounds fails the run |

Techniques (the method page says which one and why):

1. **Analytic.** Sums: variances add (independence assumed and stated). Ratios `r = a/b`: delta method `var(r) ≈ r²(var(a)/a² + var(b)/b²)`. Intervals are converted to variances via the indicator's `ci_level`.
2. **Empirical back-cast.** Hold out a known anchor (a census year, a set of stations), run the method, take the error distribution by unit-size class → interval half-width. Used by `temporal_interp`, `dasymetric`, `grid_zonal` of modelled grids.
3. **Monte Carlo.** Draw inputs from their intervals (and weights from their plausible ranges), seeded by the fingerprint, `mc_runs` draws, report percentiles. Used by `composite_index`, `exposure_overlay`.
4. **Model spread.** Run alternative models/weightings and report min/max with `MODEL_SPREAD` (WorldPop vs HRSL vs GHSL; population- vs area-weighted crosswalk; equal vs PCA weights). Honest when no error model exists.

When none applies, the method must emit a **quality class** (A–D) with published class definitions instead of an interval.

## 6. Explainability

`Method::explain` answers "which inputs made this number" without storing per-row lineage for millions of rows:

```
GET /v2/observations/{obs_id}/provenance?depth=1
→ derivation { recipe, method steps, params, input versions, fingerprint, code rev }
  contributors: [ { obs_id, indicator, geo, period, value, weight, role: "numerator|denominator|source_unit|anchor|cell_sum" } … ]
  formula: "est.pop_total[LK1103005, 2019] = geometric(census12', census24) × rake_factor(LK11, 2019)"
```

For grid methods the contributors are summarised (`cells: 143, layer, sum, mean`), not listed. `depth=n` recurses through contributors that are themselves derived, down to extracted rows with document locators.

## 7. DAG execution and incremental recompute

```sql
CREATE TABLE catalog.methods (id text, version text, info jsonb NOT NULL, PRIMARY KEY (id, version));
CREATE TABLE catalog.recipes (
  id text PRIMARY KEY, output_dataset text NOT NULL UNIQUE REFERENCES catalog.datasets (id),
  stability text NOT NULL CHECK (stability IN ('experimental','stable')),
  spec jsonb NOT NULL, spec_sha256 bytea NOT NULL
);
CREATE TABLE catalog.recipe_inputs (               -- the global DAG's edges
  recipe_id text NOT NULL REFERENCES catalog.recipes (id) ON DELETE CASCADE,
  port text NOT NULL, dataset_id text NOT NULL REFERENCES catalog.datasets (id), optional boolean NOT NULL DEFAULT false,
  PRIMARY KEY (recipe_id, port)
);
CREATE TABLE data.derivations (
  version_id   bigint PRIMARY KEY REFERENCES data.dataset_versions (id),
  recipe_id    text NOT NULL REFERENCES catalog.recipes (id),
  fingerprint  bytea NOT NULL,
  methods      jsonb NOT NULL,     -- [{step, id, version}]
  params       jsonb NOT NULL,
  inputs       jsonb NOT NULL,     -- [{port, dataset, dsv, version_id}]
  tools        jsonb NOT NULL,     -- {"gdal": "3.11.x", "valhalla": "3.5.x"}
  git_rev      text NOT NULL, engine_version text NOT NULL,
  report       jsonb NOT NULL,     -- QA results + diagnostics
  job_id       bigint, started_at timestamptz, finished_at timestamptz
);
CREATE UNIQUE INDEX derivations_fp ON data.derivations (recipe_id, fingerprint);
CREATE TABLE data.version_inputs (                 -- normalised copy of inputs for "what depends on this version?"
  version_id bigint NOT NULL, input_version_id bigint NOT NULL, port text NOT NULL,
  PRIMARY KEY (version_id, input_version_id, port)
);
```

**Staleness.** A recipe is stale when the fingerprint computed from (current spec, resolved method versions, tool versions, the input versions that would be pinned by the next release) has no row in `data.derivations`. That single rule covers a new input version, a changed parameter, a method upgrade and a tools-image upgrade.

**Execution** (release train step 3, [03 §8](03-ingestion.md#8-release-train)):

1. Build the candidate input map = current release ∪ newly approved versions.
2. Topologically sort recipes (Kahn); for each, compute the fingerprint; if a derivation exists → reuse its version; else run.
3. A run's output version enters the candidate map immediately, so downstream recipes see it (fixpoint in one pass, because the graph is acyclic).
4. Failure or failed QA gate → the recipe and everything downstream of it keep their previous versions for this release; an incident `recipe_failed` records why; nothing upstream is blocked.

**Cost control.**

- Heavy recipes (`network_access`, island-wide `exposure_overlay`) declare `schedule: weekly|monthly|manual` and are skipped by daily trains even if stale; the dataset card then shows `stale_inputs: true` with the input versions it was computed from. OSM changes every day; travel times do not need to.
- Recipes run *before* the train's lock, in the worker's heavy slot, one at a time; the train only selects finished derivations.
- **Partial recompute** is a method capability, not a framework feature: `rollup`, `ratio`, `grid_zonal` and `features_zonal` accept a `changed_keys` hint (units/periods whose inputs changed, from the input version diff) and recompute only those, copying the rest by range extension (unchanged rows are simply not closed). Methods without the capability recompute fully. Because rows are range-versioned, an unchanged output row costs nothing either way.

### Publication of derived data

Publication classes A/B/C describe extraction and do not apply to recipes. A derived version is gated as follows:

| Situation | Outcome |
|---|---|
| Recipe `stability: stable`, all QA gates pass, method versions unchanged (same MAJOR) since the last published derivation | auto-approved in the train |
| Recipe `experimental`, or first version ever of the dataset, or a method MAJOR bump, or a parameter change | `in_review`: a reviewer reads the derivation report (QA results, validation score, diff against the previous version, the ten largest changes) and approves with `lanka review approve` |
| Any QA gate fails | version `rejected`; previous version stays pinned; incident `recipe_failed` |
| More than 5 % of existing keys change by more than the recipe's `max_revision_pct` (default 10 %) with unchanged method and parameters | demoted to `in_review` (an input changed more than expected) |

Promotion to `stable` needs two consecutive clean releases and a recorded validation score on the method page ([08 §4](08-openness-and-governance.md#4-contribution-model)).

**Sizing [A].** Top-20 products at full coverage: ~60–80 M derived observation rows (dominated by ADM4 × annual × breakdown series and monthly rainfall anomalies), ~40 grid layers. Daily train with no heavy recipes: under 5 minutes. Monthly heavy pass (routing): 2–4 hours in the batch slot.

## 8. Labelling

Estimates must never be mistaken for official figures. Seven mechanisms, all enforced by validators or types rather than by reviewer vigilance:

1. **Separate indicators.** An estimate is never written into an official indicator. `census24.pop_total` holds only what DCS printed (plus `computed` roll-ups where DCS printed none, marked as such); the annual estimate is `est.pop_total`. The validator rejects `basis ∈ {estimated, forecast}` rows in an indicator whose `class` is `official`. Roll-ups are written by recipes into their own `derived-` dataset (an indicator belongs to one dataset), so the roll-up indicator declares `completes: census24.pop_total` (registry; `catalog.indicators.completes`, migration 0013): `/v2/frame`, `/v2/indicators/{id}/map` and `/v2/areas/{code}` serve its rows under the completed id at levels where that indicator has no rows of its own, with `basis = computed`, the roll-up's `obs_id` (provenance), its dataset version in `meta`, and `completed_from: <roll-up id>` on the column/entry. Completion never crosses classes: a modelled value (`pop.grid_total`) never completes an official indicator.
2. **Reserved namespaces.** Indicator ids of class `estimated` start with `est.`, `idx.` (composite indices), `acc.` (accessibility), `exp.` (exposure). Dataset ids start with `derived-`.
3. **`basis` on every row and in every format.** REST, CSV, Parquet, SDMX-CSV (`OBS_STATUS` + a `BASIS` attribute), MCP results. It is not optional metadata; it is a column.
4. **Envelope summary.** `meta.contains = ["reported","estimated"]`, and `meta.estimate_notice` (one sentence naming the method and its page) whenever any returned value is estimated or forecast.
5. **Filters with a safe default for mixing.** Endpoints that return many indicators at once (`/v2/areas/{code}`, `compare`) group values under `official`, `third_party`, `derived`, `estimated` keys; `?basis=reported` restricts any endpoint to reported values.
6. **Mandatory method page and limits.** A derived dataset cannot be registered without `limits` ("what it cannot tell you"), a method page, a validation score and the interval technique. `lanka registry validate` enforces presence; the dataset card shows them above the data.
7. **No gap filling across classes in a series response.** A series for an official indicator never has estimated points spliced in. Clients that want a continuous line request both series and get two, with a `relation: {estimates_of: "census24.pop_total"}` link from the estimate to the official indicator.

MCP-specific: tool results put `basis` beside each value and repeat the notice in the text block, because a language model will otherwise paraphrase an estimate as a fact.

## 9. Testing methods

| Layer | What | Example |
|---|---|---|
| Unit | Synthetic inputs, hand-computed answers | 2 source units → 3 target units with known overlap |
| Property (`proptest`) | Invariants that must hold for any input | `crosswalk_rebase`, `dasymetric`, `rake`: totals preserved to 1e-9 relative; `rollup` of a `sum` indicator equals the sum of leaves; `grid_zonal(sum)` over all units + unassigned cells equals the grid total; `ratio` output is invariant to row order; every method is deterministic for a fixed seed |
| Golden | Small real fixtures, committed expected output (Parquet → dsv compared) | Colombo district subset for each method |
| Reference | Agreement with an established implementation on a fixture | `small_area_fh` vs R `sae::eblupFH`; `spatial_interp` vs `gstat`; `composite_index` PCA weights vs `prcomp` |
| Validation (recorded in the method page, re-run per release) | Real-world accuracy | intercensal back-cast MAPE at GN level; flood exposure hit-rate vs DesInventar; travel time vs a sample of routing-engine queries |

## 10. Build order of methods

Matches [09-roadmap.md](09-roadmap.md): `rollup`, `ratio`, `features_zonal` (WP-06) → `grid_zonal` (WP-05) → `build_crosswalk`, `crosswalk_rebase` (WP-10) → `temporal_interp`, `temporal_project`, `rake` (WP-11) → `dasymetric`, `exposure_overlay` (WP-17) → `cost_distance`, `network_access`, `walkshed` (WP-18) → `anomaly_index`, `spatial_interp`, `calibrate_elasticity` (WP-21) → `small_area_fh`, `composite_index`, `entity_resolution` (WP-22).
