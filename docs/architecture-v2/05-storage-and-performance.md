# 05 — Storage and performance

Capacity and latency numbers in this document are estimates **[A]** unless a measurement is cited; WP-01 and WP-13 each include a benchmark task that replaces them with measured values in this file.

## 1. Schema evolution from `0001_foundation.sql`

Migrations are forward-only SQL files embedded by `sqlx::migrate!` (kept). Planned sequence:

| Migration | Contents |
|---|---|
| `0002_standard_model.sql` | `geo.schemes`, `geo.units`, `geo.unit_geoms`, `geo.unit_xrefs`, `geo.entities`; view `geo.admin_units`; `catalog.units_of_measure`, `codelists`, `codes`, `dim_sets`, `publishers`; new columns on `catalog.indicators`, `catalog.datasets` (`key`, `kind`, `recipe_id`, `live`, `boundary_set`), `catalog.licences`; new `data.observations` (+ `obs_provenance`, `obs_hf`), data copy from the old table, drop old table and enums `data.obs_status`; `data.dataset_versions` (`seq`, `status` gains `approved`, `boundary_set`, `multiset_hash`) |
| `0003_ops_jobs.sql` | `ops.jobs`, `ops.extracts`, `ops.extract_artifacts`, `ops.reviews`, `ops.review_cells`, `ops.check_results`, `ops.incidents`, `ops.release_calendar`, `ops.takedowns`; drop `ops.alerts`; `data.dataset_version_extracts` |
| `0004_roles.sql` | roles and grants (§7) |
| `0005_serve.sql` | schemas `serve`, `stage`; `serve.area_latest`, `serve.indicator_coverage`, `serve.search`, `serve.release_state` |
| `0006_enrichment.sql` | `catalog.methods`, `catalog.recipes`, `catalog.recipe_inputs`, `data.derivations`, `data.version_inputs` |
| `0007_features.sql` | `geo.features` reshaped (range-versioned, partitioned by dataset), `geo.tilesets` |

`0002` is the only destructive one. It runs against ~10,000 observation rows and 14,417 units, in one transaction, and its acceptance test compares every `(indicator, geo, period, dims, value)` before and after.

## 2. Observations

```sql
CREATE TABLE data.observations (
  obs_id        int8   NOT NULL,                      -- dataset_key << 40 | per-dataset sequence
  value         float8,
  lo            float8,
  hi            float8,
  dataset_key   int4   NOT NULL,                      -- partition key
  indicator_key int4   NOT NULL,
  geo_key       int4   NOT NULL,
  dims_key      int4   NOT NULL DEFAULT 0,
  vfrom         int4   NOT NULL,                      -- first dataset-version seq in which the row is valid
  vto           int4   NOT NULL DEFAULT 2147483647,   -- last seq in which it is valid (MAX = still open)
  flags         int4   NOT NULL DEFAULT 0,
  time_start    date   NOT NULL,
  geo_level     int2   NOT NULL,
  basis         int2   NOT NULL,                      -- 1 reported 2 computed 3 estimated 4 forecast
  status        int2   NOT NULL,                      -- 1 normal 2 provisional 3 revised 4 break 5 low_reliability 6 missing 7 nil 8 suppressed
  freq          "char" NOT NULL,                      -- A F H S Q M W D P
  PRIMARY KEY (dataset_key, indicator_key, geo_key, time_start, freq, dims_key, vfrom),
  CHECK (vfrom <= vto)
) PARTITION BY LIST (dataset_key);
```

Column order is by alignment (8-byte, 4-byte, 2-byte, 1-byte) so there is no padding: 24-byte tuple header + 8 + 8·(value, lo, hi present or in the null bitmap) + 5·4 + 4 + 4 + 3·2 + 1 ≈ **72–88 bytes per row** against ~220+ bytes for the 0001 layout (text keys, inline JSONB locator, verbatim text) **[A]**.

Small codes are `int2` with a lookup in `lanka-core` (and a `data.code_labels` table for humans) rather than Postgres enums: 2 bytes instead of 4, and adding a code is not a catalog-locking `ALTER TYPE`.

### 2.1 Why version ranges

The foundation stores a full copy of a dataset per version (`version_id` in the key) and keeps four versions. That is simple but (a) rewrites every row of a 7 M-row ADM4 series when one month is appended, (b) cannot hold daily datasets, and (c) loses `as_of` history after four versions.

