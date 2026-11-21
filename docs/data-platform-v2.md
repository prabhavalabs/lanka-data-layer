# Data platform v2 — design

Status: foundation + first vertical slice implemented (`engine/`, `infra/v2/`, `registry/schema/`).
Scope: the v2 data model, ingestion and serving engine that replaces the v1 foundry → `lanka.sqlite` → Hono path.
The v1 contract in [architecture.md](architecture.md) stays binding for `/v1` until v2 reaches parity; nothing here changes `/v1`.

Binding constraints for this design (they override the November revamp plan where the two differ):

- **Self-hosted only.** One Hetzner VPS (8 vCPU / 15 GB / 150 GB NVMe), Docker Compose, every service on `127.0.0.1`, host nginx in front. No Cloudflare products and no cache-layer assumptions: R2, Workers, D1 and CDN purging are out.
- **PostgreSQL + PostGIS is the primary store.** SQLite stays only as v1's artifact and, later, as an export format.
- **Rust for the engine and every server.** TypeScript remains for the web app.

---

## 1. Architecture

```
                                UPSTREAM
  World Bank API   statistics.gov.lk XLSX/PDF   HDX (COD-AB)   Geofabrik PBF   rasters   git repos
        │                    │                       │              │             │          │
        └────────────────────┴───────────┬───────────┴──────────────┴─────────────┴──────────┘
                                         │ HTTPS (rustls), 3 attempts, backoff
┌─────────────────────────── HETZNER VPS (8 vCPU / 15 GB / 150 GB) ───────────────────────────────────┐
│                                                                                                      │
│  lanka scheduler (container, restart=always)      lanka ingest run <source>   (CLI, same code path)  │
│     every 60 s: which sources are due? ──────────────┐                                               │
│     pg advisory lock per source                      ▼                                               │
│  ┌──────────────────────────── lanka-ingest (Rust) ─────────────────────────────────────────────┐    │
│  │ discover → fetch → archive ─────────────► /srv/lanka/raw/<sha256>   (content-addressed)      │    │
│  │                     parse (connector) → validate → stage (COPY) → gate → publish / review    │    │
│  │ every stage → ops.run_events ; failure → ops.alerts → webhook ; success → heartbeat ping     │    │
│  └───────────────────────────────────────────┬──────────────────────────────────────────────────┘    │
│                                              ▼                                                       │
│  ┌──────────────────────── PostgreSQL 18 + PostGIS 3.6 (127.0.0.1:55432) ───────────────────────┐    │
│  │ catalog.*  registry mirror      ops.*  runs, events, artifacts, alerts                       │    │
│  │ geo.*      admin_units (p-code, MultiPolygon, GiST), crosswalks, grid_layers, features       │    │
│  │ data.*     dataset_versions (immutable) · observations (LIST-partitioned by dataset)         │    │
│  │            releases → release_datasets (data_version = map dataset → version) · changelog    │    │
│  └──────────────┬────────────────────────────────────────────── NOTIFY lanka_release ───────────┘    │
│                 │                                         │                                          │
│                 ▼                                         ▼                                          │
│  lanka serve  127.0.0.1:8601  /v2/*  (axum)     grids/*.bin  memory-mapped (phase 2)                 │
│     └─ lanka-core: catalog snapshot (Arc, hot-swapped) + prepared queries ── same core ──┐           │
│  lanka mcp    127.0.0.1:8602  /mcp   (phase 2)                                           │           │
│  api (Hono)   127.0.0.1:8600  /v1/*  unchanged until parity                              │           │
│  web          static build served by nginx                                               │           │
│  host nginx :443 (certbot) — the only public listener; rate-limit zones                  │           │
└──────────────────────────────────────────────────────────────────────────────────────────┼───────────┘
                                                                                           │
                         external dead-man's switch (pinged on success) ── alerts if silent
                                                                                           │
   browsers · REST clients (OpenAPI 3.1) · MCP clients · `lanka` CLI (offline mode reads exports) ◄┘
```

One Rust workspace, one binary (`lanka`): ingest, scheduler, server, MCP and the CLI share `lanka-core`, so a number is computed by exactly one implementation regardless of how it is asked for.

---

## 2. Data model (PostgreSQL + PostGIS)

Migration: [`engine/migrations/0001_foundation.sql`](../engine/migrations/0001_foundation.sql). Four schemas.

### 2.1 `catalog` — the registry, mirrored

