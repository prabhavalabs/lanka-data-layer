# 09 — Roadmap

Work packages are sized for one implementer sub-agent each (package-scoped, contract-first, no commits — the main session reviews and commits, per the repository's sub-agent workflow). "Days" are person-days for a lead reviewing sub-agent work; data-heavy packages take their figures from [`docs/data/inventory.md`](../data/inventory.md) and [`docs/data/value-added.md`](../data/value-added.md). All estimates are **[A]**.

## 1. Ordering logic

1. **Rebase the foundation first** (WP-01…03) while only ~10,000 observations exist. Every later package writes rows; changing the row shape after wave 1 would cost a re-ingest.
2. **Then the cheapest parity and the enrichment enablers together** (WP-04…06): OSM spotlight layers, the grid, roll-ups and per-capita. These need no PDF work and no unclear licences.
3. **Then the two multipliers**: declarative drivers (WP-07) and the PDF harness (WP-08) — 85 of 147 parity rows and ~238 of 338.5 person-days are PDF tables.
4. **Value-added products are interleaved**, not left to the end: items 1–4 of the top 20 (harmonised boundaries, intercensal population, zonal population, per-capita) ship by WP-11, the open-input products (#6–#9, #11) by WP-18.
5. Public interfaces (exports, OGC, MCP, CLI) come once there is enough data to make them worth pointing at (WP-09, WP-13…15), but the REST surface grows with each package.
6. **Maps, analysis and workspaces (WP-26…38, [10](10-map-composition.md)–[12](12-enterprise-and-byod.md)) are a second track interleaved with the first**, ordered by what a visitor can see: composable maps on real data as soon as the foundation and the first layers exist (WP-26…28, alongside Phase B), analysis next (WP-29, WP-30), then ownership (accounts, private data), then teams and on-prem. The front-end track starts immediately against mock data ([ui-contract-explore.md](ui-contract-explore.md)) and switches to the real API package by package (§3, Phase G).

```
WP-01 ─► WP-02 ─► WP-03 ─┬─► WP-04 ─┬─► WP-09 ─► WP-13 ─► WP-14, WP-15 ─► WP-20
                         ├─► WP-05 ─┤
                         ├─► WP-06 ─┴─► WP-10 ─► WP-11 ─► WP-17 ─► WP-18 ─► WP-21 ─► WP-22
                         ├─► WP-07 ─► WP-12 (batches)      WP-16 (after WP-07)
                         └─► WP-08 ─► WP-19 (batches)      WP-24 (after WP-13)   WP-23, WP-25 (ops, parallel)

Second track (maps · analysis · workspaces):
WP-01 ─► WP-26 ──┬─► WP-27 (needs WP-04, WP-05) ───────────────────────────► FE-2
WP-03 ───────────┴─► WP-28 ─► WP-31 ─► WP-32 (needs WP-02) ─► WP-33 (needs WP-29) ─► WP-36 (needs WP-15)
WP-05, WP-06, WP-26 ─► WP-29 ─► WP-30 ─► WP-37          WP-31 ─► WP-35          WP-34 (needs WP-14, WP-29)     WP-38 (any time)
```

Recommended single-lead order through the first user-visible results: WP-01 → WP-02 → WP-03 → **WP-26** → WP-04 ‖ WP-05 ‖ WP-06 ‖ **WP-28** → **WP-27** → **WP-29** → WP-07 ‖ WP-08 (data track continues in sub-agents) → **WP-31** → **WP-30** → **WP-32** → **WP-33** → WP-09 … as before, with WP-34…38 after Phase E.

## 2. What changes in the existing `engine/`

| Path | Verdict | What |
|---|---|---|
| `engine/Cargo.toml` | extend | add members `lanka-store`, `lanka-query`, `lanka-grid`, `lanka-enrich`, `lanka-export`, `lanka-mcp`, `lanka-extractd`; deps `parquet`/`arrow`, `proptest`, `faer`, `rayon`, `memmap2`, `rmcp`, `pmtiles`, `async-trait`, `semver`, `schemars`; `license = "Apache-2.0"` after Q1. `memmap2` needs `unsafe` at the call site: relax `unsafe_code = "forbid"` to `deny` with one audited `#[allow]` module in `lanka-grid` |
| `migrations/0001_foundation.sql` | keep (immutable history) | superseded by 0002–0007 ([05 §1](05-storage-and-performance.md#1-schema-evolution-from-0001_foundationsql)) |
| `lanka-core/src/grid.rs` + `tests/grid_vectors.rs` | keep | parameters read from the country profile; LK vectors stay pinned |
| `lanka-core/src/pcode.rs` | extend | becomes `geo_code.rs`: p-code (profile pattern) + `scheme:code` |
| `lanka-core/src/period.rs` | rewrite (70 lines) | `Period { freq, start }`, full grammar of [02 §5](02-standard-model.md#5-time), render + parse round-trip |
| `lanka-core/src/envelope.rs` | extend | `meta.attribution`, `contains`, `estimate_notice`, `spec_version`, `live`, `page`; keep `source` alias |
| `lanka-core/src/catalog.rs` | extend | surrogate-key maps, codelists, dim sets, units, recipes; move to `lanka-query` |
| `lanka-core/src/query.rs` (973 lines) | split + rewrite SQL | semantics → `lanka-query`; SQL → `lanka-query::pg` on the new tables; `reverse` logic kept and moved behind `Backend::units_at`; `quantile_breaks`, `freshness_state` kept |
| `lanka-core/src/db.rs`, `error.rs` | keep | |
| `lanka-ingest/src/archive.rs` | move | → `lanka-store::FsStore` (same layout, same test) |
| `lanka-ingest/src/connector.rs` | rewrite | traits of [03 §2.1](03-ingestion.md#21-the-traits-enginecrateslanka-ingestsrcconnectorrs-rewritten); `connector_for` match replaced by registry-driven lookup |
| `lanka-ingest/src/pipeline.rs` | rewrite around the kept stage order | add fingerprint short-circuit, conditional GET, streaming fetch, extract/standardise split, job integration |
| `lanka-ingest/src/publish.rs` (727 lines) | rewrite | multiset dsv, stage-table diff, range-versioned insert, `approved` status; `create_release` moves to the train; `approve`/`reject` kept in spirit; CSV quoting helper dropped (binary COPY) |
| `lanka-ingest/src/geo_load.rs` | extend | writes `geo.units` + `geo.unit_geoms`; upsert logic kept |
| `lanka-ingest/src/registry.rs` | extend | new schema fields (units, codelists, aggregation, class, schedule, expect, storage), connectors/templates/recipes/checks directories, overlay directories |
| `lanka-ingest/src/run.rs` | trim | remove `AlertConfig`, `raise_alert`, heartbeat; keep `RunTracker`, events, advisory-lock crash detection; add incidents |
| `lanka-ingest/src/connectors/world_bank.rs` | replace | by `registry/connectors/worldbank-wdi.yaml` on the `http_json` driver; its tests become the driver's golden test |
| `lanka-ingest/src/connectors/dcs_census.rs` | keep as code connector | port to extract/standardise; name matching moves to the pipeline (`GeoRef::Name`) |
| `lanka-ingest/src/connectors/cod_ab.rs` | keep as code connector | fetch via the `ckan` driver helper instead of `--from` v1 layers |
| `lanka-ingest/tests/pg_integration.rs` | extend | [07 §8](07-operations.md#8-testing-strategy) |
| `lanka-server/src/lib.rs` | extend | versioned routes, new resources, OGC, tiles, status; ETag without release id |
| `lanka-server/src/openapi.rs` | keep | |
| `lanka-cli/src/main.rs` | extend | subcommands of [06 §5](06-serving.md#5-cli) |
| `registry/schema/*.json` | extend | + `connector`, `recipe`, `template`, `check`, `codelist`, `profile` schemas |
| `registry/datasets/*.yaml` (263) | mechanical migration | units → UCUM, dims → codelist refs, `measure_type`/`aggregation`/`class` filled by rule with a review list for the ambiguous ones (WP-01 script, output reviewed as one PR) |
| `infra/v2/docker-compose.yml`, `postgresql.conf` | extend | [07 §1](07-operations.md#1-deployment-topology), [05 §6](05-storage-and-performance.md#6-loading-and-maintenance) |
| `engine/README.md` env table | update | drop `LANKA_ALERT_WEBHOOK`, `LANKA_HEARTBEAT_URL`; `LANKA_DATA_DIR` → `LANKA_STORE_DIR` (old name accepted for one release) |
| `engine/Cargo.toml` (second track) | extend | add members `lanka-mapdoc`, `lanka-stats`, `lanka-analysis`, `lanka-app`, `lanka-stats-wasm`; deps `openidconnect`, `jsonschema`, `chacha20poly1305`, `wasm-bindgen` (wasm crate only) |
| `lanka-core/src/query.rs` → `lanka-query` | extend | `Backend::frame(&FramePlan)`, columnar points, value-tile reader; every data method takes `Scope { Public, Workspace }` |
| `lanka-server/src/lib.rs` | extend | `/v2/frame`, `/v2/grids/{id}/values`, `/v2/features/{ds}/points`, `/v2/analyses*`; mounts `lanka-app` routes at `/app/v1` when `LANKA_APP_PLANE=on`; pools B and C |
| `lanka-ingest` (worker) | extend | `--lane batch|interactive`; job kinds `analysis`, `upload`, `sync`; upload path reuses extract/standardise with an interactive mapping |
| `lanka-store` | extend | namespaces `Analysis`, `Ws` (never under `/dl`) |
| `infra/v2/docker-compose.yml` | extend | service `analyst`; `infra/v2/profiles/{public,mirror,private,laptop}` |
| `spec/` | extend | `spec/map/1/` (schema, ramps, embed messages), `spec/analysis/1/` (request, result, per-method schemas, guard registry) |

### Adjustments to earlier packages from documents 10–12

These are additive. None changes a decision of documents 01–09.

| Package | Adjustment | Cost |
|---|---|---|
| **WP-01** | (1) `lanka-core/src/flags.rs`: register bit 16 `INTERPOLATED_SPACE` and bit 17 `DISAGGREGATED`, both propagating ([02 §4](02-standard-model.md#4-the-observation)); `RESERVED_MASK` then covers bits 18–27 and 30–31; the round-trip property test covers 18 bits; the flag label rows in `0002` gain the two codes. (2) Migration `0002`: `CHECK (key < 1000000)` on `catalog.datasets.key`, reserving keys ≥ 1,000,000 (and their `obs_id` range) for workspace datasets. Nothing else: `ws.*` gets its own partition function in WP-32. | ≤ 0.25 d |
| WP-03 | Migration `0003`: create `ops.jobs` with `lane text NOT NULL DEFAULT 'batch' CHECK (lane IN ('batch','interactive'))`, include `lane` in the claim index, and allow kinds `analysis`, `upload`, `sync` from the start. Worker claims by lane. | 0.25 d |
| WP-04 | `admin` tileset: `pcode` present on every level's layer (for `promoteId`) and a `centroids` layer; registry `features` datasets declare typed `fields` (name, type, unit, measure type) next to `tiles.attrs`. | 0.5 d |
| WP-05 | `geo.grid_layers` gains `overview: mean\|sum\|max\|mode`; grid PNG route keeps its shape (value tiles are WP-27). | 0.1 d |
| WP-06 | Methods are callable as a library outside a recipe run (in-memory `OutputSink`, inputs from a `Backend`), because `lanka-analysis` uses them for alignment. | 0.5 d |
| WP-13 | `Scope` threaded through `lanka-query`; `frame` in the conformance corpus; `/id/map/…`, `/id/analysis/…` persistent URIs. | 0.5 d |
| WP-17 | The `exposure_overlay` kernel is delivered by WP-29 and `jrc-river-flood-hazard` ingestion by WP-27; WP-17 keeps the recipes, intervals and DesInventar validation (−1.5 d). | −1.5 d |
| WP-21 | `spatial_interp` (IDW, ordinary/universal kriging) is delivered by WP-33; WP-21 keeps the climate recipes (−2 d). | −2 d |

## 3. Work packages

Format: **goal** · scope (paths) · inputs · acceptance · days · depends on.

### Phase A — rebase the foundation

**WP-01 Standard model in the database and core types** — 6 d — depends: none
- Goal: migration `0002_standard_model.sql` and the core types every other package uses.
- Scope: `engine/migrations/0002_*.sql`, `engine/crates/lanka-core/src/{period,geo_code,units,basis,flags,dims,profile}.rs`, `country/lk/profile.yaml`, `registry/codelists/`, `registry/schema/`, `tools/registry-migrate/`.
- Inputs: [02](02-standard-model.md), [05 §1–§3](05-storage-and-performance.md).
- Acceptance: migration applies on a database holding the current four published versions and every `(indicator, geo, period, dims, value)` is identical before/after (scripted comparison); `geo.admin_units` view returns the same 14,417 rows; registry migrated (263 datasets validate against the extended schema; a generated list of indicators whose `aggregation` could not be inferred is attached for review); property tests for period round-trip, dims canonicalisation, flag registry; row-size and index-size measurements for a synthetic 10 M-row partition recorded in [05](05-storage-and-performance.md) replacing the [A] figures; no `LK` literal left in `lanka-core` (profile-driven).

**WP-02 Store, extracts, connector SDK v2, range-versioned staging** — 8 d — depends: WP-01
- Goal: L0/L1 on `BlobStore`; the new `Connector` trait; multiset dsv; stage → diff → range insert.
- Scope: `engine/crates/lanka-store/`, `lanka-ingest/src/{connector,pipeline,publish,extract}.rs`, `lanka-ingest/src/connectors/{dcs_census,cod_ab}.rs`, migration `0003` (extract tables only).
- Inputs: [01](01-layers.md), [03 §1–§2](03-ingestion.md), [05 §2–§3](05-storage-and-performance.md).
- Acceptance: the three existing sources run end to end on the new path and reproduce the same values as today (8,395 census cells + 365 ratios, 1,203 WDI observations, 14,417 units); a second run is `no_change` at the fingerprint stage with zero fetches; property test of range-version invariants passes; staging 1 M synthetic rows takes < 10 s and appending 1,000 rows to it rehashes only the 1,000; `lanka ingest restandardise` works offline from extracts.

**WP-03 Worker, job queue, release train, status APIs, roles, images** — 7 d — depends: WP-02
- Goal: unattended operation and the emit-only monitoring surface.
- Scope: `lanka-ingest/src/{jobs,scheduler,train}.rs`, `lanka-cli` (`worker`, `release`), `lanka-server` (`/v2/health`, `/v2/status*`), migrations `0003` (rest), `0004_roles`, `0005_serve`, `infra/v2/{docker-compose.yml,deploy.sh,Dockerfile,nginx/}`.
- Inputs: [03 §4, §8](03-ingestion.md), [07 §1–§4](07-operations.md), ADRs 0011, 0012, 0016.
- Acceptance: `lanka worker` schedules the three sources from registry cadence; killing it mid-job re-queues the job; a train with no approved versions is `no_change`; `lanka_api` cannot write (integration test); `/v2/status` reflects a forced failure within one tick and `/v2/status/events?after=` is strictly ordered; alert/heartbeat code and `ops.alerts` are gone; images build and `infra/v2/deploy.sh` performs a blue/green switch on a test host.

### Phase B — quick-win parity and enrichment enablers

**WP-04 Features, OSM, v1 port, vector tiles** — 8 d (+ 10.5 d of parity wave 1 is inside this and WP-05) — depends: WP-03
- Goal: first point/line/polygon data in v2 and the spotlight layers.
- Scope: migration `0007_features`, `lanka-ingest/src/drivers/{osm_pbf,geo_file}.rs`, `registry/connectors/osm-*.yaml`, `lanka-export/src/tiles.rs`, `lanka-server` (`/v2/features`, `/v2/nearest`, `/v2/tiles`), `lanka-extractd` (tippecanoe op only; full sandbox in WP-08).
- Inputs: v1 foundry transforms for POIs/roads/rail/water/protected areas; `docs/data/inventory.md` wave 1.
- Acceptance: hospitals, schools, parks, roads, railways, waterways, attractions, waterfalls, trails/cycling, expressways (with `status` attribute), BOI zones (OSM industrial polygons, `UNOFFICIAL_GEOM`) published; counts within 2 % of v1 for the ported layers (parity test); every feature has `unit_key` and `cell_id`; a daily OSM refresh writes only changed features; PMTiles tilesets build and are served both as files and as `/z/x/y`.

**WP-05 Grid engine** — 7 d — depends: WP-03 (parallel with WP-04)
- Goal: rasters → canonical grid arrays → zonal observations.
- Scope: `engine/crates/lanka-grid/`, `lanka-ingest/src/drivers/raster.rs`, `lanka-extractd` (gdal ops), `lanka-enrich/src/methods/grid_zonal.rs`, `lanka-server` (`/v2/point`, `/v2/radius`, `/v2/grids`, PNG tiles), `registry/connectors/{worldpop,esa-worldcover,copernicus-dem,worldclim}.yaml`.
- Inputs: [05 §8](05-storage-and-performance.md#8-grid-store), v1 `cells`/`cell_lookup`.
- Acceptance: WorldPop 2025 equals v1 cell for cell (5,468,498 cells); `gnd_idx` equals v1 `cell_lookup` (5,467,970 cells); `/v2/reverse` uses the grid path and matches the polygon path on a 100k-point sample; zonal population per GN sums to the grid total (property test); land cover, terrain and climate normals published at ADM1–ADM4; COGs pass validation; zonal pass timing recorded.

**WP-06 Enrichment core** — 6 d — depends: WP-03 (parallel)
- Goal: the recipe runner and the three methods everything else needs.
- Scope: `engine/crates/lanka-enrich/` (`method.rs`, `runner.rs`, `dag.rs`, `methods/{rollup,ratio,features_zonal}.rs`), migration `0006_enrichment`, `registry/recipes/`, `registry/schema/recipe.schema.json`, `lanka-cli` (`recipe`), provenance endpoint for derived rows.
- Inputs: [04](04-enrichment.md).
- Acceptance: census 2024 ADM1 roll-ups, densities (`/km2`), `admin.area_km2`/`admin.child_count`, facility counts per unit and per 10,000 population published through recipes; fingerprint reuse proven (second train runs nothing); `explain` returns contributors; the validator rejects an `estimated` row in an `official` indicator; status/flag propagation tests pass.

### Phase C — the multipliers

**WP-07 Declarative drivers I + expectations** — 9 d — depends: WP-02
- Goal: add a regular source without writing Rust.
- Scope: `lanka-ingest/src/drivers/{http_json,sdmx,ckan,csv,xlsx_table,html_table,git_files}.rs`, `lanka-ingest/src/expect.rs`, `registry/schema/connector.schema.json`, `lanka-cli` (`connector test`).
- Acceptance: `worldbank-wdi` runs from YAML with output identical to the Rust connector (which is then deleted); 10 further class-A sources onboarded as YAML only (IMF WEO, FAOSTAT ×2, ILOSTAT, UN SDG, WHO GHO, HDX COD-PS, WFP food prices, GeoNames postal, OurAirports); every expectation type of [03 §5.2](03-ingestion.md#52-expectations-declarative-per-dataset) has a passing and a failing test.

**WP-08 Sandbox, PDF harness, evidence sheets, review** — 9 d — depends: WP-03
- Goal: class B at throughput.
- Scope: `engine/crates/lanka-extractd/`, `infra/v2/tools.Dockerfile`, `lanka-ingest/src/drivers/pdf_table.rs`, `registry/templates/`, `lanka-export/src/evidence.rs`, `lanka-cli` (`review open|approve|reject`, `template propose`, `extract propose`), `ops.reviews`.
- Acceptance: three real tables extracted with templates (one Statistical Abstract, one LFS, one Treasury) with all template checks passing and bbox locators; evidence sheet renders page crops with cell overlays; a malformed-PDF corpus fails cleanly inside limits; the `tools` container has no network (test asserts DNS failure); class C dry run on one scanned table produces an anchored extract and requires per-cell sign-off.

**WP-09 Exports and bulk downloads** — 6 d — depends: WP-04, WP-05
- Scope: `engine/crates/lanka-export/` (Parquet, CSV, SDMX-CSV, GeoParquet, FlatGeobuf/COG via tools, `datapackage.json`, DCAT, STAC, PROV, manifest, packs, legacy `lanka.sqlite`), nginx `/dl`.
- Acceptance: a release produces the layout of [06 §6](06-serving.md#6-bulk-downloads); validators pass (Frictionless, DCAT SHACL, STAC, GeoParquet, COG); tier rules enforced (a `permission_unclear` dataset is absent from packs — test); `lanka pull --into-db` restores a release with identical dsvs; `lanka rebuild --verify` passes for three datasets.

### Phase D — population spine and first value-added products

**WP-10 Geography spine and harmonised boundaries** — 8 d engineering + 12.25 d data (parity wave 2) — depends: WP-06
- Scope: `registry/crosswalks/`, connectors for `gnd-id-crosswalk`, `admin-region-history`, `dcs-census-2012`, `dcs-census-2024-gnd`, `dcs-midyear-population-district`, `dcs-vital-statistics`; methods `build_crosswalk`, `crosswalk_rebase`; recipe `derived-harmonised-boundary-series`.
- Acceptance: 100 % of GN ids matched between 2012 and 2024 (the reference platform reaches 97.4 %); crosswalk weights sum to 1 (validator); 2012 census totals preserved to 1e-9 after re-basing; `REBASED`/`SPLIT_UNIT` flags set; parity rows `demographics`, `census-age-sex-dsd`, `population-estimates-district`, `population-change*`, `vital-statistics` marked done in `dc-parity.csv`.

**WP-11 Intercensal and zonal population, per-capita** — 9 d (value-added #2, #3, #4) — depends: WP-10, WP-05
- Scope: methods `temporal_interp`, `temporal_project`, `rake`; recipes `derived-intercensal-population`, `derived-zonal-population`, `derived-per-capita-indicators`; `docs/methods/`.
- Acceptance: QA gates of the recipe in [04 §4](04-enrichment.md#4-recipes) pass; back-cast MAPE reported on the method page; every `est.*` row has `lo`/`hi`; per-capita indicators set `DENOM_MISMATCH` correctly; the area profile groups values by class.

**WP-12 Class-A/near-A series (parity wave 3)** — 48.5 d data, in batches of ≤ 5 sources per sub-agent — depends: WP-07
- Sources: NSO generation (daily + quarter-hourly → `obs_hf`), CBSL tables (exchange rates, NEER/REER, fiscal, debt, revenue, trade, remittances, tourism earnings, bank branches), PUCSL reliability, IMMR, Treasury procurement, irrigation gauges, paddy 1979–2026.
- Acceptance per batch: YAML connector + fixtures + expectations; history backfilled; a scheduled run publishes without human action; parity rows marked done with record counts and period coverage compared.

### Phase E — interfaces

**WP-13 Query core split, REST v2 completion, OGC API** — 9 d — depends: WP-09
- Scope: `engine/crates/lanka-query/` (`Backend`, `PgBackend`), `lanka-server` (all resources of [06 §2](06-serving.md#2-rest-v2), versioned URLs, formats, `ogc.rs`), `spec/openapi/v2.json`, `engine/tests/corpus/`, benchmark suite.
- Acceptance: conformance corpus passes on PgBackend; OGC ETS Features Core passes; QGIS draws `admin-adm3` with a joined indicator; latency table of [05 §11](05-storage-and-performance.md#11-latency-targets) replaced by measurements; 304s served without a database round trip.

**WP-14 MCP server** — 5 d — depends: WP-13
- Scope: `engine/crates/lanka-mcp/`, `/mcp` route, `lanka mcp`.
- Acceptance: all tools of [06 §4](06-serving.md#4-mcp-server) callable anonymously over HTTP and stdio; every result carries `data_version`, attribution, `next`; estimate notice appears in text for an `est.*` query; rate limit enforced; a scripted agent transcript (resolve place → profile → series → provenance) passes as a test.

**WP-15 Offline pack, `geocode-csv`, `lanka pull`** — 6 d — depends: WP-13
- Scope: `lanka-query::pack` (`PackBackend`), `lanka-export` (packs), `lanka-cli`.
- Acceptance: corpus passes on PackBackend with byte-identical payloads; `geocode-csv` on a 1 M-row file equals `/v2/reverse` on a 10k sample and meets the throughput target (recorded); works with the network disabled.

**WP-16 Cross-source checks and quality pages** — 4 d — depends: WP-07, WP-12 (partly)
- Scope: `registry/checks/`, `lanka-ingest/src/checks.rs`, `/v2/datasets/{id}/quality`, incidents.
- Acceptance: the initial check set of [03 §5.3](03-ingestion.md#53-cross-source-consistency-checks) runs after each train; a seeded inconsistency opens an incident and shows on both dataset cards.

### Phase F — the rest of the value-added programme and parity

**WP-17 Dasymetric and exposure** — 12 d (value-added #5, #8, #9) — depends: WP-11
- Methods `dasymetric`, `exposure_overlay`; inputs Google Open Buildings, JRC flood, DesInventar; recipes `derived-dasymetric-disaggregation`, `derived-flood-exposure`, `derived-multi-hazard-exposure`.
- Acceptance: source totals reproduced exactly (gate); quality classes published; flood-exposure validation hit-rate vs DesInventar on the method page.

**WP-18 Routing and accessibility** — 15 d (value-added #6, #7, #11, part of #15; parity routing chain) — depends: WP-04, WP-11
- Valhalla batch profile, `osm-routing-graph`, methods `cost_distance`, `network_access`, `walkshed`; recipes `derived-travel-time-facilities`, `derived-facility-coverage`, `derived-market-access`, `derived-transit-access`; MCP `travel_reach`.
- Acceptance: island-wide travel-time grids to hospitals and schools; population within 15/30/60 min per GN with speed-profile spread; monthly schedule honoured (daily trains skip it and show `stale_inputs`).

**WP-19 Census 2024, HIES, LFS PDF tables (parity wave 4)** — 65 d data, batched per publication — depends: WP-08
- Acceptance per batch: templates + tests; evidence-sheet review completed by a named reviewer; additivity expectations pass; parity rows marked done.

**WP-20 `/v1` on the Rust server; retire Hono** — 6 d — depends: WP-13, WP-09
- Acceptance: recorded v1 response corpus passes byte-for-byte after key sorting; nginx switched; Hono container removed; `lanka.sqlite` still exported.

**WP-21 Climate, night-lights, heat** — 10 d (value-added #12, #19, #20) — depends: WP-05, WP-11.

**WP-22 Poverty, indices, attractions register** — 25 d (value-added #10, #13, #14, #16, #17, #18) — depends: WP-17, WP-18, WP-19 (HIES) — `small_area_fh` checked against R `sae` fixtures; CV-based suppression; composite indices with rank intervals.

**WP-23 Scale-out kit** — 6 d — when a trigger in [05 §13](05-storage-and-performance.md#13-scale-out-path) fires — `S3Store` + Garage runbook, replica + PgBouncer runbook, worker-box compose file.

**WP-24 Spec extraction, conformance suite, fork test** — 6 d — depends: WP-13 — `spec/1.0/`, `lanka conformance`, `country/zz-test/`, `registry-templates/global/`, `CONTRIBUTING.md`, `GOVERNANCE.md`, `REUSE.toml`.

**WP-25 Backups, restore drill, CI hardening** — 4 d — depends: WP-03 (run in parallel with Phase B) — pgBackRest + restic profiles, `restore.sh`, `restore-drill.sh`, `cargo deny`, fuzz targets, image attestation; acceptance: a scripted restore to a scratch host passes `lanka rebuild --verify` and the corpus.

### Phase G — maps, analysis, workspaces (second track)

Migration numbers for this track are assigned in merge order after `0007`; the names are fixed: `app_snapshots`, `analysis`, `accounts`, `workspace_data`, `teams_embeds`.

**WP-26 Map document spec and frames** — 5 d — depends: WP-01
- Goal: the open map spec exists and area layers have their data shape.
- Scope: `spec/map/1/{map.schema.json,ramps.json}`, `engine/crates/lanka-mapdoc/` (types, validation, limits, canonical JSON + hash, reference extraction), `lanka-core/src/query.rs` (`frame`), `lanka-server` (`GET /v2/frame`, JSON + Arrow), `lanka-cli` (`map validate`), generated TypeScript types published as `@lanka-data-layer/map-spec`.
- Inputs: [10 §2, §3.1](10-map-composition.md#2-the-map-document--ldl-map1), [ui-contract-explore.md](ui-contract-explore.md).
- Acceptance: the example document of [10 §7](10-map-composition.md#7-worked-example--flood-exposure-vs-property-prices-western-province) validates; 20 negative fixtures fail with a JSON-pointer path; unknown `x-*` keys survive a parse/serialise round trip; `/v2/frame` returns the 2024 census ADM4 population for one province with values equal to `/v2/indicators/{id}/map`; Arrow and JSON forms carry identical values; frame latency recorded in [10 §3.1](10-map-composition.md#31-frames); the generated TypeScript types compile against the fixtures of the UI contract.

**WP-27 Layer data delivery** — 7 d — depends: WP-04, WP-05, WP-26
- Goal: every layer type has a real data source.
- Scope: `lanka-grid` (overview pyramids, `ldl-v16` encoder), `lanka-server` (`/v2/grids/{id}/values/{z}/{x}/{y}.png`, `/v2/features/{ds}/points` Arrow + JSON), `lanka-export/src/tiles.rs` (`centroids` layer, `dem` raster-dem tileset), `registry/connectors/jrc-flood-hazard.yaml` (+ zonal `haz.flood_area_share`), `lanka-mapdoc` resolver (`resolve` as a library function).
- Acceptance: decoding a value tile reproduces the source cells exactly at native zoom (property test over random tiles) and within the declared quantisation error for `f32` layers; overview tiles equal the declared reducer; 250,000 points serialise under 5 MB and in < 150 ms **[A]**; JRC flood depth RP10–RP500 published as grid layers and ADM1–ADM4 shares; the resolver returns frame/values/points/tiles shapes by the size rules of [10 §3.3](10-map-composition.md#33-features); a 12-layer reference document renders in a headless-browser smoke test against a local stack.

**WP-28 App plane I: snapshots, resolve, embed** — 4 d — depends: WP-03, WP-26
- Goal: anyone can share and embed a composed map without an account.
- Scope: `engine/crates/lanka-app/` (skeleton, `Scope`, rate identity), migration `app_snapshots` (schema `app`, `app.map_snapshots`, role `lanka_app` and its grants), `lanka-server` (`/app/v1/maps/snapshots`, `/app/v1/maps/resolve`, `/app/v1/maps/{id}`, `/resolved`, `/package`), nginx `location /app/` (no cache, own rate zone), `/v2/oembed` for map URLs, `LANKA_APP_PLANE` switch.
- Acceptance: posting the same document twice returns one id; a 300 KB document is rejected; `lanka_app` cannot write `data.*`, `catalog.*`, `geo.*` and `lanka_api` cannot read `app.*` (integration tests); with the plane off, `/app/v1` is 404 and `/v2` is unaffected; the resolved document of the example carries attribution and the estimate notice; a response under `/app/` is never stored by nginx (test against the real config); snapshot rate limit enforced.

**WP-29 Analysis core and tier-1 methods** — 10 d — depends: WP-05, WP-06, WP-26
- Goal: "relate two layers" works end to end with guard rails.
- Scope: `engine/crates/lanka-stats/` (`classify`, `describe`, `correlate`, `regress`, `compare`, `rng`, effective-n adjustment), `lanka-stats-wasm`, `engine/crates/lanka-analysis/` (request/result types, resolve, planner, alignment via `rollup`/`grid_zonal`/`features_zonal`/`sample`/`point_aggregate`, guard registry, result store, sync executor), `exposure_overlay` kernel, migration `analysis`, role `lanka_analysis`, `lanka-server` (`/v2/analysis/methods`, `/v2/analyses`, `/plan`, outputs, bundle), `spec/analysis/1/`, `docs/methods/analysis-*.md`.
- Inputs: [11](11-analysis-engine.md).
- Acceptance: `relate`, `regress`, `group_compare`, `exposure`, `zonal`, `describe`, `classify` pass unit tests on textbook data and reference fixtures from R to 1e-8; the same request returns the same id and bytes twice and after an unrelated release, and a new id after an input changes; every tier-1 guard has a positive and a negative test and the vocabulary scan passes; a blocked request returns no estimates; an open-data golden test (flood area share vs population density, ADM3, island) matches a committed result; native and WASM builds of `classify`/`correlate`/`regress` agree to the last bit on the fixtures; WASM bundle size recorded (target ≤ 500 KB compressed **[A]**); timings of [11 §8](11-analysis-engine.md#8-where-the-computation-runs) measured and written back.

**WP-30 Spatial statistics and the interactive lane** — 9 d — depends: WP-29
- Scope: `lanka-stats::{spatial,spreg,trend,fdr}`, `lanka-analysis` methods `autocorr`, `local_clusters`, `hotspots`, `spatial_regress`, `change`, `composite`, `isochrone` (cost-distance), weights build job, compose service `analyst`, async path (202, polling, events), cost model and budgets.
- Acceptance: Moran, Geary, Lee, local Moran, Gi*, LM tests, spatial 2SLS and GMM agree with `spdep`/`spatialreg` fixtures; seeded permutations reproduce exactly; queen weights for ADM2–ADM4 built and their island lists recorded; an island-wide ADM4 `relate` with 999 permutations completes in the interactive lane while a long batch job is running (test with a long-running stub batch job); a job over its wall-clock or row budget is cancelled cleanly and reported; `relate` gains Lee's *L*, residual Moran and adjusted significance.

**WP-31 Accounts, workspaces, saved maps, keys, quotas, audit** — 9 d — depends: WP-28
- Scope: migration `accounts` (`app.users`, `identities`, `sessions`, `plans`, `workspaces`, `memberships`, `projects`, `api_keys`, `usage_daily`, `maps`, `map_versions`, `map_refs`, `audit_log`), `lanka-app` (OIDC sign-in, sessions, CSRF, workspace and map routes, keys, token buckets, audit chain), `lanka-cli` (`app plan set`, `app user disable`, `takedown --map`), row-level security policies.
- Acceptance: sign-in against a test OIDC provider; a two-workspace isolation test over every `/app/v1` route; saved map versions are immutable and `If-Match` conflicts are detected; freezing a map pins its analysis results (they survive eviction); a revoked or expired key is rejected and never appears in logs; quota exhaustion returns 429 with `Retry-After` while `/v2` cached reads continue; the audit chain verifies and rejects an edited row; account deletion removes personal data and the personal workspace.

**WP-32 Bring your own data** — 10 d — depends: WP-31, WP-02
- Scope: migration `workspace_data` (schema `ws`), upload endpoints, profile + mapping + match-report API, geocoding modes of [12 §5.3](12-enterprise-and-byod.md#53-geocoding-modes), `lanka-extractd` op for `ogr2ogr`, private tiles, `lanka-query` `Scope::Workspace` for series/frame/features, visibility rules, deletion, `lanka-cli` (`takedown --workspace-dataset`).
- Acceptance: a CSV with coordinates, a CSV with district/DSD names (including three deliberately ambiguous names that must land in the match report, not be guessed), an XLSX with p-codes, a GeoJSON and a zipped Shapefile each import and appear as layers and in a mixed public + private frame; a malformed-file corpus fails cleanly inside limits; columns flagged as personal data default to dropped; a private dataset is unreachable from `/v2`, from another workspace, and from an embed without a token (tests); delete removes partition, blobs, tiles and dependent results; re-uploading a new month with the stored mapping produces a new version whose diff is only the new rows.

**WP-33 Point samples, interpolation, reference analysis** — 7 d — depends: WP-29, WP-32
- Scope: `lanka-stats::interp` (IDW, variogram fitting, ordinary/universal kriging, leave-one-out), method `interpolate`, point-support `relate`/`regress` with fixed effects and distance rings, grid distance transform, the synthetic `@demo/property-asking-prices-sample` generator (documented formula, seeded), `docs/methods/analysis-point-samples.md`.
- Acceptance: kriging agrees with `gstat` fixtures; the fenced rule of [11 §3.4](11-analysis-engine.md#34-interpolation-policy-for-point-samples-amends-04-3) is enforced by tests (a count variable is refused; an unmasked price surface is refused); the Western Province example runs end to end from the document of [10 §7](10-map-composition.md#7-worked-example--flood-exposure-vs-property-prices-western-province) and its result equals a committed golden file, with `SYNTHETIC_INPUT` present; the generator's planted effect (a known price discount inside the flood zone) is recovered by `group_compare` within its interval.

**WP-34 MCP and CLI for analysis and maps** — 3 d — depends: WP-14, WP-29
- Scope: `lanka-mcp` tools of [11 §11.2](11-analysis-engine.md#112-mcp), `lanka analyze`, `lanka analysis …`, `lanka map validate|resolve`.
- Acceptance: a scripted agent transcript (search → relate → read guards → create map link) passes; every analysis tool's text block starts with the guards; `lanka analyze` against an offline pack reproduces the server's id for a tabular request.

**WP-35 Teams, projects, white-label embeds** — 6 d — depends: WP-31
- Scope: migration `teams_embeds` (`app.embed_profiles`, `embed_tokens`, `read_log`), team workspaces and roles, projects, embed tokens and per-profile `frame-ancestors`, the `postMessage` contract (`spec/map/1/embed-messages.schema.json`), `@lanka-data-layer/embed` loader, audit export, read auditing.
- Acceptance: an embed of a private map works only with a valid token and only inside an allowed origin; attribution and notices render under every profile (visual test with the credit switched off); role matrix test; audit export verifies against the chain head.

**WP-36 On-prem profile, upstream sync, workspace connectors, distribution binary** — 8 d — depends: WP-32, WP-15
- Scope: `infra/v2/profiles/`, `LANKA_ANONYMOUS=off`, `sync` job (`lanka pull --into-db` on a schedule, `--from-dir` for air-gapped sites), overlay connectors with `target.workspace`, hosted scheduled pulls with the hardened fetcher and `app.secrets`, `lanka_cli::run` + registry hooks, an example `lanka-dist-example` crate with one extra analysis method, `docs/self-hosting/private.md`.
- Acceptance: a scripted install of the `private` profile on a clean host signs in through a test IdP, syncs a release from a test upstream with identical dsvs, runs the reference analysis with the same result id as upstream, and refuses anonymous `/v2`; the example distribution binary lists its extra method in `/v2/analysis/methods`; the fork test of [07 §8](07-operations.md#8-testing-strategy) runs with the app plane on.

**WP-37 Tier-3 analysis** — 6 d — optional, when a user needs it — depends: WP-30
- `gwr`, `isochrone_network` (Valhalla on demand in the `private` profile), `zones` support (user polygons as units), `ws.grids` (user rasters).

**WP-38 Property-price data programme** — 3 d of work plus elapsed time — depends: none
- Written requests to two listing portals, CBSL and the Valuation Department for an aggregated price product (schema in [11 §10](11-analysis-engine.md#10-data-reality-property-prices)); read and record each portal's current terms; register the sources as `planned`/`blocked` with the outcome; ingest `cbsl-land-valuation-indicator` and `cbsl-condominium-prices` as time series (class B).
- Acceptance: `registry/sources/` reflects each request and reply; the registry notes replace the **[A]** markers of [11 §10](11-analysis-engine.md#10-data-reality-property-prices); no scraped data anywhere in the registry or fixtures (CI check on fixture provenance notes).

#### Front-end track against this plan

The web team builds against [ui-contract-explore.md](ui-contract-explore.md) from day one; the table says which package turns each increment from mock to real.

| Increment | Content | Mock from | Real when |
|---|---|---|---|
| FE-0 | Spike: deck.gl interleaved with the pinned MapLibre; `multiply` blend; value-tile shader on synthetic tiles | now | — |
| FE-1 | Layer panel, style editor, legend, time slider, document state + undo | now | — |
| FE-2 | Real area, raster and point layers | — | WP-26 (frames) + WP-04/05 + WP-27 |
| FE-3 | Share link, fork, embed route, image export | now (local drafts, URL fragment) | WP-28 |
| FE-4 | Relate, inside/outside, exposure, stats cards with guards | now | WP-29; clusters/hot spots WP-30 |
| FE-5 | Sign-in, saved maps and versions, keys | now | WP-31 |
| FE-6 | Upload wizard, match report, private layers | now | WP-32; price example WP-33 |
| FE-7 | Team settings, embed profiles | later | WP-35 |

Remaining parity waves 5–7 (developments + transport analyses 20.25 d; remaining P0/P1 PDF series 71.25 d; P2/P3 long tail 110.75 d) are data batches of the WP-12/WP-19 kind and are scheduled by priority once Phase E is done; they need no new engine capability.

## 4. Totals and milestones

| Milestone | Packages | Engineering days | Data days | Outcome |
|---|---|---|---|---|
| M1 Foundation rebased | WP-01…03, WP-25 | 25 | — | unattended, observable, backed up; same data as today on the new model |
| M2 Spotlight parity + enablers | WP-04…06 | 21 | (10.5 inside) | every spotlight layer, grids, roll-ups, per-capita on open data |
| M3 Multipliers | WP-07…09 | 24 | — | YAML connectors, PDF harness, bulk downloads, standards conformance |
| M4 Population spine + value-added 1–4 | WP-10, WP-11 | 17 | 12.25 | beyond the reference platform on population; comparable-over-time series |
| M5 Interfaces | WP-13…16 | 24 | — | REST complete, OGC, MCP, offline CLI |
| M6 Series + documents | WP-12, WP-19 | — | 113.5 | waves 3–4: 67 of 148 parity rows at exact parity |
| M7 Value-added programme | WP-17, 18, 21, 22 | 62 | — | accessibility, exposure, poverty, indices |
| M8 Consolidation | WP-20, WP-24 | 12 | — | Hono retired; spec published; fork test green |
| **M2a Composable maps** (after M2's WP-04/05) | WP-26…28 | 16 | — | layered maps on real data; share and embed without an account |
| **M3a Analysis** | WP-29, WP-30, WP-34 | 22 | — | relate / compare / exposure / clusters with guard rails, citable results, agent tools |
| **M4a Ownership** | WP-31…33 | 26 | — | accounts, saved maps, keys, private uploads, the flood-vs-price reference analysis |
| **M9 Teams and on-prem** | WP-35, WP-36 (+ WP-37 optional 6, WP-38 3) | 14 | — | team workspaces, white-label embeds, private instance profile |

Engineering ≈ 185 d for documents 01–09 plus ≈ 87 d for documents 10–12 (78 d without the optional WP-37 and the outreach package), less 3.5 d moved out of WP-17/WP-21; parity data to wave 4 ≈ 136 d; remaining parity ≈ 202 d. With one lead plus sub-agents in parallel on the data batches, M1–M5 is roughly four to five months **[A]**; interleaving M2a and M3a as recommended in §1 adds about seven weeks of lead time before M5 and puts composable maps in front of users at the end of M2 **[A]**.

## 5. Open questions

| # | Question | Default taken in this design | Blocks |
|---|---|---|---|
| Q1 | Relicense the engine MIT → Apache-2.0? | Yes | WP-24 (`REUSE.toml`, spec licence files) |
| Q2 | Off-site backup target: a Hetzner Storage Box (rented SFTP storage) or a host you own? Either satisfies "self-hostable"; the design only needs SSH/SFTP. | Storage Box | WP-25 |
| Q3 | Class C extraction needs a model endpoint when an operator runs `lanka extract propose`. Is calling a hosted LLM API acceptable for that operator-only step, or must it be a self-hosted model? | Pluggable; hosted allowed, never from the worker | WP-08 (class C part only) |
| Q4 | Who are the named reviewers for class B/C versions and for promoting recipes to `stable`? The two census versions approved by `bootstrap-agent` must be re-approved by a person. | Maintainer | WP-03 go-live |
| Q5 | Public base URL and takedown contact address for persistent identifiers (`/id/…`) and `LICENSE-DATA.md`. | `lanka-data-layer.prabhavalabs.com`, `takedown@` that domain | WP-13, WP-24 |
| Q6 | Publish `permission_unclear` data through the API with attribution while permission letters are pending (the inventory's stated position), or hold each publisher's data until a reply? | Publish at tier `api` | WP-12, WP-19 |
| Q7 | The no-ADM4-daily rule: daily climate at ADM2–ADM3 + monthly at ADM4. Acceptable, or is GN-level daily rainfall a product requirement? | Rule stands | WP-21 sizing (would add ~230 M rows per variable) |
| Q8 | Intraday release cuts for `live` datasets (≤ 1/h): wanted, or is one release per day enough? | Enabled | WP-03 |
| Q9 | Is a second small box acceptable before the scale-out triggers fire, to isolate heavy routing/raster jobs from the API from day one? | No — single VPS until a trigger | WP-18 |
| Q10 | Sinhala/Tamil translations of indicator labels: who reviews them? | Left empty until a reviewer exists | none |
| Q11 | Is anything charged for (hosted team plan, managed private instances, support), or is the hosted service free with operator-set limits? The design has plans as rows and no billing code either way. | Free; limits set by the operator | none (WP-31 plan values) |
| Q12 | Enabling sign-in makes the operator a controller of personal data (handle, e-mail) and a host of user content. Which legal entity is named in the privacy notice and terms, and who answers takedown and deletion requests? | Prabhava Labs; sign-in stays switched off on the public instance until both documents exist | WP-31 go-live |
| Q13 | Sign-in through GitHub and Google (OIDC) is a login-time dependency on third parties. Acceptable, or must the public instance run its own identity provider / e-mail links (adds a mail system)? | GitHub + Google | WP-31 |
| Q14 | May private uploads be hosted on the single public VPS (encrypted volume, small quotas), or should hosted private data wait for a second box, leaving BYOD to the on-prem profile at first? | Hosted, with the free-plan quotas of [12 §6](12-enterprise-and-byod.md#6-api-keys-quotas-plans) | WP-32 go-live |
| Q15 | Workspace data with `public` visibility appears on the public site as community data. Launch with it, or with `private` and `unlisted` only until there is moderation capacity? | `private` + `unlisted` only at launch | WP-32 |
| Q16 | Approach listing portals, CBSL and the Valuation Department for an aggregated price product, and who signs the requests? | Yes; the owner signs | WP-38 |
| Q17 | The `analyst` service takes the VPS to ~13.8 GB of 15 GB at peak. Accept, lower anonymous analysis limits, or take the second box of Q9 now? | Accept; `analyst` is the first service to move | WP-30 |