With `(vfrom, vto)`:

- **stage** version *n*: insert only rows that are new or changed (`vfrom = n`); unchanged rows are untouched. Staged rows are invisible to every released version because no release pins seq *n* yet.
- **release** of version *n*: `UPDATE … SET vto = n-1` for rows of the previous version that were changed or removed. This is the only UPDATE the table ever sees, and a trigger rejects any UPDATE that touches another column.
- **reject** a staged version: `DELETE WHERE vfrom = n` (nothing was closed).
- **read current**: `WHERE vto = 2147483647` (partial indexes); **read as of** a release that pinned seq *k*: `WHERE vfrom <= k AND vto >= k`.
- One open (staged / in review / approved-not-released) version per dataset at a time; a second staging supersedes the first (its rows are deleted) — enforced by a partial unique index on `data.dataset_versions (dataset_id) WHERE status IN ('staged','in_review','approved')`.

The staging diff is one set-based statement per dataset: COPY into `stage.obs_<job>` (unlogged, no indexes) → `INSERT … SELECT` of rows whose `(value, lo, hi, basis, status, flags)` differ from the open row with the same key (anti-join / `IS DISTINCT FROM`) → record the rows to close in `data.version_closes (version_id, obs_id)` (a logged table, so it survives a crash), applied at release and then deleted.

### 2.2 Partitioning

- `LIST (dataset_key)`, one partition per dataset, created by `data.ensure_obs_partition(dataset_key)` at registry sync (kept from 0001, re-keyed). ~260 partitions today, a few hundred later; every data query carries `dataset_key` (resolved from the indicator in the catalogue snapshot), so pruning is at plan time.
- A partition expected to exceed **50 M rows** is created sub-partitioned by `RANGE (time_start)` (decade for monthly series, year for daily). The registry declares it (`storage: {subpartition: year}`); it is not retrofitted automatically.
- Hard policy to keep row counts sane: **no ADM4 × daily observations**. Daily climate/hazard series are published at ADM2–ADM3 and as grid layers; ADM4 gets monthly aggregates. The registry validator rejects an indicator with `geo_levels ∋ adm4` and `period_type = daily` unless `storage.allow_adm4_daily: true`.
- `data.observations_default` stays empty; a nightly check opens an incident if it is not.

### 2.3 Indexes (per partition, created by `ensure_obs_partition`)

| Index | Definition | Serves |
|---|---|---|
| PK | `(dataset_key, indicator_key, geo_key, time_start, freq, dims_key, vfrom)` | uniqueness, `as_of` reads, history |
| `_series` | `(indicator_key, geo_key, dims_key, time_start) INCLUDE (value, lo, hi, basis, status, flags, freq, obs_id) WHERE vto = 2147483647` | series for ≤ 25 geographies: index-only |
| `_map` | `(indicator_key, time_start, freq, geo_level, dims_key) INCLUDE (geo_key, value, lo, hi, basis, status, flags) WHERE vto = 2147483647` | choropleth: one index-only range scan per level |
| `_obs` | `BRIN (obs_id) WITH (pages_per_range = 16)` | provenance lookup by id; obs_ids are assigned in insert order, so BRIN is tiny and selective |
| `_time` (sub-partitioned or append-heavy partitions only) | `BRIN (time_start)` | time-window scans and retention |

Area profiles no longer touch `data.observations` at all (§5). There is no index on `geo_key` alone: "everything about one place" is served by `serve.area_latest`, and a per-partition geo index would be the largest index on every partition for one query shape.

After bulk loads: `ANALYZE` the partition. Partitions use `fillfactor = 100`: the only UPDATE is the `vto` close at release, and because `vto` appears in the partial-index predicates that update is not HOT. This is accepted — closes are batched once per release, touch only changed rows, and autovacuum is tuned for it (§6).

### 2.4 Provenance side table

```sql
CREATE TABLE data.obs_provenance (
  obs_id      int8 NOT NULL,
  dataset_key int4 NOT NULL,
  artifact_id int8 NOT NULL,
  extract_id  int8,
  extract_row int4,
  locator     jsonb NOT NULL,
  verbatim    text,
  method      int2 NOT NULL,       -- api, sheet_cell, csv_row, pdf_text, pdf_reviewed, ocr, llm_dual, manual, feature
  confidence  real,
  review      int2 NOT NULL,       -- auto_accepted, pending, approved, rejected
  PRIMARY KEY (dataset_key, obs_id)
) PARTITION BY LIST (dataset_key);
```