`registry/sources/*.yaml` and `registry/datasets/*.yaml` are the source of truth (reviewed in git, validated against `registry/schema/*.schema.json`). `lanka registry sync` upserts them in one transaction into:

| Table | Key | Notes |
|---|---|---|
| `catalog.licences` | `id` | `status ∈ open, attribution, permission_unclear, restricted`; one status per licence id (sync fails otherwise) |
| `catalog.sources` | `id` | access method/endpoint/auth, licence, cadence, release calendar, expected lag, geo levels and join type, DCAT themes, priority, status; `spec` = the YAML verbatim as JSONB, `spec_sha256` |
| `catalog.datasets` | `id` | description, method, **limits** (mandatory "what it cannot tell you"), publication class A/B/C, time coverage, enrichment |
| `catalog.dataset_sources` | (dataset, source) | many-to-many |
| `catalog.indicators` | `id` (global) | dataset, label, unit, declared dims, geo levels, period type |

Registry ids that disappear are not deleted (history must stay joinable); sources are marked `retired`.

### 2.2 `ops` — monitoring and provenance anchors

| Table | Purpose |
|---|---|
| `ops.artifacts` | Every file or API response ever fetched: `sha256` (unique, 32 bytes), source, URL, fetched_at, bytes, media type, ETag, Last-Modified, edition, archive path |
| `ops.ingestion_runs` | One row per run: trigger, status (`running, succeeded, no_change, needs_review, failed`), counts, host, engine version, error. A partial unique index allows one `running` row per source |
| `ops.run_events` | Structured log per stage (`discover … publish, alert`), level, JSON data |
| `ops.run_artifacts` | Which artifacts a run touched |
| `ops.alerts` | Raised alerts with delivery state (`delivered_at`, `error`) — undelivered alerts are retried |
| `ops.source_health` (view) | Last run, last success, consecutive failures, cadence interval per source → freshness state |

### 2.3 `geo` — places

| Table | Notes |
|---|---|
| `geo.admin_units` | **p-code is the primary key and the only join key.** `level` 0–4, trilingual names, `parent_pcode` (deferrable FK), denormalised `adm1/2/3_pcode` for one-hop roll-ups, `boundary_set`, geodesic `area_km2`, `centroid` (= `ST_PointOnSurface`, always inside), `bbox`, full-resolution `geom MultiPolygon(4326)` (repaired with `ST_MakeValid` when invalid), `geom_simplified` (0.0005°), `geom_sha256` of the source GeoJSON (cheap change detection), `artifact_id`. GiST on `geom`, trigram on `name_en` |
| `geo.boundary_sets` | Vintage of a boundary edition (`cod-ab-lka`, later `dcs-2012-gnd`, `ec-pd`, …) |
| `geo.crosswalks` | `(from_set, from_code) → (to_set, to_code, weight, method)`; methods `exact_code, name_exact, name_alias, area_overlap, pop_overlap, manual`. Name → p-code matches are written here by the connectors that make them (census 2024: 364 rows), so the matching is auditable data, not code |
| `geo.grid_layers` | Index of grid layers (dtype, scale/offset, nodata, unit, zonal rule, native resolution, file path, sha256, version) — values are **not** in Postgres, see §5 |
| `geo.features` | Points/lines/polygons that are not admin units (POIs, stations, roads); GiST, `cell_id`, `adm4_pcode`, provenance columns |

### 2.4 `data` — versions, observations, releases

**Observations** — one long, narrow table for every indicator:

```
data.observations (PARTITION BY LIST (dataset_id))
  obs_id        bigint identity           -- stable id for provenance links
  dataset_id    text                      -- partition key
  version_id    bigint                    -- immutable dataset version
  indicator_id  text
  geo_code      text                      -- p-code ('LK' national) or another coded geography
  geo_level     smallint                  -- 0..4 (9 = non-admin); looked up, never trusted from a parser
  period        text                      -- '2024' | '2024-Q2' | '2026-08' | '2026-11-20'
  period_start  date                      -- sortable
  dims          text                      -- canonical 'age_group=0-14;sex=total' (keys sorted), '' if none
  value         float8                    -- NULL when status says so
  status        enum  reported|revised|provisional|estimate|forecast|modelled|derived|not_available|nil|suppressed
  ── row-level provenance (inline, 1:1) ──
  artifact_id   bigint → ops.artifacts    -- → sha256 + archived bytes
  locator       jsonb                     -- {"table":"A5","sheet":"A5","cell":"C10","row":"Colombo District","column":"Total"}
                                          -- {"url":…,"path":"$[1][3].value","series":"SP.POP.TOTL"}
                                          -- {"page":19,"bbox":[…]} | {"formula":"100 * male / female","inputs":["D10","E10"]}
  verbatim      text                      -- the token exactly as published ("2375415", "2.5e7")
  method        enum  api|sheet_cell|csv_row|pdf_text|pdf_reviewed|llm_dual|manual|spatial|derived
  confidence    real                      -- 1.0 api/sheet_cell; formula for llm_dual (§3.5)
  review        enum  auto_accepted|pending|approved|rejected
  PRIMARY KEY (dataset_id, version_id, indicator_id, geo_code, period, dims)
```

