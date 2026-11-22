# Analysis guard rails (11 §6, ADR 0022)

Every result carries guards — `{code, severity: note | warn | block, message, detail?, action?, overridden?}` —
sorted block → warn → note with `NOT_CAUSAL` first among the notes. A `block` returns `state: "blocked"` with no
estimates, no outputs and no interpretation; under `SMALL_N` p-values are not in the result at all. Messages are
fixed templates (`lanka-analysis/src/guards.rs`); a test scans every template and every rendered summary and
reading for causal wording ("causes", "leads to", "due to", "because of", "effect of", "impact of").
`GET /v2/analysis/methods` serves the registry.

| Code | Active | Trigger (WP-29 implementation) | Severity |
|---|---|---|---|
| `NOT_CAUSAL` | WP-29 | relationship methods (`relate`, `regress`, `group_compare`) | note |
| `MIN_N` | WP-29 | complete cases below the method's block threshold | block |
| `SMALL_N` | WP-29 | between block and warn thresholds | warn; p removed |
| `MAUP` | WP-29 | areal relationship results; carries the scale-sensitivity line | note |
| `MAUP_UNSTABLE` | WP-29 | sign flip or \|Δr\| > 0.2 at an adjacent level | warn |
| `ECOLOGICAL` | WP-29 | an input's unit (or its density's source) counts persons, households, dwellings, buildings, properties | warn |
| `RESIDUAL_AUTOCORR` | WP-29 | residual Moran's I (kNN weights) permutation p < 0.05 | warn |
| `SIZE_CONFOUND` | WP-29 | x and y both `count`/`sum` | warn |
| `SHARED_LINEAGE` | WP-29 | x and y share an upstream dataset in the recipe graph (the boundary dataset excluded) | warn |
| `ESTIMATED_INPUT` | WP-29 | any column with basis estimated/forecast or class `estimated` | note |
| `MODEL_ON_MODEL` | WP-29 | x and y both estimated | warn |
| `INPUT_UNCERTAINTY_IGNORED` | WP-29 | an input carries intervals (analytic uncertainty) | note |
| `PERIOD_MISMATCH` | WP-29 | periods > 2 years apart; > 5 years blocks unless `override` | warn / block |
| `LOW_COVERAGE` | WP-29 | complete cases < 80 % of the support; < 30 % blocks | warn / block |
| `PSEUDO_REPLICATION` | WP-29 | a column aligned by `inherit` (effective n = parents) | warn |
| `OUTLIER_LEVERAGE` | WP-29 | robust vs OLS slope > 25 % apart or max Cook's D > 1 | warn |
| `NO_CONTROLS` | WP-29 | one predictor | note |
| `MULTICOLLINEAR` | WP-29 | any VIF > 10 | warn |
| `SUPPRESSED_CELLS` | WP-29 | units excluded because the publisher suppressed values | note |
| `SYNTHETIC_INPUT` | WP-29 | dataset `@demo/…` or limits say synthetic | warn |
| `COMMUNITY_INPUT` | WP-29 | workspace data (`@…`) | warn |
| `DISAGGREGATED_INPUT` | WP-17 | `dasymetric` alignment | warn |
| `MULTIPLE_TESTING` | WP-30 | local statistics | note |
| `INTERP_SOCIO` | WP-33 | interpolated socio-economic surface | warn |
| `EXPLORATORY` | WP-37 | tier-3 methods | warn |

Overridable: `PERIOD_MISMATCH` only. An override is recorded (`overridden: true`) and repeated in the citation.
Tests: `lanka-analysis/src/tests.rs` — one positive and one negative fixture for every guard active in WP-29.