Only extracted rows have a provenance row. Derived rows (`basis = computed|estimated|forecast` produced by a recipe) have none — their lineage is `data.derivations` + `Method::explain`. With an expected ~10–15 M extracted rows against ~100 M+ derived rows, this keeps the hot table narrow and stops the JSONB locator of a formula being stored a hundred million times.

### 2.5 High-frequency series

```sql
CREATE TABLE data.obs_hf (
  ts            timestamptz NOT NULL,
  value         float8,
  indicator_key int4 NOT NULL,
  geo_key       int4 NOT NULL,            -- stations are units of scheme 'station'
  dims_key      int4 NOT NULL DEFAULT 0,
  status        int2 NOT NULL DEFAULT 1,
  artifact_id   int8 NOT NULL,
  ingested_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (indicator_key, geo_key, dims_key, ts)
) PARTITION BY RANGE (ts);               -- monthly partitions, created a month ahead; BRIN(ts)
```

Append-only with upsert on correction (the superseded value goes to `data.obs_hf_revisions`). Not release-pinned. A recipe (`ratio`/aggregation) rolls it up into daily/monthly observations, which are. Sources: NSO quarter-hourly generation, river and reservoir gauges, station weather, air quality.

## 3. Versions and the dsv hash

```sql
ALTER TABLE data.dataset_versions
  ADD COLUMN seq            int4 NOT NULL,           -- 1,2,3… per dataset; UNIQUE (dataset_id, seq)
  ADD COLUMN multiset_hash  bytea NOT NULL,          -- 32 bytes
  ADD COLUMN schema_hash    bytea NOT NULL,          -- indicators + dims + units at stage time
  ADD COLUMN boundary_set   text REFERENCES geo.boundary_sets (id),
  ADD COLUMN git_rev        text NOT NULL,
  ADD COLUMN reproducible   boolean NOT NULL DEFAULT true;
-- status: staged → in_review → approved → published → superseded | rejected | withdrawn
```

`dsv = 'dsv_' ‖ hex(sha256(schema_hash ‖ multiset_hash ‖ row_count))[0:16]`, where

```
multiset_hash = Σ sha256(canonical_row_bytes)   (mod 2^256, as a 256-bit little-endian integer)
canonical_row = indicator_id ‖ 0x1f ‖ geo_code ‖ 0x1f ‖ period ‖ 0x1f ‖ dims ‖ 0x1f ‖ ryu(value) ‖ 0x1f ‖ ryu(lo) ‖ 0x1f ‖ ryu(hi)
                ‖ 0x1f ‖ basis ‖ 0x1f ‖ status ‖ 0x1f ‖ flags ‖ 0x1f ‖ verbatim ‖ 0x1f ‖ canonical_json(locator)
```

Properties: order-independent (no sort of millions of rows); **incremental** — version *n*'s hash is version *n-1*'s plus the added rows minus the removed ones, so appending one day to a ten-year daily series hashes one day; path-independent — the same content reached by different histories yields the same dsv; and still a function of content only (uses string ids, never surrogate keys, so a re-imported database produces identical dsvs). Additive set hashing is not collision-resistant against an adversary choosing rows; it is used here for change detection and integrity of our own pipeline output, and the per-file sha256 in the release manifest remains the cryptographic integrity check for downloads.

The four existing dsvs change once (migration 0002 recomputes them and keeps the old value in `data.dataset_versions.legacy_dsv`).

## 4. Features and boundaries

- `geo.features` is rebuilt like observations: `LIST (dataset_key)` partitions, `(vfrom, vto)` ranges, `feature_key text` (the upstream key), `kind`, trilingual names, `geom geometry(Geometry, 4326)`, `cell_id int4`, `unit_key int4` (deepest admin unit, by point-on-surface for lines/polygons), `attrs jsonb`, `flags int4`, `entity_id text`. Indexes: GiST on `geom` `WHERE vto = MAX`; `(kind, unit_key) WHERE vto = MAX`; trigram on `name_en` for searchable datasets only (declared in the registry).
- OSM daily churn: only changed features produce new rows (same range mechanism), so a daily Geofabrik refresh of ~1–2 M Sri Lanka features we keep **[A]** writes thousands of rows, not millions.
- Unit geometry loads keep the implemented flow (COPY to temp → set-based upsert → rebuild only changed geometry) and write `geo.unit_geoms` per boundary set.
- Point → unit assignment in bulk uses the grid (`cell_id` → `gnd_idx` array) and falls back to `ST_Intersects` + ≤ 2 km KNN only for cells straddling a boundary or outside land (implemented logic in `query::reverse`, moved to `lanka-grid::assign`).