Decisions:

- **Provenance inline, not a side table.** It is 1:1 with the observation; a join would double the hot path for nothing. Large values (locators) are small JSONB and TOAST handles outliers. Derivation parameters common to a whole version live on `dataset_versions`.
- **Enums for the three per-row categoricals** (4 bytes each instead of text).
- **`dims` as a canonical string**, not columns: dimension sets differ per indicator and one unique key covers all of them; the registry declares allowed dim keys and the validator enforces them.

**Partitioning.** `LIST (dataset_id)`, one partition per dataset, created by `data.ensure_obs_partition(dataset)` during registry sync with three per-partition indexes:

| Index | Serves |
|---|---|
| PK `(dataset_id, version_id, indicator_id, geo_code, period, dims)` | series: `WHERE dataset_id=$ AND version_id=$ AND indicator_id=$ AND geo_code = ANY($)` (measured 0.4 ms, 24 buffer hits) |
| `(geo_code, version_id, indicator_id, period_start DESC)` | area profile: everything about one place, latest per indicator |
| `(version_id, indicator_id, period, geo_level) INCLUDE (geo_code, value, dims)` | choropleth: index-only scan of one level |
| `(obs_id)` | provenance lookup |

Every query passes `dataset_id`, so the planner prunes to one partition; a grid-derived dataset with millions of ADM4 rows never bloats the indexes of a 1,000-row national series, and dropping or rebuilding a dataset is a partition operation. A `DEFAULT` partition exists only as a safety net. When a single dataset passes ~50 M rows it gets sub-partitioned by `version_id` range so old versions detach cheaply.

**Versions and `data_version`.**

- `data.dataset_versions` — immutable. `dsv = 'dsv_' + 16 hex` of the SHA-256 of the canonical row stream (rows sorted by key; value, status, verbatim, locator, method). Artifact hashes are deliberately excluded: a re-download with new bytes but identical values is *no change*. Status `staged → in_review → published → superseded` (or `rejected`, `withdrawn`). Each stores its validator report and its diff against the version it supersedes.
- `data.releases` (`data_version = YYYYMMDD.N`, same format as v1) + `data.release_datasets` — a release is an immutable map dataset → version. Publishing creates the next release by copying the previous map and re-pointing one dataset, under a transaction advisory lock. `data.changelog` stores rows added/removed/changed per release and dataset.
- **"Current" is a view**, not a copy: `data.current_release` (latest by sequence), `data.current_versions`, `data.current_observations`. Readers resolve the current version ids once (the server caches them) and query by `version_id`, so a release flip is a pointer move with zero data movement and full history stays queryable (`?as_of=<data_version>` is a free follow-up).
- An `AFTER INSERT` trigger on `data.releases` sends `NOTIFY lanka_release`; servers swap their catalog snapshot immediately (with a 5-minute safety refresh).
- Retention: superseded versions stay in Postgres for 4 releases of their dataset, then are deleted; they remain reproducible from the raw archive (artifact sha256 + parser at a commit).

### 2.5 Crosswalk and enrichment rules

- p-codes are the only admin join key. Name matching happens **once, at ingest**, through `geo.crosswalks` (+ reviewed alias lists in `registry/crosswalks/`), constrained to the parent unit, and unmatched rows are reported in run events — never silently dropped (census 2024: exactly one, "Kalmunai North Sub", population 31,556).
- Old → new boundaries: `area_overlap`/`pop_overlap` weights in `geo.crosswalks`; re-based figures carry `status='modelled'`.

---

## 3. Ingestion pipeline

### 3.1 Stages (implemented)

