# 09 — Roadmap

Work packages are sized for one implementer sub-agent each (package-scoped, contract-first, no commits — the main session reviews and commits, per the repository's sub-agent workflow). "Days" are person-days for a lead reviewing sub-agent work; data-heavy packages take their figures from [`docs/data/inventory.md`](../data/inventory.md) and [`docs/data/value-added.md`](../data/value-added.md). All estimates are **[A]**.

## 1. Ordering logic

1. **Rebase the foundation first** (WP-01…03) while only ~10,000 observations exist. Every later package writes rows; changing the row shape after wave 1 would cost a re-ingest.
2. **Then the cheapest parity and the enrichment enablers together** (WP-04…06): OSM spotlight layers, the grid, roll-ups and per-capita. These need no PDF work and no unclear licences.
3. **Then the two multipliers**: declarative drivers (WP-07) and the PDF harness (WP-08) — 85 of 147 parity rows and ~238 of 338.5 person-days are PDF tables.
4. **Value-added products are interleaved**, not left to the end: items 1–4 of the top 20 (harmonised boundaries, intercensal population, zonal population, per-capita) ship by WP-11, the open-input products (#6–#9, #11) by WP-18.
5. Public interfaces (exports, OGC, MCP, CLI) come once there is enough data to make them worth pointing at (WP-09, WP-13…15), but the REST surface grows with each package.

```
WP-01 ─► WP-02 ─► WP-03 ─┬─► WP-04 ─┬─► WP-09 ─► WP-13 ─► WP-14, WP-15 ─► WP-20
                         ├─► WP-05 ─┤
                         ├─► WP-06 ─┴─► WP-10 ─► WP-11 ─► WP-17 ─► WP-18 ─► WP-21 ─► WP-22
                         ├─► WP-07 ─► WP-12 (batches)      WP-16 (after WP-07)
                         └─► WP-08 ─► WP-19 (batches)      WP-24 (after WP-13)   WP-23, WP-25 (ops, parallel)
```

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

Engineering ≈ 185 d; parity data to wave 4 ≈ 136 d; remaining parity ≈ 202 d. With one lead plus sub-agents in parallel on the data batches, M1–M5 is roughly four to five months **[A]**.

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