## 5. Serving projections (`serve.*`)

```sql
CREATE TABLE serve.area_latest (
  geo_key int4 NOT NULL, indicator_key int4 NOT NULL, dims_key int4 NOT NULL,
  time_start date NOT NULL, freq "char" NOT NULL,
  value float8, lo float8, hi float8, basis int2 NOT NULL, status int2 NOT NULL, flags int4 NOT NULL,
  obs_id int8 NOT NULL, dataset_key int4 NOT NULL,
  PRIMARY KEY (geo_key, indicator_key, dims_key)
);
CREATE INDEX area_latest_ds ON serve.area_latest (dataset_key);     -- delete-by-dataset at release

CREATE TABLE serve.indicator_coverage (
  indicator_key int4 NOT NULL, geo_level int2 NOT NULL, dims_key int4 NOT NULL,
  periods int4 NOT NULL, first_start date, last_start date, units int4 NOT NULL,
  vmin float8, vmax float8, breaks float8[],          -- precomputed quantile breaks for the latest period
  PRIMARY KEY (indicator_key, geo_level, dims_key)
);

CREATE TABLE serve.search (
  kind text NOT NULL,            -- unit | place | postal | dataset | indicator | feature
  ref  text NOT NULL,            -- code / id
  label_en text NOT NULL, label_si text, label_ta text, sublabel text,
  rank real NOT NULL,            -- log(population) boost etc. (v1 ranking constants)
  lat float8, lon float8,
  PRIMARY KEY (kind, ref)
);
CREATE INDEX search_trgm_en ON serve.search USING gin (label_en gin_trgm_ops);
CREATE INDEX search_trgm_si ON serve.search USING gin (label_si gin_trgm_ops);
CREATE INDEX search_trgm_ta ON serve.search USING gin (label_ta gin_trgm_ops);
```

These are **plain tables maintained by the release job** (delete rows of changed datasets, insert from the open rows), not materialised views: a materialised view can only be refreshed whole, and `REFRESH … CONCURRENTLY` over ~100 M source rows every day is the wrong cost. They are disposable — `lanka serve rebuild` regenerates all of them from L2–L4.

Materialised views are used for exactly one thing: `ops.source_health_mv` (freshness state per source, refreshed by the scheduler tick), because it is small and whole-refresh is the natural operation.

## 6. Loading and maintenance

- **COPY everywhere.** Binary `COPY` into `stage.*` unlogged tables (no WAL, no indexes), then one `INSERT … SELECT` into the partition. Target ≥ 300k rows/s for staging and ≥ 100k rows/s for the indexed insert on this hardware **[A]**; a 7 M-row first load of an ADM4 series in ~1–2 minutes.
- Large first loads (> 5 M rows into an empty partition): create the partition without secondary indexes, load, build indexes with `maintenance_work_mem = 1GB` and 3 parallel workers, `ANALYZE`, then attach. `ensure_obs_partition` gets a `deferred_indexes` flag for this.
- Per-table autovacuum settings on observation partitions: `autovacuum_vacuum_scale_factor = 0.02`, `autovacuum_vacuum_insert_scale_factor = 0.1`, `autovacuum_freeze_min_age` default; the release job runs `VACUUM (ANALYZE)` on partitions where it closed more than 5 % of rows.
- `postgresql.conf` (kept from `infra/v2/postgres/postgresql.conf`, with changes): `max_connections 60` (was 80), `shared_buffers 3GB` (kept), `effective_cache_size 8GB`, `work_mem 32MB` (kept; recipe sessions `SET LOCAL work_mem = '256MB'`), `max_wal_size 8GB` (kept), `wal_level replica` + `archive_mode on` (new, for pgBackRest), `default_toast_compression lz4` (new), `track_io_timing on` (new), `idle_in_transaction_session_timeout 5min` (new), `statement_timeout` set per role (§7), partition-wise join/aggregate on (kept), `jit off` (kept).

## 7. Roles, pooling, timeouts