```
discover → fetch → archive → parse → validate → stage → gate → publish | review
```

| Stage | What happens | Where |
|---|---|---|
| discover | Connector lists candidates (URLs, or local files with their canonical URL) — cheap listing calls only | `Connector::discover` |
| fetch | Bounded concurrency (6), 3 attempts with backoff, 120 s timeout, ETag/Last-Modified captured | `pipeline::fetch_one` |
| archive | Bytes → `$LANKA_DATA_DIR/raw/ab/cd/<sha256>` atomically (temp + fsync + rename), deduplicated; `ops.artifacts` row | `archive.rs` |
| parse | Connector turns archived bytes into typed outputs (observations, admin units, crosswalks); **no network**, so re-parsing old artifacts is deterministic | `Connector::parse` |
| validate | Indicator belongs to the dataset, dims keys declared, periods parse, values finite, keys unique, every geo code exists in `geo.admin_units` (levels are looked up, not trusted); connector-specific checks (census additivity: DS rows sum to district, districts to country, three tables agree on totals) | `publish::validate`, connectors |
| stage | New `dataset_versions` row (`staged`) and **COPY** (CSV) straight into the dataset's partition — staging is the final location, invisible until a release pins it | `publish::copy_rows` |
| gate | Diff vs current version (full outer join on the natural key); class A publishes only if ≤ 1 % of historical rows changed and ≤ 1 % were removed, otherwise it is demoted to review | `publish_observations` |
| publish / review | Class A → new release. Class B/C → `in_review`; `lanka review approve <dsv> --by <name>` marks rows `approved` and releases; `reject` keeps the version archived and unpublished | `publish.rs` |

Admin units follow the same flow (content digest → no-op if unchanged → COPY to a temp table → set-based upsert that rebuilds geometry only for changed units → version + release → `ANALYZE`).

### 3.2 Connector contract

```rust
trait Connector {
    fn source_id(&self) -> &'static str;
    fn discover(&self, ctx) -> Future<Result<Vec<Candidate>>>;
    fn parse(&self, ctx, fetched: &[Fetched]) -> Future<Result<Vec<Output>>>;   // no network
}
enum Output { Observations { dataset_id, vintage, rows }, AdminUnits { … }, Crosswalks { … } }
```

Connectors never write to the database; the pipeline does, so every source gets identical provenance, monitoring and publication rules. Connectors are Rust (decision: the user override puts the engine in Rust; XLSX via `calamine`, JSON via `serde`, PDF text layer via `pdftotext -bbox-layout` subprocess in phase 3). The v1 TypeScript parsers are ported, not called.

### 3.3 Publication classes

| Class | Gate | Human step |
|---|---|---|
| A — auto | validators + revision gate (≤ 1 % changed, ≤ 1 % removed) | none; digest after the fact |
| B — reviewed deterministic parse (XLSX/PDF tables) | validators; always lands `in_review` | `lanka review approve` after reading the evidence (diff, validator report, locators). Evidence-sheet HTML is a follow-up |
| C — LLM-assisted | dual extraction + text-layer anchoring; confidence = 1.0 both agree & anchored, 0.8 anchored single reading, 0.5 disagree, 0 not anchored (rejected) | per-cell sign-off for every value < 1.0 plus a 10 % sample; `method='llm_dual'`, `reviewed_by/at` mandatory |

### 3.4 Scheduling — decision: a Rust scheduler in a container

`lanka scheduler` runs as a Compose service (`restart: unless-stopped`) and every 60 s computes due sources **from the database** (`ops.source_health` + registry cadence + release calendar), then runs them through the same `run_source` path as the CLI, at most 2 in parallel, each under its per-source advisory lock.