```sql
CREATE ROLE lanka_owner  NOLOGIN;                        -- owns objects; used only by `lanka db migrate`
CREATE ROLE lanka_ingest LOGIN;                          -- worker
CREATE ROLE lanka_api    LOGIN;                          -- server (read-only)
CREATE ROLE lanka_recipe LOGIN;                          -- recipe computations (read data, write scratch + stage)

GRANT USAGE ON SCHEMA catalog, geo, data, serve TO lanka_api;
GRANT SELECT ON ALL TABLES IN SCHEMA catalog, geo, data, serve TO lanka_api;
GRANT SELECT ON ops.source_health_mv, ops.ingestion_runs, ops.run_events, ops.incidents, ops.jobs TO lanka_api;   -- status endpoints
ALTER ROLE lanka_api    SET default_transaction_read_only = on;
ALTER ROLE lanka_api    SET statement_timeout = '5s';
ALTER ROLE lanka_api    SET idle_in_transaction_session_timeout = '10s';
ALTER ROLE lanka_recipe SET statement_timeout = '30min';
ALTER ROLE lanka_ingest SET statement_timeout = '15min';
REVOKE UPDATE, DELETE ON data.releases, data.release_datasets FROM lanka_ingest;   -- releases are append-only
```

"The API never writes" is enforced by the database. `lanka_recipe` cannot write `data.*` directly; recipe output goes through the same stage → validate → version path as ingestion.

Pooling: `sqlx` pools in process — `lanka serve` 24 connections, `lanka worker` 8 (+ 4 `lanka_recipe`), admin headroom. **No PgBouncer** on one host with two client processes; it is added (transaction mode, in front of the replica) when a second API instance exists.

Prepared statements: every hot query is a `sqlx` prepared statement with integer parameters; partition pruning happens at plan time because `dataset_key` is always a bound constant, with `plan_cache_mode = force_custom_plan` for `lanka_api` so pruning is never lost to a generic plan.

## 8. Grid store

- Format unchanged from the foundation: one raw little-endian array per layer × period on the canonical grid (4,200 × 2,700 = 11,340,000 cells), dtype `u8/u16/i16/u32/f32` with scale/offset/nodata in `geo.grid_layers`; file `store/grids/<layer>/<period>/<sha16>.bin` + a COG `.tif`.
- Add a 4 KiB header to the `.bin` (magic `LKGRID01`, dtype, rows, cols, bbox, step, nodata, scale, offset, sha256 of the payload) so a file is self-describing in offline packs; the payload starts at offset 4096 (page-aligned for mmap).
- Sizes: u8 11.3 MB, u16 22.7 MB, f32 45.4 MB per layer-period. 300 layer-periods mixed ≈ 8 GB on disk. Only layers flagged `serve: true` (point/radius queries, PNG tiles) are mapped by `lanka serve`; mmap means resident memory is whatever the page cache holds for cells actually read.
- Lookup arrays: `gnd_idx` (u16, unit index of the reference vintage → `geo.units.key` via a side table), `postal_idx` (u16), `land_mask` (u8), `cell_area_m2` (f32, one value per row is enough: 4,200 floats). These are grid layers of dataset `grid-lookups`, versioned like any other; a new boundary vintage produces a new `gnd_idx`.
- Zonal pass: one linear scan of `gnd_idx` + the layer, accumulating into a 14k-slot array; ~30–60 ms per layer on one core **[A]**; all 300 layers in under a minute.
- Time-dense rasters (daily CHIRPS since 1981 = ~16,500 layers) are **not** stored as daily grid layers. They are zonal-aggregated at ingest directly from the upstream file to ADM2/ADM3 observations and monthly grid layers; the raw files stay in L0 under the raster retention rule.

## 9. Tiles

| Tileset | Built from | How | Refresh |
|---|---|---|---|
| `admin` (ADM0–4, zoom-gated), `electoral`, `local-authority`, `moh` | `geo.unit_geoms` | export FlatGeobuf → tippecanoe (in the sandbox) → PMTiles | when the boundary dataset version changes |
| `osm-base` layers (roads, rail, water, landuse, places) | `geo.features` | same | weekly (not daily) |
| per-dataset feature tilesets (POIs, facilities, hazards polygons) | `geo.features` | same; datasets declare `tiles: {minzoom, maxzoom, attrs[]}` | when the dataset version changes |
| grid PNG tiles | mmap arrays | rendered on request by `lanka serve` (256 px, nearest/average resampling, colour ramp from the indicator), cached by nginx | — |

`geo.tilesets (id, sha16, path, minzoom, maxzoom, layers jsonb, version_ids int8[], built_at)` indexes them. Clients either read `https://…/dl/tiles/<tileset>/<sha16>.pmtiles` with the PMTiles protocol (range requests straight from nginx, no application code) or use `/v2/tiles/{tileset}/{z}/{x}/{y}.mvt`, which `lanka serve` answers by reading the PMTiles file (`pmtiles` crate, directory cached in RAM).

**Choropleths do not get tiles.** The map endpoint returns columnar `{codes[], values[], breaks[]}` (kept from the foundation) and the client joins on p-code to the `admin` tileset (`promoteId: pcode`). 14,043 GN values are ~120 KB gzipped; this removes per-indicator tile generation entirely. No dynamic `ST_AsMVT` and no tile server process (Martin, pg_tileserv) — rejected in [ADR 0014](adr/0014-pmtiles-and-http-caching.md).

## 10. Caching

No CDN. Three layers:

1. **In-process**: catalogue snapshot, unit table (14k rows + other schemes), dim sets, codelists, PMTiles directories, current version map. Hot-swapped on `NOTIFY lanka_release`.
2. **HTTP validators**: every response carries `ETag = "<hash(route, normalised query, dsv set touched)>"` — the release id is deliberately *not* in the ETag any more, so a release that did not change the datasets a response depends on does not invalidate it. `If-None-Match` is answered `304` from memory without touching Postgres (the dsv set for a route is known from the catalogue snapshot).
3. **nginx `proxy_cache`** on local disk (`/var/cache/nginx/lanka`, `max_size=2g`, `inactive=7d`, key = `$request_uri` + `$http_accept` class + `$http_accept_encoding` class), `proxy_cache_lock on`, `proxy_cache_use_stale updating error timeout`, `proxy_cache_background_update on`, `proxy_cache_revalidate on` (nginx revalidates with the ETag, so a hit after expiry costs one in-memory 304).

| URL class | `Cache-Control` | nginx TTL |
|---|---|---|
| Unversioned data (`/v2/areas/…`, `/v2/indicators/…`) | `public, max-age=300, stale-while-revalidate=86400` (v1 rule kept) | 300 s, then revalidate |
| Version-pinned (`/v2/r/{data_version}/…`, `?as_of=`) | `public, max-age=31536000, immutable` | 30 d |
| Files under `/dl/datasets/<id>/<dsv>/`, `/dl/tiles/…/<sha16>…` | `public, max-age=31536000, immutable` | served by `sendfile`, not cached |
| `/dl/releases/current/…` | `no-cache` (302 to the versioned path) | — |
| `/v2/status*`, `/v2/health` | `no-store` | bypass |
| Live (`obs_hf`) | `public, max-age=60` | 60 s |
| Search/suggest | `public, max-age=60` | 60 s, `proxy_cache_min_uses 2` |

Compression: `gzip` and `brotli` done by `tower-http` for JSON (kept); nginx `gzip_static`/`brotli_static` for pre-compressed files in `/dl`.

## 11. Latency targets

On the single VPS, warm cache, measured at the application (WP-13 adds a `criterion` + `oha` benchmark suite that fails CI on > 30 % regression against the recorded baseline):

| Request | p50 | p99 |
|---|---|---|
| reverse geocode (grid path) | 0.3 ms | 2 ms |
| area profile (`serve.area_latest`) | 3 ms | 15 ms |
| series, ≤ 25 geographies | 2 ms | 10 ms |
| map, ADM3 (339 units) / ADM4 (14,043 units) | 3 ms / 20 ms | 12 ms / 60 ms |
| provenance of one observation | 2 ms | 10 ms |
| MVT tile from PMTiles | 1 ms | 5 ms |
| grid PNG tile | 4 ms | 20 ms |
| 304 revalidation | 0.05 ms | 0.5 ms |

The foundation measured 2–15 ms per request locally for area, reverse and map on the 0001 schema; the targets assume the narrower table does no worse.

## 12. Capacity plan — one VPS (8 vCPU / 15 GB RAM / 150 GB NVMe)

### Memory