Why not systemd timers: due-ness is data (cadence, last success, upstream release calendars, retries after failure) that already lives in Postgres; a stateless loop over that state handles missed runs, reboots and retries without unit files per source, and keeps one deployment unit. Crash-safety comes from the design rather than the supervisor: runs hold a session advisory lock, so a `running` row whose lock is free is provably abandoned and is closed as failed by the next start (implemented). Rejected: cron in a container (silent death — dc's failure mode), Airflow/Dagster (a second product for ~170 sources), GitHub Actions cron as primary (best-effort, disabled on inactive repos).

### 3.5 Freshness vs upstream release calendars

- State per source: `fresh`, `due` (cadence elapsed since last success), `overdue` (2× cadence), `failing` (≥ 3 consecutive failed runs), `never_run` (implemented in `query::freshness_state`; exposed on `/v2/datasets/{id}` and `lanka ingest status`). Next: `new_upstream` (discover found a candidate not yet published), `in_review`, `retired`.
- Change-based first: `discover` hashes the candidate set (date-stamped CBSL workbook URLs, WB `lastupdated`, HDX resource `last_modified`); a new candidate triggers a run. Age thresholds are the backstop.
- `release_calendar_url` + `expected_lag_days` come from the registry; once a year the expected dates are entered in `ops.release_calendar` (follow-up table). No `new_upstream` within 3 days of an expected date raises an `overdue_release` alert — that usually means a URL pattern changed.

### 3.6 Alerting — fixing dc's silent failure

Three independent detectors:

1. **Per run (implemented):** any error closes the run as `failed`, writes `ops.alerts`, and POSTs JSON to `LANKA_ALERT_WEBHOOK` (works with ntfy, Matrix, Slack, Discord hooks). Delivery failure is recorded on the alert and retried by the scheduler. Validator failures and gate demotions are visible as run status `needs_review`.
2. **Dead-man's switch (implemented as hook):** every successful run GETs `LANKA_HEARTBEAT_URL`; the scheduler also pings every tick. The check service must live **off** the VPS to detect VPS death — this is the one deliberate external dependency (open decision D-B below).
3. **Outside-in watchdog (follow-up):** an hourly GitHub Actions job fetches `/v2/status` and fails if any source is `failing`, the scheduler's last tick is > 15 min old, or a class-A source is `overdue`.

### 3.7 Raw archive

Local NVMe, content-addressed, write-once (`data/v2/raw` in dev, `/srv/lanka/raw` in prod). Nightly `restic` backup of the archive plus `pg_dump -Fc` to a Hetzner Storage Box (self-hosted principle: storage we rent, not a CDN product). Archive objects of `permission_unclear`/`restricted` sources are never exposed publicly; only their sha256 is.

---

## 4. Enrichment and aggregation

Everything below is SQL or Rust over p-codes — never names.

1. **Point → place.** `ST_Intersects` against full-resolution GN polygons (GiST) with a ≤ 2 km KNN snap for near-shore points (implemented in `query::reverse`; Point Pedro snaps 704 m). Bulk geocoding uses the grid (`cell_id` → `gnd_idx` array) instead of polygons.
2. **Point sets → indicators.** Features get `cell_id` and `adm4_pcode` at load; counts/densities per unit are a `GROUP BY adm4_pcode` then roll-up.
3. **Rasters → grid → indicators.** GDAL warp to the canonical 0.001° grid (build container), stored as arrays (§5); zonal pass over `gnd_idx` yields ADM4 values in one sweep (sum / mean / pop-weighted mean / class share), written as `method='spatial'` observations with `status='modelled'` where a GN covers < 10 cells.
4. **Roll-up ADM4 → ADM3 → ADM2 → ADM1 → ADM0** via `adm1/2/3_pcode` columns in one `GROUP BY`: counts sum; rates are recomputed from numerator and denominator indicators (never averaged); provenance `method='derived'`, `locator = {formula, inputs: [dsv…]}`. Census 2024 has no province rows upstream — the province (ADM1) values are the first derived roll-up to add.
5. **Derived indicators** (ratios, densities, change, shares) are declared in the registry (`enrichment.aggregations`), computed in the same run, and carry their formula and input cells/versions in the locator (implemented for `census24.sex_ratio`: `{"formula": "100 * male / female", "inputs": ["D10", "E10"]}`).

---

## 5. Grid layers — decision: memory-mapped arrays + COG, not Postgres

11,340,000 cells × N layers × periods does not belong in rows (one WorldPop layer would be 5.5 M rows ≈ 400 MB with indexes; PostGIS raster is slower than a flat array for point and zonal reads and bloats backups).

- Each layer/period is a raw little-endian array (`u8/u16/i16/u32/f32` + scale/offset/nodata), 11.34 M cells (u16 = 22.7 MB), memory-mapped read-only by `lanka serve`: O(1) point reads by `cell_id`, radius sums as row-range scans, a full zonal pass in tens of milliseconds.
- A grid-aligned Cloud-Optimised GeoTIFF of the same layer is produced for download.
- `cell_lookup` becomes parallel arrays (`gnd_idx u16`, `postal_idx u16`, …).
- Postgres holds only the index (`geo.grid_layers`) and the zonal **results** as observations, so every grid layer is queryable by p-code like any other indicator.

---

## 6. Interfaces — one query core

`lanka-core::query` holds every read. Transports are thin.

### 6.1 REST v2 (implemented: `lanka serve`, axum)

| Endpoint | Notes |
|---|---|
| `GET /v2/health` | DB + PostGIS version, data_version, admin unit count |
| `GET /v2/catalog?q=&theme=` | datasets with licence statuses, published dsv |
| `GET /v2/datasets/{id}` | card: description, limits, sources, licences, indicators, current version, freshness per source |
| `GET /v2/areas/{pcode}` | hierarchy, children, area, interior point, bbox, latest value of every indicator held |
| `GET /v2/indicators/{id}/series?geo=LK,LK11&from=&to=&dims=` | ≤ 25 geographies |
| `GET /v2/indicators/{id}/map?level=&period=&parent=&dims=` | columnar `{pcodes[], values[], breaks[]}` (quantiles), no geometry |
| `GET /v2/observations/{id}/provenance` | artifact sha256, URL, fetched_at, locator, verbatim, method, review record |
| `GET /v2/reverse?lat=&lon=` | deepest unit + chain + grid cell; `match: contains|nearest` |
| `GET /v2/openapi.json` | OpenAPI 3.1 from `utoipa` annotations |

Envelope `{success, message, payload, meta}`; `meta = {data_version, contract_version: 2, dataset_versions[{dataset, dsv, vintage}], source[], licences[{id, status, attribution}], next[{rel, href}]}`. ETag = `"<data_version>-<hash(path, query, dsv set)>"`, `Cache-Control: public, max-age=300, stale-while-revalidate=86400`, `If-None-Match` → 304 (nginx `proxy_cache` in front is enough — no CDN). Errors: 400 validation, 404 `not_found`/`not_in_coverage`, 500 masked. API pool: 32 connections, `statement_timeout = 5s`, prepared statement cache; measured locally 2–15 ms per request (area, reverse, map).

### 6.2 MCP (next)

Anonymous, read-only, streamable HTTP at `/mcp` (+ `lanka mcp` stdio) on the same core. Tools as in the revamp plan (`search_catalog`, `describe_dataset`, `resolve_place`, `get_area_profile`, `query_indicator`, `compare_areas`, `analyze_point`, `find_features`, `get_provenance`, later `travel_reach`); every result carries `provenance[]`, `licences[]`, `data_version`, `next[]`.

### 6.3 CLI (implemented subset)

```
lanka db migrate
lanka registry validate | sync
lanka ingest run <source> [--from DIR]      lanka ingest status [--json]
lanka review list | approve <dsv> --by NAME | reject <dsv> --by NAME --note TEXT
lanka serve [--bind 127.0.0.1:8601]
lanka reverse <lat> <lon> [--json]          lanka area <pcode>
```

---

## 7. Deployment on the VPS

- `infra/v2/docker-compose.yml`: `db` = PostGIS (`imresamu/postgis:18-3.6.1-trixie`, the multi-arch build of the official recipe; `postgis/postgis` tags are amd64-only — either works on the amd64 VPS), named volume, `127.0.0.1:55432`, healthcheck, `shm_size: 1g`, 7 GB memory cap. Next services: `engine` (`lanka serve`, `127.0.0.1:8601`) and `scheduler` (`lanka scheduler`), both from one distroless image built from `engine/`.
- `postgresql.conf` tuned for the 15 GB host shared with the server and builds: `shared_buffers 3GB`, `effective_cache_size 9GB`, `work_mem 32MB`, `maintenance_work_mem 1GB`, `random_page_cost 1.1`, `effective_io_concurrency 200`, `io_method worker` (PG18 AIO), `max_wal_size 8GB`, `wal_compression zstd`, `jit off`, partition-wise join/aggregate on, `pg_stat_statements`, slow-query log at 250 ms.
- nginx: `/v2/` → `127.0.0.1:8601` with the existing rate-limit zones and a small `proxy_cache` keyed on URI honouring ETag; `/v1/` unchanged.
- Roles (follow-up migration): `lanka_owner` (migrations), `lanka_ingest` (writes), `lanka_api` (SELECT only on `catalog`, `geo`, `data`, `ops.source_health`) — enforces "the API never writes" in the database itself.
- Backups: nightly `pg_dump -Fc` + WAL archiving (`wal_level=replica` already) and `restic` of the raw archive to a Hetzner Storage Box; monthly restore test.

---

## 8. What exists now (first vertical slice)

| Piece | State |
|---|---|
| Schema | migration 0001: all tables above, partition function, views, NOTIFY trigger |
| Registry | JSON Schemas (`registry/schema/`), serde loader with full error lists, cross-reference warnings, transactional sync (167 sources, 244 datasets, 475 indicators sync cleanly) |
| `hdx-cod-ab-lka` → `cod-ab-lka-boundaries` | 14,417 units ADM0–ADM4 with full geometry, ~60 s cold, ~10 s no-op reload |
| `worldbank-wdi` → `wdi-lka` | 27 series (5 named indicators + 22 under `wdi.series` with `series_code` dim), 1,203 observations, class A auto-published, ~2 s |
| `dcs-census-2024` → `dcs-census-2024-population`, `dcs-census-2024-ethnicity-religion` | live XLSX A5–A7 (sha256 identical to the v1 cache), 8,395 sheet-cell observations + 365 derived sex ratios, 364 crosswalk rows; class B → reviewed; values identical to v1 `lanka.sqlite` (8,395/8,395) |
| Server | 8 routes + OpenAPI, ETag/304, snapshot hot-swap on NOTIFY |
| Tests | grid vectors (mirrors `shared/src/grid.test.ts`), p-codes, periods, envelope, registry loader (fixtures + the repository registry), archive, digests, CSV quoting, WB and census parsers, router tests, and a gated end-to-end Postgres test (throwaway DB: sync, runs, locks, admin load, publish, no-change, gate demotion, approve, changelog, all queries, failure + alert) |

---

## 9. Roadmap (follow-ups, in order)

1. **Scheduler + status:** `lanka scheduler` loop, `ops.release_calendar`, `/v2/status` and a public status page, alert retry, GitHub watchdog workflow.
2. **Roles and container image:** `lanka_api` read-only role, distroless image, compose services `engine` + `scheduler`, nginx `/v2/` route on the VPS.
3. **Roll-ups and derived indicators** from `enrichment.aggregations` (census ADM3 → ADM1; densities from `admin.area_km2`), and `admin.area_km2` / `admin.child_count` observations for `cod-ab-lka-boundaries`.
4. **Direct HDX COD-AB connector** (CKAN `package_show` → zip → GeoJSON) replacing the `--from` read of v1 layers; GN-level census via `nuuuwan-lk-census-2024` + `gnd-id-crosswalk`.
5. **Grid engine:** `lanka-grid` (GDAL warp in a build container), arrays + COG, zonal → observations; WorldPop parity with v1 cell for cell; `/v2/point`, `/v2/grid`.
6. **MCP** on `lanka-core` (HTTP + stdio).
7. **Evidence sheets** for class B/C review (HTML per version: value, verbatim, locator, previous value, delta flag) and PDF connectors with bbox locators; class C dual extraction with a spend cap.
8. **Exports:** per-release Parquet/CSV/GeoParquet/SQLite bundles (open vs full by licence status) served by nginx from local disk; `lanka pull` and offline CLI mode.
9. **`/v1` parity** on the Rust server against a recorded response corpus; then retire Hono.
10. Shared grid test-vector file (`shared/grid-vectors.json`) read by TS and Rust tests so the four copies of the formula share one source.

## 10. Open decisions

| # | Decision | Recommendation |
|---|---|---|
| D-A | WDI coverage: the registry declares 5 named WDI indicators plus a generic `wdi.series` (dim `series_code`). Promote the 27 core series to named indicators (clean URLs, labels, units), keep the generic one for the long tail? | Yes — edit `registry/datasets/wdi-lka.yaml` (owned by the registry stream) |
| D-B | Dead-man's switch host: it must run outside the VPS to catch VPS death | Self-hosted Healthchecks on a second tiny box, or the hosted healthchecks.io free tier |
| D-C | Alert channel for `LANKA_ALERT_WEBHOOK` | ntfy (self-hostable) + email |
| D-D | Who signs class B reviews; the two census versions in the local DB were approved by `bootstrap-agent` after an automated 8,395/8,395 comparison with v1 and must be re-approved by a person on the VPS | Maintainer approves via `lanka review approve` |
| D-E | Off-site backup target | Hetzner Storage Box (restic + pg_dump) |