| Consumer | Budget | Limit mechanism |
|---|---|---|
| PostgreSQL (`shared_buffers` 3 GB + backends + maintenance) | 6 GB | compose `mem_limit: 6g` (was 7g) |
| `lanka serve` (heap ~300 MB + mapped grids via page cache) | 1 GB | `mem_limit: 1g` (mmap pages are page cache, not counted against RSS hard limits the same way; `MADV_RANDOM`) |
| `lanka worker` (ingest + light recipes) | 2 GB | `mem_limit: 2g` |
| `lanka-tools` sandbox (poppler/tesseract/GDAL/tippecanoe) | 2 GB peak, 0 idle | `mem_limit: 2g` |
| Valhalla (batch profile, only while a routing recipe runs; worker heavy slot is exclusive) | 3 GB peak, 0 idle | `mem_limit: 3g` |
| v1 API (Hono + SQLite) until retired | 0.3 GB | existing |
| nginx + OS + page cache | ≥ 2 GB | remainder |

Peak (tools + Valhalla never run together: both use the single heavy slot): 6 + 1 + 2 + 3 + 0.3 = 12.3 GB.

### Disk

| Consumer | Year 1 budget **[A]** | Basis |
|---|---|---|
| Postgres data (observations ~150 M rows × ~85 B heap + ~1.3× indexes ≈ 30 GB; features + geometry 8 GB; ops/catalog/serve 5 GB; bloat/headroom 30 %) | 55 GB | §2 |
| WAL + pgBackRest spool | 10 GB | `max_wal_size 8GB` |
| `store/raw` (documents ~6 GB; rasters + PBF under retention ~14 GB) | 20 GB | [01 retention](01-layers.md#retention) |
| `store/extract`, `store/evidence` | 3 GB | |
| `store/grids` (300 layer-periods + COGs) | 12 GB | §8 |
| `store/datasets`, `store/releases`, `store/tiles` | 20 GB | dsv-keyed, 24-month retention |
| Valhalla tiles + OSM extract | 3 GB | |
| nginx cache, logs, Docker images | 7 GB | |
| **Total** | **130 GB of 150 GB** | |

This is tight by design: the disk is the first limit that will be hit, which is why the first scale-out step is a volume, not a server.

### CPU

API traffic at 50 req/s average is < 0.5 core with the cache in front **[A]**. The worker's heavy slot (GDAL warp, tippecanoe, routing, big COPYs) can saturate 4–6 cores for minutes to hours; it is `cpus: 5` and `nice`-d so `lanka serve` and Postgres keep headroom.

## 13. Scale-out path

Each step is independent, triggered by a measured threshold exposed in `/v2/status` (so the external monitor can see it coming), and needs no redesign.

| Step | Trigger (any) | Action | New moving parts |
|---|---|---|---|
| S1 — more disk | filesystem > 75 % for 7 days | attach a Hetzner volume, move `store/` to it (`LANKA_STORE_DIR`), keep Postgres on NVMe | none |
| S2 — worker box | heavy jobs overlap with API p99 > 2× target on 3 days in a month; or a recipe needs > 3 GB RAM; or Valhalla must stay up | second box runs `lanka worker` + tools + Valhalla, connects to Postgres over WireGuard; primary keeps `lanka serve`; store shared via S3 (S4) or, initially, the worker writes blobs through an authenticated internal `PUT` endpoint on the primary | WireGuard, second host |
| S3 — read replica + second API | sustained > 150 req/s uncached, or Postgres CPU > 60 % from reads for 7 days, or a need for zero-downtime upgrades | streaming replica (physical, async); `lanka serve` on both hosts; PgBouncer (transaction mode) per host; nginx `upstream` with two backends (`least_conn`); replica lag exposed in `/v2/health` and the API refuses a replica more than one release behind | PgBouncer, replication |
| S4 — object store | store > 100 GB, or S2/S3 need shared blobs, or downloads > 30 % of bandwidth | Garage on the hosts that exist (replication factor 2–3), `S3Store` backend, nginx proxies `/dl` to Garage's web endpoint; grids synced to a local cache directory per API host | Garage |
| S5 — bigger primary | Postgres working set > RAM (buffer hit ratio < 98 % on hot queries) | vertical resize (32 GB) before any sharding; observation partitions are already independent, so cold partitions can later move to a second tablespace/volume | none |

Not on the path: sharding, Citus, Kafka, Kubernetes, a separate analytics database. If a use case for ad-hoc analytical SQL appears, the answer is the published Parquet files and DuckDB on the user's machine.
