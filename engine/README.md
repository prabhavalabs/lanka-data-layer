# engine — Lanka Data Layer v2 (Rust)

The v2 data platform: ingestion, Postgres/PostGIS store, REST v2 server and the `lanka` CLI.
Design: [docs/architecture-v2/](../docs/architecture-v2/README.md) (the standard model is
[02-standard-model.md](../docs/architecture-v2/02-standard-model.md); storage in
[05-storage-and-performance.md](../docs/architecture-v2/05-storage-and-performance.md)). Foundation:
[docs/data-platform-v2.md](../docs/data-platform-v2.md).

| Crate | What |
|---|---|
| `lanka-core` | standard-model types — country profile, geography codes (profile-driven p-codes + `scheme:code`), periods (`freq` + `time_start`, full grammar), units/measure semantics, `basis`/`status`/method/review codes, flag registry, dims, dsv multiset hashing — plus the canonical grid (pinned to the v1 test vectors), envelope, Postgres pool + embedded migrations, catalog snapshot (surrogate-key maps) and every read query. No country literal: everything country-specific comes from `country/<id>/profile.yaml`. |
| `lanka-store` | the blob store behind one `BlobStore` trait (`FsStore` today; S3/Garage later): content-addressed `raw/` (L0) and `extract/` (L1), immutable named objects, atomic temp + fsync + rename |
| `lanka-ingest` | registry loader + JSON-Schema validation + sync (profile, units, codelists, connector specs, schedules), connector SDK v2 (`discover` → `extract` → `standardise`), declarative drivers (`http_json`, tabular `csv`/`sdmx`/`ckan`/`xlsx_table`/`html_table`/`git_files`, spatial `osm_pbf`/`geo_file`/`raster`; `registry/connectors/*.yaml`), declarative expectations (`expect.rs`), `lanka connector test`, code connectors (`dcs-census-2024`, `hdx-cod-ab-lka`), L1 Parquet extracts, layered change detection, run tracking + the event feed, validate → stage (binary COPY + set-based diff, incremental dsv, range-versioned) → gate → approve/review, the job queue (`jobs.rs`), scheduler (`scheduler.rs`), release train (`train.rs`) and `lanka worker` (`worker.rs`), the WP-01 registry migration |
| `lanka-mapdoc` | the `ldl-map/1` map document (WP-26): serde types (`x-*` keys preserved), validation (embedded `spec/map/1/map.schema.json` + limits/references, JSON-pointer problems), RFC 8785 canonical JSON + content hash, references (`app.map_refs`), resolution against the catalogue, the ramp/tone registry, generated TypeScript |
| `lanka-server` | axum `/v2/*` (data, frames, health, status) + OpenAPI (utoipa) |
| `lanka-cli` | the `lanka` binary |

Toolchain: Rust 1.94+ (edition 2024; sqlx 0.9 needs 1.94). Crate versions are pinned exactly in `Cargo.toml`.

## Run it locally

From the repository root:

```bash
# 1. Postgres 18 + PostGIS 3.6 on 127.0.0.1:55432
cp infra/v2/.env.example infra/v2/.env          # set POSTGRES_PASSWORD
docker compose -f infra/v2/docker-compose.yml --env-file infra/v2/.env up -d

export DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/lanka
cargo build --release --manifest-path engine/Cargo.toml
alias lanka=./engine/target/release/lanka

# 2. schema + registry + country profile
lanka db migrate               # migrations, then registry/profile sync + dsv backfill of migrated versions
lanka registry validate        # registry/*.yaml + country/<id>/profile.yaml against registry/schema/*.json
lanka registry sync            # mirror into catalog.* / geo.schemes (ingest runs do this too)

# 3. ingest (order matters: boundaries first — other sources key to p-codes)
lanka ingest run hdx-cod-ab-lka --from ../lanka-data-layer/foundry/data/artifacts/layers   # v1 layer exports
lanka ingest run worldbank-wdi                 # live API (YAML driver), class A → approved
lanka ingest run dcs-census-2024               # live XLSX, class B → waits for review
lanka ingest run worldbank-wdi                 # again: no_change at the fingerprint, nothing fetched
lanka ingest run worldbank-wdi --force         # skip the fingerprint/artifact short-circuits
lanka ingest restandardise worldbank-wdi       # offline: standardise → publish from the stored extracts
lanka review list
lanka review approve <dsv> --by "<your name>" --note "checked against the report"   # → approved
lanka release cut                              # the train: every approved version in one release
lanka ingest status

# 4. serve (and, unattended, the worker)
lanka serve --bind 127.0.0.1:8601
lanka worker                                   # scheduler tick + job executor (see "Operations")
curl -s localhost:8601/v2/status | jq .payload
curl -s localhost:8601/v2/areas/LK11 | jq '.payload.indicators[0], .meta.dataset_versions'
curl -s 'localhost:8601/v2/indicators/wdi.ny_gdp_mktp_cd/series?geo=LK&from=2020'
curl -s 'localhost:8601/v2/indicators/wdi.series/series?geo=LK&dims=series_code=SP.POP.TOTL'
curl -s 'localhost:8601/v2/indicators/census24.pop_total/map?level=3&parent=LK1&dims=sex:total'
curl -s 'localhost:8601/v2/frame?indicators=census24.pop_total,census24.sex_ratio&level=3&dims=sex=total'
curl -s 'localhost:8601/v2/frame?indicators=census24.pop_total&level=2&format=arrow' -o frame.arrow
curl -s 'localhost:8601/v2/r/20261122.1/frame?indicators=wdi.ny_gdp_mktp_cd&level=0&from=2015&to=2023'
lanka map validate my-map.json && lanka map resolve my-map.json
curl -s localhost:8601/v2/observations/<obs_id>/provenance
curl -s 'localhost:8601/v2/reverse?lat=6.9344&lon=79.8428'
curl -s 'localhost:8601/v2/units?level=3&parent=LK1' | jq '.payload.total, .payload.units[0]'
curl -s 'localhost:8601/v2/units/LK11/geometry?simplify=display&format=geojson'
curl -s localhost:8601/v2/openapi.json

# CLI queries use the same core
lanka reverse 6.9344 79.8428
lanka area LK1103
```

`--from` on `hdx-cod-ab-lka` points at the v1 foundry's `foundry/data/artifacts/layers/` (gitignored; produced by `pnpm foundry run build`). The census connector reads statistics.gov.lk directly; pass `--from <dir>` with `a5-sex-age.xlsx`, `a6-ethnicity.xlsx`, `a7-religion.xlsx` to work offline.

## Data model (migration 0002, WP-01)

- **Observations** (`data.observations`, LIST-partitioned by `dataset_key`): integer keys
  (`dataset_key`, `indicator_key`, `geo_key`, `dims_key`), period as `(freq, time_start)`,
  `value`/`lo`/`hi`, `basis` (reported, computed, estimated, forecast) and `status`
  (normal, provisional, revised, break, low_reliability, missing, nil, suppressed) as `int2`,
  a `flags` bitmask (registry in `lanka-core/src/flags.rs`), and the version range
  `vfrom`/`vto` (dataset-local seqs; `vto = 2147483647` while current). `obs_id = dataset_key << 40 | seq`.
  The only UPDATE ever allowed is the `vto` close at release (trigger `observations_guard`).
- **Staging**: rows are COPY'd into a temp table and diffed set-wise against the released rows;
  only new or changed rows are inserted (`vfrom = n`), the rows they replace are listed in
  `data.version_closes` and closed when the version is released; rejecting deletes them.
  Unchanged re-runs are `unchanged` with nothing written.
- **Provenance** (`data.obs_provenance`, 1:1 by `obs_id`): artifact, locator, verbatim, method, review.
- **dsv** = `dsv(schema_hash, multiset_hash, rows)` — an order-independent, incremental
  sum of row hashes over codes (not keys); `legacy_dsv` keeps the 0001 value.
- **Geography**: `geo.units` (every coded geography; admin p-codes bare, others `scheme:code`;
  `anc_keys` ancestors), `geo.unit_geoms` per boundary set, `geo.schemes`, reference vintage
  flag on `geo.boundary_sets`; `geo.admin_units` is a compatibility view.
- **Catalogue**: UCUM `catalog.units_of_measure` (`registry/units.yaml`), `catalog.codelists`
  and `catalog.codes` (`registry/codelists/*.yaml`), interned `catalog.dim_sets`, indicator
  `measure_type`/`aggregation`/`class`/`unit_mult`, licence `spdx`/`redistribution`,
  `catalog.profile`. Human-readable codes: `data.code_labels`; the current release in codes:
  `data.current_observations_coded`.
- **Registry**: indicators declare `unit` (UCUM), `measure_type`, `aggregation`, `class` and
  codelist-backed `dims`; `lanka registry migrate [--write] [--review-out FILE]` performed the
  one-off conversion of the 263 datasets (review list: `registry/review-wp01-standard-model.csv`).
  Datasets whose dims have no codelist yet validate with a warning and fail at ingest.

## Ingestion v2 (WP-02)

Six layers ([01](../docs/architecture-v2/01-layers.md)); WP-02 implements L0–L2:

```
discover ─► fetch ─► archive ─► extract ─► standardise ─► validate ─► stage ─► gate ─► publish | review
 fingerprint  conditional GET   L0 raw/     L1 extract/      pure            binary COPY → diff
              2/host, 1 req/s   (sha256)    (Parquet)                        → range insert
```

- **Store** (`lanka-store`): every file goes through `BlobStore`. `raw/ab/cd/<sha256>` (layout of the
  foundation's archive, unchanged) and `extract/ab/cd/<sha256>.parquet` are content-addressed; downloads
  stream into `tmp/` and are renamed into place, never held in memory.
- **Extracts** (L1, `ops.extracts`, `ops.extract_artifacts`, `ops.run_extracts`,
  `data.dataset_version_extracts`; migration 0003): zstd Parquet in one of three shapes — `cells`
  (XLSX/PDF/HTML tables), `records` (JSON/CSV/API), `features` (geodata). Encoding is deterministic, so the
  same artifacts + extractor version give the same extract hash. Additive deviations from 01 §L1:
  `cells.vtype` (string vs number cell), `records.table_id` (API metadata vs data rows) and
  `features.geojson` instead of `wkb` (the boundary dsv hashes the source GeoJSON text; WKB comes with
  `geo_file`, WP-04). Provenance rows written from WP-02 on carry `extract_id` + `extract_row`.
- **Connector SDK v2** (`connector.rs`): `discover` (network allowed, returns candidates + fingerprint),
  `extract` (bytes → L1; no network, no database), `standardise` (L1 → outputs through a streaming
  `OutputSink`; pure: extract + registry + a read-only `Lookup` snapshot, no `PgPool`). `connector_for`
  is registry-driven: a `registry/connectors/<source>.yaml` selects a **driver**, otherwise a code
  connector from `connectors/mod.rs`.
- **Drivers**: `http_json` (REST/JSON: key lists from the registry, URL templates, a JSONPath subset
  that keeps numbers' exact tokens, metadata/pagination checks, status mapping, locator and verbatim
  templates). `worldbank-wdi` runs from `registry/connectors/worldbank-wdi.yaml` (schema
  `registry/schema/connector.schema.json`); `world_bank.rs` is gone and its tests are the driver's golden
  test (same locators, verbatim tokens, dsv `dsv_ebf6ae74a3b83910`).
- **Change detection**, cheapest first: (1) the fingerprint (driver-defined, e.g. WDI's database
  `lastupdated`; file size + mtime for local inputs) keyed with the connector version and the source,
  dataset, connector-spec and profile hashes equals the last successful run's → `no_change`, nothing
  fetched; (2) the artifact set (sha256 per key) equals the last successful run's → `no_change` before
  extract; conditional GET (`If-None-Match`/`If-Modified-Since`) reuses the stored artifact on 304;
  (3) staging finds no added/changed/restated/removed row → `unchanged`, nothing hashed or written.
- **Staging**: binary `COPY` into a session temp table (never WAL-logged; the per-session form of 05's
  `stage.*`), `ANALYZE`, one set-based classification against the released rows, then the insert of
  new/rewritten rows only. The dsv is **incremental**: released multiset + hashes of the new rows −
  hashes of the rows they close, so a re-run hashes nothing and appending 1,000 rows to 1 M hashes
  exactly 1,000 (`PublishResult.rows_hashed`). Measured (release build, loaded laptop): 1,004,400 rows
  staged in 8.4 s; identical re-stage 6.8 s, 0 hashed; +1,000 rows 6.6 s, 1,000 hashed and written.

### Decision note: `status = revised` (02 §5, 05 §3)

*Context.* 02 §5 says a changed value for a key opens a new row with `status = revised` unless the
source states provisional/final; 05 §3 puts `status` into the canonical row and requires the dsv to be
path-independent and a function of the delivered content.

*Decision.* `revised` is **derived by the pipeline, never delivered**: connectors deliver the source
status (validation rejects `revised` from a connector). At staging, a row is stored as `revised` when the
source says `normal` and its value/interval differs from the released row of the same key, or when it
rewrites a row that was already `revised` (a restatement keeps the mark; unchanged rows are not
rewritten and keep theirs). The diff compares stored `revised` as `normal`, and the canonical row hashes
`revised` as `normal` (`lanka_core::dsv::hashed_status`).

*Consequences.* The same content has the same dsv whatever history led to it; the stored row and the
source row hash identically, so the incremental dsv equals a full recomputation from the database (the
property test checks this after every release). A source that itself publishes a "revised" flag maps it
to a flag or note, not to this status. No existing dsv changed (the four foundation versions hold no
`revised` rows).

### Decision note: returning to earlier content (A → B → A, migration 0008)

*Context.* The dsv is path-independent (05 §3), so a delivery that restores the content of an older,
superseded version has that version's dsv. Staging found the old row by dsv and returned `unchanged`:
the restored content was never published and B stayed current.

*Decision.* A dsv names **content**, not a version row. Migration `0008_version_reinstatement.sql`
drops `UNIQUE (dsv)` (plain index + a partial unique index over open versions: two rows with one
dsv are never both staged/in review/approved). Staging is blocked only by an *open* or *withdrawn*
version with the same content; a superseded one is not: the delivery stages a new version (new seq)
whose rows reinstate A's values against B (`vfrom = seq`, B's rows closed at release, `revised`
derived as usual), with A's dsv. The old version row and every release that pinned it are untouched,
so history and `as_of` stay reconstructable. `lanka review approve <dsv>` addresses the open row;
boundary versions (`geo_load`) reuse only open/rejected rows. Readers that resolve a dsv to rows
(`/v2/frame?dsv=`) take the latest released seq — the rows are identical.

*Test.* The range-version property test now draws half its histories from a pool of 2–3 contents
(A → B → A, A → B → C → A …) and checks: same content ⇒ same dsv, a revisit is `published` as a new
version row, current rows = delivered content, every released seq and every release pin reconstructs.
It fails on the old code.

### Observation indexes (migration 0003)

`_series` and `_map` lost their INCLUDE lists; `_map` also drops `freq` (one indicator, one frequency)
and, being highly repetitive, deduplicates to a few percent of its 0002 size. Reads now fetch tens to a
few hundred heap rows that sit together (rows are inserted per version in source order).

Synthetic 10,091,900-row partition (50 indicators × 14,417 units × 14 years, ordered inserts, fresh
`CREATE INDEX`; `pgbench` single client, custom plans, mean latency; two alternated rounds, second shown — the first agreed within 25 %). Index sizes: PK 546 MB unchanged, `_series` 834 → 304 MB, `_map` 835 → 68 MB:

| | heap | indexes | ratio | series (5 geos) | map ADM3 | map ADM4 | area | obs by id |
|---|---|---|---|---|---|---|---|---|
| 0002 | 904 MB | 2,215 MB | 2.45× | 0.44 ms | 0.42 ms | 7.04 ms | 0.86 ms | 0.70 ms |
| 0003 | 904 MB | 918 MB | 1.02× | 0.35 ms | 0.39 ms | 6.11 ms | 0.82 ms | 0.65 ms |

Endpoints on the local release (curl p50, 300 requests): area LK11 2.57 → 1.93 ms, series 2.50 → 1.42 ms,
map ADM3 3.15 → 2.56 ms, provenance 1.76 → 1.58 ms (the machine was less loaded for the second run; the
point is that none regressed).

## Operations (WP-03)

Migrations `0004_operations.sql` (job queue, trains, reviews, incidents, event feed, roles) and
`0005_serve.sql` (`serve.area_latest`). Design: [07](../docs/architecture-v2/07-operations.md),
ADRs [0011](../docs/architecture-v2/adr/0011-postgres-job-queue-worker.md),
[0012](../docs/architecture-v2/adr/0012-daily-release-train.md),
[0016](../docs/architecture-v2/adr/0016-status-apis-only.md).

```
registry (cadence, schedule, calendars) ─► ops.source_freshness ─► scheduler tick (60 s, leader)
                                                                       │ enqueue (dedupe)
                                    NOTIFY lanka_jobs ─► ops.jobs ◄────┘
                                                           │ claim: FOR UPDATE SKIP LOCKED, lane, weight ≤ free slots
                                     lanka worker ◄────────┘ session advisory lock per job + 15 s heartbeat
                                        │ ingest ─► run ─► version approved (class A) / in_review (B/C)
                                        │ release (daily 20:30 UTC, ≤ 1/h for live datasets, `lanka release cut`)
                                        └────────► one release: all approved versions + serve.area_latest ─► NOTIFY lanka_release
```

- **Approval ≠ publication.** Staging never releases. A class-A version that passes the gate and a
  B/C version a reviewer signs off (`lanka review approve`) become `approved`; the next train pins every
  approved version into one `data_version` and refreshes `serve.area_latest` in the same transaction.
  `ops.reviews` keeps every decision. Boundary geometry (`geo.units`) is still updated in place at
  staging until boundary vintages are release-pinned (WP-10).
- **Jobs** (`ops.jobs`): kinds of 07 §3 plus `analysis`, `upload`, `sync`; lanes `batch`/`interactive`.
  Idempotent enqueue (one live job per `dedupe_key`), claim by priority (release 10, ingest 50), slot
  weights (`LANKA_WORKER_SLOTS=4`, an ingest weighs 2), retries 15 min → 1 h → 6 h, then `failed` + a
  `job_failed` incident. A `running` job whose advisory lock is free belongs to a dead worker and is
  re-queued by the next tick. `lanka jobs cancel <id>`: queued at once, running on the next heartbeat.
  SIGTERM: running jobs get `LANKA_WORKER_GRACE` seconds, then are re-queued without spending an
  attempt. This version executes `ingest`, `restandardise` and `release`; other kinds stay queued.
- **Leader/standby**: `pg_try_advisory_lock('lanka_worker_leader:<lane>')`; a second worker idles
  and takes over when the leader's session ends.
- **Scheduling** is data: `schedule: {check_every, window, stale_after, release_calendar}` and
  `paused` in `registry/sources/*.yaml` (all optional; defaults by cadence, 03 §4), synced to
  `catalog.sources` / `ops.release_calendar`; `catalog.sources.schedulable` = an implemented connector
  exists. `ops.source_freshness` computes every source's state (`fresh | due | overdue | failing |
  in_review | expected_missing | never_run | paused | retired`) and `next_check_at`; a reached calendar
  date makes a source due (`upstream_change`). One-off sources run until their first success.
  Sources that read local exports get them from `LANKA_LOCAL_INPUTS=hdx-cod-ab-lka=/dir`.
- **After a rejection** the source's earlier change keys are void, so the next run fetches and
  stages again without `--force`; identical content re-opens the review (the rejected row keeps its
  decision under a renamed dsv).
- **Monitoring is emit-only** (webhook alerts, heartbeat ping and `ops.alerts` are gone):

| Endpoint | |
|---|---|
| `GET /v2/health[?deep=1]` | 200 / 503 (Postgres unreachable or no release); db latency, store, uptime, version |
| `GET /v2/status` | always 200; `overall` ok/degraded/failing + `reasons`, worker tick age and queue, release + last train, source counts, open incidents, disk, backups |
| `GET /v2/status/sources?state=&schedulable=1` | per-source state, last run, next check, expected edition |
| `GET /v2/status/datasets?state=` | released dsv, pending (approved / in review) dsv |
| `GET /v2/status/runs?since=&source=&status=&cursor=&limit=` | run history, newest first, cursor-paged |
| `GET /v2/status/events?after=&level=&source=&limit=` | event feed, strictly increasing ids in commit order (`ops.emit` serialises inserts); NDJSON with `Accept: application/x-ndjson` |
| `GET /v2/status/incidents?open=1` | opened/closed automatically: `source_failing`, `expected_missing`, `train_failed`, `job_failed`, `disk_pressure`, `backup_stale`, `default_partition_rows` |

  `overall`: **failing** if the worker's last tick is > 5 min old, the last train failed or a P0 source
  is failing; **degraded** if any source is failing/overdue/expected_missing, the oldest runnable job
  waits > 6 h, the store disk is > 85 % or (once `lanka ops record-backup` has been used) the last good
  pg/store backup is > 36 h old.
- **Roles** (05 §7): `lanka_owner` (NOLOGIN) owns every schema object; `lanka_api` (read-only by
  default transaction, 5 s statements; data plane + status tables), `lanka_ingest` (rows only, no DDL;
  releases/changelog append-only; partitions through the `SECURITY DEFINER` `data.ensure_obs_partition`;
  events through `ops.emit`; `pg_maintain` for ANALYZE), `lanka_recipe` (read-only). `lanka db migrate`
  runs as the admin role and re-applies `ops.apply_grants()`; `--set-role-passwords` (or
  `lanka db set-passwords`) takes `LANKA_API_PASSWORD` / `LANKA_INGEST_PASSWORD` / `LANKA_RECIPE_PASSWORD`.
- **Images and compose**: `infra/v2/Dockerfile` (context `engine/`; Rust 1.94 build stage →
  `gcr.io/distroless/cc-debian13`, uid 10001; 22 MB compressed). `infra/v2/docker-compose.yml`: `db`,
  one-shot `migrate`, `serve` (lanka_api, store read-only), `serve-green` (profile `green`), `worker`
  (lanka_ingest); read-only root filesystems, `cap_drop: ALL`, `no-new-privileges`, ports on
  127.0.0.1 only. `infra/v2/deploy.sh <tag>`: pull → migrate → start the idle colour → health → nginx
  upstream switch (`infra/v2/nginx/lanka-v2.conf`) → stop the old colour → recreate the worker.

```bash
lanka worker [--lane batch] [--slots 4] [--once]      # --once: one tick, drain runnable jobs, exit
lanka jobs list | show <id> | cancel <id>
lanka jobs enqueue ingest <source> [--from DIR] [--force] | restandardise <source> | release [--note]
lanka release cut [--note TEXT] | list
lanka ops tick                                        # one scheduler tick by hand
lanka ops record-backup --kind pg|store|drill --ok    # backup scripts end with this (WP-25)
lanka serve rebuild                                   # regenerate serve.area_latest
lanka probe http://127.0.0.1:8601/v2/health           # container health check (no curl in the image)
docker compose -f infra/v2/docker-compose.yml --env-file infra/v2/.env up -d --build
```

### Decision note: WP-03 deviations

- *Migration numbers*: 0003 was taken by WP-02's extracts, so the rest of 05 §1's `0003` and `0004_roles`
  are one `0004_operations.sql`; `0005_serve.sql` keeps its number (WP-06/WP-04 keep 0006/0007).
  `ops.extract*`/`check_results`/`takedowns`/`review_cells` are left to WP-07/WP-08.
- *Freshness is a view* (`ops.source_freshness`), not the materialised `ops.source_health_mv`: ~200
  sources make it a few ms, and it can never be staler than the run history.
- *Leases*: the session advisory lock is authoritative; `heartbeat_at`/`lease_until` are visibility.
- *Event feed* is `ops.run_events` generalised (nullable `run_id`, `kind`, `source_id`, `dataset_id`,
  `job_id`) rather than a second table.
- *Disk*: the worker measures the store filesystem (`statvfs`) each tick; the database volume is not
  visible to the engine, so `disk.pg_pct` is null and `pg_bytes` is reported instead.
- *Backups*: staleness counts only after the first `lanka ops record-backup` (WP-25 delivers the scripts).
- Not in this package: `/internal/metrics` (Prometheus), the `tools` image and `lanka-extractd` (WP-08),
  export/tiles/check jobs after a train (WP-09), recipe recomputation (WP-06; the train calls a no-op hook).

### Environment

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | local dev URL | Postgres |
| `LANKA_REGISTRY_DIR` | nearest `registry/` above cwd | registry root |
| `LANKA_COUNTRY_PROFILE` | the only `country/*/profile.yaml` beside the registry | country profile (ingest/sync; servers read `catalog.profile`) |
| `LANKA_GIT_REV` | `unknown` | git revision recorded on dataset versions |
| `LANKA_STORE_DIR` | `<repo>/data/v2` | blob store root (`raw/`, `extract/`, `tmp/`, …; gitignored). `LANKA_DATA_DIR` is still accepted for one release |
| `LANKA_FETCH_RPS` | `1` | request starts per second per upstream host (plus 2 in flight per host) |
| `LANKA_BIND` | `127.0.0.1:8601` | server bind (never 0.0.0.0 on a host; inside a container `0.0.0.0` with the port published on 127.0.0.1) |
| `LANKA_WORKER_LANE` / `LANKA_WORKER_SLOTS` | `batch` / `4` | worker lane and slot budget |
| `LANKA_WORKER_TICK` / `LANKA_WORKER_GRACE` | `60` / `25` | scheduler tick and SIGTERM grace, seconds |
| `LANKA_TRAIN_AT` | `20:30` | daily release train, UTC |
| `LANKA_LIVE_TRAINS` | `true` | intraday trains (≤ 1/h) for `live` datasets with an approved version |
| `LANKA_LOCAL_INPUTS` | — | `source=/dir,…`: `--from` for scheduled runs of sources that read local exports |
| `LANKA_API_PASSWORD` / `LANKA_INGEST_PASSWORD` / `LANKA_RECIPE_PASSWORD` | — | role passwords set by `lanka db migrate --set-role-passwords` |
| `LANKA_LOG_JSON` | false | JSON log lines |
| `LANKA_EXTRACTD_SOCKET` | — | Unix socket of the `tools` sidecar (`lanka-extractd`); every PDF/OCR operation goes through it |
| `LANKA_EXTRACTD` | — | `require`: refuse to run document tools without the socket (production); unset: local subprocesses under the same limits (development only, logs a warning) |
| `LANKA_EXTRACTD_{WALL_SECS,CPU_SECS,MEM_MB,FSIZE_MB,MAX_INPUT_MB,MAX_OUTPUT_MB,MAX_RENDER_MPX}` | 60, 60, 1024, 256, 200, 64, 40 | per-call limits of the sandbox |
| `LANKA_REVIEWERS` | — | comma-separated reviewer handles allowed to approve, reject and sign (unset: any non-empty name) |
| `LANKA_LLM_PROVIDER` / `LANKA_LLM_ENDPOINT` / `LANKA_LLM_API_KEY` / `LANKA_LLM_MODEL` | `openai` / — | operator-only, `lanka extract propose` (class C); never set on the worker |
| `LANKA_LLM_USD_PER_MTOK_IN` / `_OUT` | 5 / 25 | prices for the `--max-usd` spend cap |
| `LANKA_PDF_FIXTURES` | — | directory of real PDFs (`<file key>.pdf`) for `tests/pdf_templates.rs` |

### PDF tables, evidence and review (WP-08)

```sh
lanka template test dcs-lfs/annual-table-4.1-employment-by-industry LFS2025.pdf --edition 2025
lanka ingest run dcs-lfs --from DIR          # DIR/<file key>.pdf; leaves the version in_review + evidence sheet
lanka review open dsv_…                      # evidence sheet on 127.0.0.1:8790 (page renders, cell overlays, crops)
lanka review approve dsv_… --by NAME         # unchanged
lanka extract propose <source> --file scan.pdf --url URL --table "Table 4.1a" --readings a.json --readings b.json
lanka review sign x<id> --row N --decision accept|correct|reject [--text T] --by NAME
lanka review approve x<id> --by NAME         # refused until every cell < 1.0 and the 10 % sample are signed
```

Templates live in `registry/templates/<source>/<table>.yaml` (`registry/schema/template.schema.json`); the `tools` sidecar is `infra/v2/tools.Dockerfile` (no network; `cargo test -p lanka-extractd --test sandbox`).
| `RUST_LOG` | `info,sqlx=warn` | tracing filter |

## Map documents and frames (WP-26)

Design: [10 §2–§3.1](../docs/architecture-v2/10-map-composition.md), [ADR 0018](../docs/architecture-v2/adr/0018-map-document-spec.md),
[ADR 0019](../docs/architecture-v2/adr/0019-frames-value-tiles-columnar-features.md), [ui-contract-explore.md](../docs/architecture-v2/ui-contract-explore.md).

- **Spec** `spec/map/1/` (CC BY 4.0): `map.schema.json` (normative, draft 2020-12), `ramps.json` (ramps and
  `ldl.tone.*` tones, light/dark variants), `index.d.ts` + `package.json` (`@lanka-data-layer/map-spec`,
  generated by `lanka map types`). `lanka-mapdoc/schema/` holds byte-identical copies (the image builds from
  `engine/` only); a test fails when they drift, another when `index.d.ts` is stale.
- **Validation** (`lanka map validate <file>…`, exit 1 on problems): schema first, then what it cannot say —
  32 layers / 8 groups / depth 3 / 16 sources / 6 analyses / 256 KB canonical JSON / 100 KB inline GeoJSON,
  unique node ids, every layer/analysis/panel reference, layer type ↔ source kind (10 §2.5), CQL2 arity,
  breaks ascending and matching `classes`, ramp names and kinds, tones, categorical `mapping` values
  (colours for colour channels, numbers for width/size), zoom/bbox/time order. Every problem is
  `{path: <JSON pointer>, code, message}`. Fixtures: the 10 §7 example and the web-v2 starter maps
  validate; 39 negative fixtures fail at their pointer; `x-*` keys at every level survive parse → serialise.
- **Canonical JSON** is RFC 8785 (UTF-16 key order, ECMAScript numbers); the content hash leaves out `id`,
  `meta.updated_at` and `null` members (null ≡ absent); `lanka map hash` prints `ms_<16 hex>`.
- **Resolution** (`lanka_mapdoc::resolve`, `lanka map resolve [--freeze]`; the HTTP routes are WP-28's):
  per source the dataset versions read (document `pin` release, `{dsv}`, or latest), the frame URL
  (`/v2/r/<release>/frame?…` when pinned, `dsv=` for exact versions, `from/to` = `time.range` for
  time-bound sources, one URL per level for `byZoom`), the admin tileset join (`/v2/tiles/admin/…`, built by
  WP-04), class/basis, attribution (every active source behind every dataset read, plus external hosts),
  notices and problems. Not served yet → `shape: "unavailable"` + `NOT_SERVED` (features, grids: WP-27;
  analyses: WP-29). Workspace ids → `access: "denied"` + `PRIVATE_DATA`.
- **Frames** `GET /v2/frame` and `GET /v2/r/{data_version}/frame` (`lanka_core::frame`): `indicators` (≤ 8),
  `level`, `parent`, `periods` | `from`/`to` (default: the latest at the level, per indicator), `dims`
  (`[<indicator>:]<pairs>` entries joined by `|`; an entry selects the combinations containing its pairs;
  unscoped entries apply to indicators that have those dimensions; default all), `dsv` (exact versions),
  `format=json|arrow|csv`. `codes` = every unit of the reference boundary set at the level (sorted), so
  arrays line up with the tileset; `null` = no value. Columns ordered (indicator, dims, period), ids
  `c0…`; `basis`/`status` scalar when uniform, else 05 §2 codes; `lo`/`hi`/`flags` null when absent;
  `stats` = n, min, max, mean, p0/5/25/50/75/95/100 (R type 7). Limits: 64 columns, 64 periods per
  indicator, codes × columns ≤ 500,000 (400 with the reason). Reads: one indexed range read per indicator
  (current release on the partial `_map` index; pinned on `vfrom ≤ seq ≤ vto`).
- **Caching**: `ETag = "<data_version>-<hash(path, query, dsvs)>"`; for current-release frames the ETag is
  computed from the catalogue snapshot and `If-None-Match` is answered 304 **before** any query.
  `Cache-Control`: 300 s + SWR 1 d floating; 1 d + SWR 7 d pinned (not `immutable` while boundary geometry
  is updated in place, WP-10). Arrow/CSV carry `X-Data-Version` and `X-Dataset-Versions`; Arrow schema
  metadata carries `ldl:frame` (support) and `ldl:meta` (the full envelope meta, so attribution travels).

Measured on the local release (release build, laptop, curl, 200 requests; 2026-11-22, release 20261121.4):

| Request | Bytes (raw / gzip) | p50 | p99 |
|---|---|---|---|
| census pop_total ADM3 (339 codes × 1 col) | — | 5.0 ms | 10.2 ms |
| census pop_total + pop_by_religion ADM3 (339 × 7), JSON | 22,990 / 8,109 | 7.7 ms | 10.0 ms |
| same, Arrow (Float64) / CSV | 29,064 / 10,588 · 13,179 | 7.6 ms | 11.8 ms |
| census pop_total ADM2, 3 sex columns | — | 2.8 ms | 3.8 ms |
| `/v2/indicators/census24.pop_total/map` ADM3 (for comparison) | — | 2.7 ms | 3.4 ms |
| `/v2/r/20261121.4/frame` WDI GDP national 2015–2023 (9 cols, pinned) | — | 3.5 ms | 4.0 ms |
| `If-None-Match` revalidation (304, no query) | 0 | 0.31 ms | — |

`/v2/frame` equals `/v2/indicators/census24.pop_total/map` at ADM2 (25/25) and ADM3 (339/339), and the Arrow
form equals the JSON bit for bit (2,373 cells). The ADM4 island-wide target of 10 §3.1 (p50 25 ms) is not
measurable yet: no indicator is loaded at ADM4 (census is ADM2/ADM3, WDI national).

### Decision note: WP-26 spec decisions and deviations

- *Client extensions of web-v2* (`web-v2/src/lib/map-spec.ts`). **Promoted into `/1`**: `blend: "overlay"`
  (the reference renderer draws every layer MapLibre-native with custom-layer blending and has the backdrop
  pass; a renderer without one draws it as `normal` and says so); named tones `ldl.tone.<name>` as colours
  (registry in `ramps.json`; unknown tones are errors); basemap `ldl-auto` (follow the viewer's scheme);
  `features.time: {field, match}` (was `x-time`); numeric categorical mappings (width/size by category) and
  `scale.range` (output px); `clamp` with open (`null`) ends. **Kept client-only** (`x-*`, preserved,
  ignored by other readers): `x-relief` (raster shaded by the DEM), `x-casing` (stroke casing), `x-coast`
  (basemap coastline), `x-data` / `x-partial` / `x-moran` / `x-spearman` (mock analysis payloads).
- *Semantics fixed while validating real documents*: a period bound to the time cursor in a document
  without `time` shows the latest period (web presets do this); layer `opacity` is optional (default 1;
  10 §7's label layer has none); saved-map ids are `m_[0-9a-z_]{1,40}`.
- *Registry changes*: `ldl.div.blue-orange` (dark) centre `#333b45` → `#2b3038` — the old centre was within
  1.7 L* of its neighbour under deuteranopia. The CVD test: monotonic L* with steps ≥ 2 for normal, protan
  and deutan; order only under tritan (ColorBrewer YlOrRd has a 1.3 L* tritan step).
- *Frames*: values are **Float64** in Arrow (10 §3.1 says Float32) so JSON and Arrow carry identical values
  (WDI GDP needs > 24 bits); codes/basis/status not dictionary-encoded yet. `support.unmatched` counts rows
  at the level whose unit is not in the boundary set. `format=parquet` → 400 (WP-09). Only the admin scheme.
- *Contract additions* (ui-contract-explore.md §4 to update): `shape: "unavailable"`, problem `NOT_SERVED`,
  notice `LICENCE_UNCLEAR` (census licence is `permission_unclear`), `ResolvedSource.versions[]`,
  `shape: "frame"` gains `by_zoom[]`.
- *Rendering*: web-v2 found every layer type works MapLibre-native with custom layers for blending (no
  deck.gl). Server outputs are unaffected (frames, value tiles and columnar points are renderer-neutral);
  ADR 0020 and 10 §4 should be revised by the doc owner.

## Tests and checks

```bash
cd engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# + the Postgres tests (create and drop throwaway databases): full cycle on a
#   synthetic country profile, 0001 → 0002 migration parity, WP-03 operations (roles,
#   approval → train, job queue crash/retry/cancel, scheduler, worker --once, status, event order)
#   and WP-26 frames (frame = map endpoint, periods × dims, release/dsv pins, A → B → A, limits):
LANKA_TEST_DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/postgres cargo test --workspace
#   (+ the range-version property test and the 1 M-row staging test; the < 10 s budget is
#   asserted in release builds: add --release)
# + parse the real census workbook from the v1 raw cache:
LANKA_TEST_CENSUS_DIR=$PWD/../../lanka-data-layer/foundry/data/raw/census-2024 cargo test -p lanka-ingest
```

## Declarative drivers and expectations (WP-07)

Design: [03 §2.2, §5.2](../docs/architecture-v2/03-ingestion.md#22-declarative-drivers--the-default-path),
[ADR 0008](../docs/architecture-v2/adr/0008-declarative-drivers-rust-only.md).

- **One tabular implementation** (`drivers/tabular.rs`) behind `csv`, `sdmx` (SDMX-CSV), `ckan`, `xlsx_table`,
  `html_table`, `git_files` and the table form of `http_json` (the WP-02 per-key form keeps WDI). The driver
  name only picks the default format; `discover.kind` (`static`, `keys`, `listing_page`, `url_pattern`,
  `ckan`, `git`) finds the files, `extract.format` (`csv`/`tsv` streamed, optionally from a zip member and
  row-filtered; `xlsx`/`xls` via calamine; `html` via scraper; `json` tables with entries and key templates)
  writes the L1 extract (cells or records).
- **Selector language** (`drivers/table.rs`): tables are assembled from the extract (header anchor, multi-row
  merged headers, `fill_down`, `rename` for printed header typos, stop/skip rows); each output maps rows to
  observations (`indicator`, `geo` root/code/name-matched with the reviewed alias list, `period` with chrono
  formats and seasons, `dims` with maps, `value`/`lo`/`hi` with number rules, `status` codes, wide → long
  `melt`, row `filter`) or to point features. Every value keeps its cell provenance and verbatim token;
  a duplicate observation key is an error that names both cells. Logic beyond that is a code connector.
- **Expectations** (`expect.rs`): every check of 03 §5.2 from `expect:` in a dataset or connector spec
  (`dataset:` scopes connector entries; default severity error for class A, warn otherwise). Evaluated after
  standardise; `sql` runs on the staged rows (`stage_obs`, read-only, 10 s, rolled back) and
  `revision_limit` sets the class-A gate. Results are in `dataset_versions.validation.expectations`.
  *Deviation:* an error fails a class-A run before staging (03 §7 says "version rejected, kept 180 days";
  the extract is kept, so the run can be re-standardised); B/C go to review with the failures.
- **`lanka connector test <source> [--from DIR] [--rows N] [--json] [--no-db]`** runs discover/fetch (or the
  fixture files, keyed by file stem), extract, a Parquet round trip and standardise in memory, prints the
  first rows and every expectation, and exits 1 on an error. `lanka connector list` lists drivers.
- **Fetching**: the user agent follows the crawler convention (`Mozilla/5.0 (compatible; lanka-data-layer-ingest/…)`)
  — CBSL's firewall rejects anything else. TLS is rustls, except for hosts the country profile lists under
  `fetch.legacy_tls_hosts` (or every host of a source with `access.tls: legacy`): those use the platform stack
  (OpenSSL in the image, Security.framework on macOS), chosen per request by host (`lanka_ingest::http`).
  `www.statistics.gov.lk` (IIS 8.5) offers only TLS 1.2 CBC suites (`ECDHE-RSA-AES256-SHA384`), which rustls
  does not implement; it answers a ClientHello without them with a TCP reset, which is why the failure
  shows as "Connection reset by peer" during connect rather than a TLS alert (headers and HTTP version
  play no part).

Sources on YAML: `worldbank-wdi` (WP-02), `dcs-agriculture` (HTML + 8 XLSX), `cbsl-exchange-rates`,
`imf-data`, `who-gho`, `un-sdg`, `ilostat`, `hdx-cod-ps-lka`, `wfp-vam`, `ourairports`, `geonames`,
`faostat` (bulk CSV: the REST API is down or token-only), `nuuuwan-lk-dcs` (`git_files`). Golden tests on
real government fixtures: `tests/drivers_golden.rs`; expectation pass/fail cases: `src/expect/tests.rs` and
`tests/pg_expect.rs` (`sql`).

## Adding a source

1. Write `registry/sources/<id>.yaml` and `registry/datasets/<id>.yaml` (validated by `lanka registry validate`);
   units must be in `registry/units.yaml` and every dimension needs a `registry/codelists/<codelist>.yaml`.
2. **Regular source** → `registry/connectors/<id>.yaml` on a driver (`http_json`, `csv`, `sdmx`, `ckan`,
   `xlsx_table`, `html_table`, `git_files`; see [Declarative drivers](#declarative-drivers-and-expectations-wp-07)).
   Try it with `lanka connector test <id>` (upstream) or `--from <fixtures>`, add `expect:` checks. No Rust.
   **Irregular source** → `crates/lanka-ingest/src/connectors/<id>.rs` implementing `Connector`
   (`discover` + `extract` + `standardise`; no DB writes, no network outside `discover`) and one line in
   `connector::code_connector`.
3. Every `ObsRow` carries `basis`/`status`/`flags` (and `lo`/`hi` for estimates), a locator, the verbatim
   token, a method and its extract row (`prov`); periods must be canonical for the indicator's frequency
   and geography codes are checked against the country profile. The pipeline handles archive, extracts,
   validation, staging and the publication gate; the worker schedules it and the train releases it.

## App plane I: snapshots, resolve, embed (WP-28)

Design: [12 §2](../docs/architecture-v2/12-enterprise-and-byod.md#2-two-planes), [10 §5–§6](../docs/architecture-v2/10-map-composition.md#5-storage),
[ADR 0023](../docs/architecture-v2/adr/0023-app-plane-optional-accounts.md). Crate `lanka-app` (mounted by `lanka-server`),
migration `0009_app_snapshots.sql`.

- **Switch**: `LANKA_APP_PLANE=on|off` (default **off**). Off: no app pool is opened, every `/app/v1` route is the
  enveloped `/v2` 404, `/v2` responses (bodies and cache headers) are identical to plane-on (unit test), and
  `/v2/oembed` answers 404 for map URLs. On: pool C (8 connections) as `lanka_app` — `LANKA_APP_DATABASE_URL`, or
  locally the server's own connection options with `SET ROLE lanka_app`; startup fails unless `current_user` is
  `lanka_app`. `/v2` handlers never touch pool C.
- **Schema/role** (0009): schema `app`, `app.map_snapshots` (10 §5; `created_by` gets its FK to `app.users` in
  WP-31; checks: id = `ms_` + 16 hex of `doc_sha256`, `doc.id = id`, ≤ 262,144 canonical bytes), role `lanka_app`
  (5 s statements). `app.apply_grants()` — run by `lanka db migrate` after `ops.apply_grants()` — gives `lanka_owner`
  ownership, `lanka_app` SELECT on `catalog/geo/data/serve` and DML on `app.*` only, and revokes everything on `app`
  from every other role. `LANKA_APP_PASSWORD` joins `lanka db set-passwords`.
- **Routes** (`/app/v1/openapi.json`, versioned apart from `/v2`):

| Route | Does | Cache |
|---|---|---|
| `POST /app/v1/maps/snapshots` | validate → `ms_<16 hex>` of the RFC 8785 content (no `id`, `meta.updated_at`, nulls) → 201 `{id, url, document, embed, created, bytes}` + `Location`; same content → 200 `created:false` (no rate token); withdrawn content → 410; > 256 KB → 413 with the `TOO_LARGE` problem; invalid → 400 with JSON-pointer problems | no-store |
| `POST /app/v1/maps/resolve` | the document + `resolved` (`lanka_mapdoc::resolve` on pool C) | no-store |
| `GET /app/v1/maps/{id}` | the stored document (`id` set); `ETag: "<id>"`; `m_…`/`@seq` → 404 until WP-31 | private, 1 h |
| `GET /app/v1/maps/{id}/resolved` | resolved document; `ETag: "<id>-<data_version>"`, 304 | private, 300 s (1 d when the pinned release exists) |
| `GET /app/v1/maps/{id}/package` | Frictionless `datapackage.json` (raw, not enveloped): one CSV resource per frame (+ `ldl:alternates` json/arrow) with licences and publishers; private, `link`/`internal`/restricted, boundary, inline, external and unserved sources go to `ldl:metadata_only` with the reason | as resolved |
| `GET /v2/oembed?url=&maxwidth=&maxheight=` | oEmbed 1.0 `rich` iframe to `/embed/m/{id}[@seq]` for `/m/…`, `/embed/m/…`, `/id/map/…` URLs of this origin; view-state params `v,t,sel,controls,interactive,lang,theme` carried; `format≠json` → 501 | public, 300 s |

  Errors: the envelope with a `Problem` payload `{type, title, status, detail, problems?, hints?}` (ui-contract §2),
  `no-store`. Every `/app/` response carries `X-Robots-Tag: noindex`. CORS: GET/POST from any origin, no credentials.
- **Rate identity** (`lanka_app::rate`): sha256(secret ‖ `YYYY-MM` ‖ address) — `LANKA_IP_HASH_SALT` (random per
  process if unset), rotated monthly, stored as `created_ip_hash`, never the address. The address is the right-most
  `X-Forwarded-For` entry when the TCP peer is loopback/private (host nginx), else the peer. Snapshot creation:
  in-memory token bucket `LANKA_SNAPSHOT_RATE` (default `5/h`) per identity → 429 + `Retry-After`. `Scope` /
  `Principal` are the stage-0 skeleton (anonymous, `Scope::Public`); `RateIdentity::Key` and `key_prefix` (shape of
  `ldl_live_…`) exist but no request gets a key identity until WP-31 verifies keys.
- **Expiry**: a daily task deletes anonymous snapshots unread for 24 months; reads touch `last_read_at` at most daily.
- **nginx** (`infra/v2/nginx/lanka-v2.conf`): `location ^~ /app/` (zone `lanka_app` 20 r/s burst 40, 429,
  `client_max_body_size 1m`, `proxy_cache off` + `proxy_no_cache`/`proxy_cache_bypass`) and
  `location = /app/v1/maps/snapshots` (+ zone `lanka_snap` 10 r/min burst 5); the two `limit_req_zone` lines join the
  http-level header. `infra/v2/nginx/test/run.sh` runs the real file in `nginx:1.27-alpine` (nginx is not installed
  on the dev Mac): `nginx -t`, then a stub upstream that marks every response `public, max-age=3600` — `/v2/` is a
  HIT with the stored body, `/app/` GET/POST get a fresh upstream response every time and no cache file has an
  `/app/` key, `X-Forwarded-For` ends with the client, the snapshot zone answers 429 past its burst.
  `UPSTREAM=host.docker.internal:8628` runs the cache checks against a live `lanka serve`. With `proxy_cache lanka`
  put into `/app/` the script fails (checked).

```bash
LANKA_APP_PLANE=on lanka serve --bind 127.0.0.1:8601
curl -s -X POST -H 'content-type: application/json' --data-binary @map.json localhost:8601/app/v1/maps/snapshots | jq .payload
curl -s localhost:8601/app/v1/maps/ms_c49007f840a3cf19/resolved | jq .payload.resolved
curl -s localhost:8601/app/v1/maps/ms_c49007f840a3cf19/package | jq '.resources[0].path'
curl -s 'localhost:8601/v2/oembed?url=https%3A%2F%2Flanka-data-layer.prabhavalabs.com%2Fm%2Fms_c49007f840a3cf19'
infra/v2/nginx/test/run.sh
```

Tests: `lanka-app` units (rates, buckets, hashing, forwarded-for trust, key shape, oEmbed URL parsing/escaping,
snapshot preparation); `lanka-server` units (plane off → every `/app/v1` route 404 and `/v2` byte-identical; plane
on → validation, 413, ids, OpenAPI, CORS; oEmbed); `crates/lanka-app/tests/app_plane.rs` on a throwaway database
(`LANKA_TEST_DATABASE_URL`): `lanka_app` gets 42501 for INSERT/UPDATE/DELETE/TRUNCATE on every table of
`catalog/geo/data/serve/ops` and for DDL, reads them, writes `app.*`; `lanka_api`/`lanka_ingest`/`lanka_recipe` get
42501 on `app.*` and have no USAGE; grants survive a re-migrate; the 10 §7 example (against a catalogue holding its
datasets) and the four web-v2 starter maps: same document twice → one id, the stored form hashes to its id, 300 KB →
413 and nothing stored, 5/h then 429 with Retry-After (re-posting stored content stays free), resolved example has
attribution and `ESTIMATE`, the package keeps private data metadata-only, withdrawal → 410, expiry.

### Decision note: WP-28 deviations

- *Default off.* 12 §2 makes `off` the mirror profile's default; it is the default everywhere until the compose file
  and `deploy.sh` provide `LANKA_APP_DATABASE_URL`/`LANKA_APP_PASSWORD` (not changed here), so a deploy of this binary
  cannot fail on a missing app role.
- *oEmbed never reads `app.*`*: a `/v2` response may not depend on app-plane state, so the title is generic
  ("Map — Lanka Data Layer") and an unknown snapshot id still gets an iframe (the embed route shows not-found). Only
  URLs of the deployment's origin (the public base: `LANKA_PUBLIC_BASE_URL`, else the profile's `site.base_url`; see 06 §2.2) are answered.
  Dataset/indicator/area URLs (06 §7) are still 404.
- *Not here*: `ops.jobs` INSERT for `upload`/`analysis` (WP-29/WP-32 add it with the job kinds), `app.usage_daily`
  flushing (WP-31), the `/m/…`/`/embed/m/…` pages (web track), `lanka takedown --map` (WP-31; withdraw by
  `UPDATE app.map_snapshots SET withdrawn = true` meanwhile).
- *Resolution of the real example*: on a database with only COD-AB, WDI and the census, the 10 §7 example resolves
  to boundaries + `PRIVATE_DATA` and `NOT_FOUND` problems (its exposure/flood/price datasets are not loaded); its
  attribution + `ESTIMATE` acceptance is therefore tested with a catalogue that holds them.

## Enrichment (WP-06)

Design: [04](../docs/architecture-v2/04-enrichment.md), [ADR 0010](../docs/architecture-v2/adr/0010-enrichment-methods-and-recipes.md),
[ADR 0005](../docs/architecture-v2/adr/0005-provenance-side-table-and-derivations.md). Migration `0006_enrichment.sql`
(`catalog.methods`, `catalog.recipes`, `catalog.recipe_inputs`, `data.derivations`, `data.version_inputs`).

```
registry/recipes/<id>.yaml ─(recipe.schema.json + lanka recipe check)─► catalog.recipes / recipe_inputs (global DAG)
train step 1 (before the lock): candidates = current release ∪ approved
  for recipe in topological order:  fingerprint = sha256(spec ‖ output dataset spec ‖ method@version ‖ tools ‖ input dsvs)
     derivation with that fingerprint whose version is current/approved/in review → reused (nothing runs)
     else lanka-enrich runner: steps → propagation → emit → QA gates → publish_observations (same validate/stage path)
          → derived gate → data.derivations + data.version_inputs → approved output joins the candidates
  the cut then releases official and derived versions together
```

- **Crate `lanka-enrich`**: `method.rs` (the `Method` trait, `MethodInfo`, rows, `Inputs`, `OutputSink`, contributors),
  `methods/{rollup,ratio,features_zonal,unit_metrics}.rs`, `zonal.rs` (array-kernel plug-in for WP-05), `recipe.rs`
  (spec, schema validation, type checks), `dag.rs` (Kahn, ties by id), `runner.rs` (execute, QA, fingerprint, explain),
  `propagate.rs`, `pg.rs` (inputs over pinned versions), `provenance.rs`. Publication is `lanka-ingest/src/derive.rs`
  (it owns the staging path); the train calls it through `train::run_train_with(…, Some(&DeriveEnv))`
  (`run_train` without an env skips recompute, so existing callers are unchanged).
- **Methods are a library**: they read only through `Inputs` (pinned Postgres versions, earlier steps, or `MemInputs`)
  and write to an `OutputSink` (`CollectSink` in memory), so `lanka-analysis` can call them outside a recipe run.
  Every output row lists its contributors (`Contrib { port, kind: obs|feature|unit|cells, idx, role, weight }`); the
  runner enforces 04 §5 from them after every step (basis = max, status suppressed > missing > low_reliability >
  provisional, flags OR of all propagating bits incl. 16 `INTERPOLATED_SPACE` and 17 `DISAGGREGATED`, null value for
  suppressed/missing, `Interval` methods must give bounds), and `explain` resolves them to obs_ids.
- **Library v1**: `rollup@1` (aggregation rule from the catalogue: sum, min, max, ratio_of_sums, weighted_mean; `none`
  never; `min_coverage` → `missing` below, `PARTIAL_COVERAGE` between; variances add for intervals), `ratio@1`
  (formula grammar `+ - * / ( )` over named terms, `select` dims, broadcast of undimensioned terms, `match_period:
  exact | {nearest: {max_years}}` with `DENOM_MISMATCH`, zero divisor → `missing`, delta-method intervals),
  `features_zonal@1` (count | length_km | area_km2 per unit and ancestors, kind/attr filters, a breakdown dimension with
  a total, flags pass through), `unit_metrics@1` (geodesic `area_km2` of the reference set, child counts).
- **Recipes** (`registry/recipes/`): `derived-admin-metrics` (`admin.area_km2`, `admin.child_count` ADM0–4, moved out of
  `cod-ab-lka-boundaries`), `derived-census-2024-rollups` (district → province for pop total/age-sex/ethnicity/religion,
  provincial sex ratio; QA: provinces sum to the printed national totals), `derived-per-capita-indicators`
  (`derived.per_km2{source_indicator=census24.pop_total}` ADM0–3, reading the two recipes above),
  `derived-facility-counts` (`derived.facility_count` and `derived.facility_per_10k` by `amenity_type`; waits for
  WP-04's features). QA checks: `sum_matches`, `non_negative`, `min_rows`, `no_missing`.
- **Derived gate** (04 §7): approved only when `stable`, QA passed, not the dataset's first version, same method MAJORs
  and parameters as the published derivation (and the class-A staging gate passes); else `in_review`
  (`lanka review approve`). A failing recipe or QA gate stages nothing, opens a `recipe_failed` incident and keeps the
  previous version; downstream recipes then see the old input and are reused.
- **Provenance**: derived rows get no `data.obs_provenance` row (their canonical row hashes without a locator; staging
  never classifies them as restated). `GET /v2/observations/{id}/provenance[?depth=1..3]` adds `derived`: derivation
  (recipe, methods, parameters, pinned input dsvs, fingerprint, git rev), formula and contributors re-computed by
  `explain` over the pinned versions; `depth` recurses into derived contributors. Attribution lists the inputs' sources.

```bash
lanka recipe check                    # schema + methods + params + ports + emit + DAG (also part of `registry validate`)
lanka recipe list                     # what the next train would do per recipe (dry run)
lanka recipe methods
lanka recipe run derived-census-2024-rollups [--force] [--dry-run]
lanka recipe explain <obs_id> [--depth 2]
lanka jobs enqueue recipe [<id>] [--force]   # worker job kind `recipe` (priority 150, weight 4)
lanka release cut                     # recompute stale recipes, then cut
```

Measured on the local database (debug build, loaded laptop; census 2024 + COD-AB ingested, 2026-11-22):

| Train | Recipes | Recompute | Result |
|---|---|---|---|
| 1 | admin-metrics (14,791 rows, 830 ms), census-rollups (216 rows, 414 ms) | 1.30 s | both in review (first version); per-capita, facilities `waiting_inputs` |
| 2 (after review) | admin + rollups reused; per-capita (374 rows, 410 ms) | 0.48 s | 2 released; per-capita in review |
| 3 | all reused | 38 ms | per-capita released |
| 4 | **nothing runs** (3 reused, facilities waiting) | 37 ms | `no_change`, train 76 ms |

Values: Western Province 6,117,341 (sum of 3 districts; provinces sum to the national total, worst 0.0 %), sex ratio
93.83, Sri Lanka 329.6 people/km² (66,079 km²). `lanka recipe explain` 0.2 s; the provenance endpoint with explain
~140 ms (debug; it re-runs the step over the pinned inputs).

### Decision note: WP-06 deviations

- *One writer per dataset*: a recipe owns its output dataset, so census ADM1 roll-ups are `rollup.census24.*` in
  `derived-census-2024-rollups`, not computed rows inside `census24.pop_total` as 04 §8.1 allows. The serve-time
  completion closes that gap (migration `0013_indicator_completes.sql`): a roll-up indicator declares
  `completes: census24.pop_total` in the registry (`catalog.indicators.completes`; validated: derived class, same
  unit, measure type, period type and dims), and `/v2/frame`, `/v2/indicators/{id}/map` and `/v2/areas/{code}`
  serve its rows under the completed id at a level where that indicator has none (`frame::source_at_level`), with
  `completed_from: "rollup.census24.pop_total"`, the roll-up's dataset (and dataset version in `meta`), `basis =
  computed` and the roll-up's `obs_id`s; the class stays the completed indicator's (`official` may carry computed
  roll-ups, 02 §4). Areas also keep the roll-up under its own id. `admin.area_km2`/`admin.child_count` moved to `derived-admin-metrics`
  (class `derived`) because a boundaries dataset version carries no observations.
- *`data.derivations`* has its own id; `version_id` is not unique, with `outcome new | unchanged`: a re-run whose output
  equals the published version records its fingerprint against that version, so it is reused next time. Failed runs
  are not recorded (incident only) and retry on the next train.
- *Tabular methods* read with one indexed SQL range scan per port and compute in Rust (pure, property-tested, usable
  in memory) rather than set-based SQL; at 14k units × a few hundred series this is milliseconds.
- *Partial recompute* (`changed_keys`) is in the context but no method uses it yet: staging needs the full row set, and
  unchanged rows already cost nothing (range versioning). `MethodCtx` has no `pg`/`scratch`/`tools` members yet (no
  method needs them); tool versions enter the fingerprint as an empty map until a method declares tools.
- *QA failure* stages nothing (04 §7 says the version is `rejected`; the effect — previous version stays — is the same).
  The class-A staging gate (> 1 % changed) also applies to derived versions, stricter than `max_revision_pct`.
- *Periods*: an emitted row is re-expressed in the output indicator's frequency (census snapshot `2024` → year 2024 for
  `derived.per_km2`).
- *Features* (`features_zonal`, WP-04): reads `geo.features` (0007) at the pinned version, hiding rows that an
  approved, unreleased version closes (`data.version_closes`, `obs_id` = `feature_id`). A features port bound to a
  dataset that is not `kind: features` is a `lanka recipe check` error. `derived-facility-counts` runs on
  `osm-pois-amenities` (kinds = the `amenity_type` codes); lines/polygons count whole in their `unit_key`.

### `grid_zonal@1` (WP-05 kernel)

`methods/grid_zonal.rs` wraps `lanka_grid::zonal::{zonal, rollup}` + `Acc::stat`, the kernel behind the raster
driver's `zonal:` entries (`lanka_ingest::grid::zonal_rows`). Parameters: `grid` (a recipe input: the raster dataset
owning the layer; a grid port may bind a `kind: indicators` raster dataset), `layer` (+ `layer_period`), `indicator`,
`period`, `stat` (`sum | mean | area_sum | max`), `cells` (`all | inside`, `land_mask` 1/2), `factor`, `levels`,
`dims`, `basis`. `PgInputs::grid_layer` reads the layer `.bin` and the reference set's `gnd_idx`/`land_mask` from the
store (`DeriveEnv.store_root`); rows are emitted for every unit and ancestor at `levels` with one `cells`
contributor. `tests/grid_pg.rs` checks it equals the driver's published zonal rows. The `share` statistic (needs the
`of` layers) and moving the raster specs' `zonal:` entries into recipes are follow-ups. The generic
`zonal::GridZonal<K: ZonalKernel>` adapter stays for custom kernels.

```bash
# tests (DB tests create and drop throwaway databases)
LANKA_TEST_DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/postgres cargo test -p lanka-ingest --test pg_enrich
cargo test -p lanka-enrich      # methods, propagation (proptest), expr, DAG/check, runner, fingerprint, zonal
```

## Features, OSM and vector tiles (WP-04)

Design: [05 §4, §9](../docs/architecture-v2/05-storage-and-performance.md#4-features-and-boundaries),
[06 §2](../docs/architecture-v2/06-serving.md), [ADR 0014](../docs/architecture-v2/adr/0014-pmtiles-and-http-caching.md),
[10 §3.3](../docs/architecture-v2/10-map-composition.md). Migration `0007_features.sql`; new crate `lanka-export`.

```
Geofabrik ─HEAD -latest─► 307 → sri-lanka-YYMMDD.osm.pbf (edition + fingerprint) ─GET─► raw/ (L0)
  osm_pbf extract: 3 parallel passes (relations → ways → nodes) ─► features extract (L1, 7 dp, all tags)
  standardise: registry/connectors/osm-geofabrik.yaml rules → 9 datasets (6 dp GeoJSON, typed attrs, flags)
  stage: hash rows → dsv; COPY (key, hash) → diff → COPY full rows of new/changed only
         → geo.features (vfrom = seq; rep point, cell_id, unit_key) + data.version_closes
  train: closes replaced rows (vto = seq − 1) ─► lanka tiles build ─► tippecanoe ─► tiles/<id>/<sha16>.pmtiles
```

```bash
lanka ingest run osm-geofabrik                      # HEAD → (new day?) GET → extract → 9 datasets
lanka ingest run osm-geofabrik --from DIR           # offline: the *.osm.pbf in DIR (archived like a download)
lanka review approve <boi-zones dsv> --by NAME      # boi-zones is class B
lanka release cut && lanka tiles build              # rebuilds only tilesets whose inputs changed
lanka tiles list
curl -s 'localhost:8601/v2/features/osm-pois-amenities?kind=hospital&within=LK11&limit=100'
curl -s 'localhost:8601/v2/features/osm-road-network?kind=motorway&format=geojson'    # bare FeatureCollection
curl -s 'localhost:8601/v2/features/osm-rail-network/node/1234567'
curl -s 'localhost:8601/v2/nearest?lat=6.9344&lon=79.8428&dataset=osm-pois-amenities,osm-rail-network&limit=5'
curl -s localhost:8601/v2/tiles | jq '.payload[] | {id, bytes, pmtiles}'
curl -s localhost:8601/v2/tiles/admin.json                                            # TileJSON 3.0.0
curl -s localhost:8601/v2/tiles/admin/10/739/492.mvt -o t.mvt                          # gzip MVT (204 = empty)
curl -s -H 'Range: bytes=0-126' localhost:8601/dl/tiles/admin/<sha16>.pmtiles          # the file, range requests
```

- **`geo.features`** (0007) is the observation model for geometry: `LIST (dataset_key)` partitions created at registry
  sync for `kind: features` datasets (`geo.ensure_feature_partition`, SECURITY DEFINER like `ensure_obs_partition`),
  immutable rows except the `vto` close (trigger `features_guard`), `feature_id = dataset_key << 40 | seq` from
  `catalog.datasets.obs_seq`, closes in the shared `data.version_closes`. Partial indexes `WHERE vto = open`: GiST
  `geom`, GiST `rep`, `(kind, unit_key)`, `(feature_key, vfrom)`, trigram `name_en` for `searchable: true`.
  `content_sha256` is the canonical row hash (key, kind, three names, 6-dp GeoJSON, canonical attrs, flags, entity).
  `geo.tilesets` + view `geo.current_tilesets` index builds. The 0001 table never held rows and is replaced.
- **Derived at insert, outside the hash**: `rep` (the point itself; midpoint along a line; GEOS-style interior point
  of a polygon — always on the geometry), `cell_id` = canonical grid cell of `rep` (Rust `lanka-core::grid`; the
  formula is not repeated in SQL), `unit_key` = deepest admin unit of the reference set containing `rep`, else the
  nearest (`unit_match` 0 contains / 1 ≤ 2 km / 2 beyond). Every published feature has both (310,629 / 310,629).
  Features whose `rep` is outside the grid or the country (ADM0 buffered by 2 km, a banded point-in-polygon index
  in Rust, `features::load_coverage`) are dropped with a counted warning (16 in the 2026-11-20 extract).
- **Change detection** for a daily refresh: (1) one HEAD of `-latest` — same dated file → `no_change`, nothing
  fetched; (2) same artifact sha → `no_change`; (3) same dsv → `unchanged` before anything is copied; (4) otherwise
  only `(key, hash)` pairs are COPY'd and diffed, and only new/changed features are sent and written. Gate as for
  observations (> 1 % changed or removed, or any feature without a unit → review).
- **Registry**: `fields: [{name, type: string|integer|number|boolean, unit, measure_type, enum}]` (only declared
  attributes are stored and served), `tiles: {tileset?, layer?, minzoom, maxzoom, attrs[], minzoom_by_kind, drop_densest}`
  and `searchable` on `features` datasets (schema `registry/schema/dataset.schema.json`, checks in `registry.rs`).
- **Drivers**: `osm_pbf` (`drivers/osm_pbf.rs`; pure Rust `osmpbf`, `LANKA_EXTRACT_THREADS` default ≤ 4;
  multipolygon/boundary relations assembled by node id into rings, inner rings to the smallest containing outer;
  route relations as multi-lines) and `geo_file` (`drivers/geo_file.rs`: GeoJSON FeatureCollections and text
  sequences, verbatim geometry in L1; shapefile/GPKG/KML need GDAL in the WP-08 sandbox). Both use one rule engine
  (`drivers/feature_rules.rs`: `match`/`not`/`types`, `geometry: point|line|area`, `kind` templates, `attrs` from
  tags or constants, `flags`, `require_name`, `name_contains`, `drop_unnamed_below`).
- **Tilesets** (`lanka-export::tiles`, `lanka tiles build`): `admin` (layers `adm0`…`adm4` zoom-gated like v1 with
  `pcode`, `level` and names on every level, plus `centroids` for labels/symbols; z0–12) and one per `features`
  dataset with `tiles:` (`key`, `kind`, `name`, declared `tiles.attrs`, `flags`; per-kind minzoom). GeoJSON text
  sequences streamed from Postgres → `tippecanoe` (`lanka-export::tippecanoe`: one typed op, fixed flags, runs in
  the work directory so no path enters the file) → `tiles/<id>/<sha16>.pmtiles` (named, immutable). A build is
  skipped when the input hash (spec + every input dsv + tippecanoe version + export format) is the newest build's.
- **Serving** (`lanka-server/src/features_api.rs`): `/v2/features/{dataset}` (bbox, within, kind, q, cursor, ≤ 2,000),
  `/v2/features/{dataset}/{key}`, `/v2/nearest` (GiST KNN then geodesic distance, ≤ 50, ≤ 50 km, ≤ 8 datasets),
  `/v2/tiles`, `/v2/tiles/{id}.json`, `/v2/tiles/{id}/{z}/{x}/{y}.mvt` (PMTiles read in process by
  `lanka-export::pmtiles`, directories cached, tiles returned gzip as stored; `ETag = sha16-z-x-y`) and
  `/dl/tiles/{id}/{sha16}.pmtiles` (single-range requests, `immutable`; nginx serves `/dl/` in production).

### Datasets (Geofabrik 2026-11-20, release 20261122.2)

| Dataset | Features | Kinds (count) |
|---|---|---|
| `osm-pois-amenities` | 24,888 | hospital 891, clinic 342, school 5,051, college 282, university 151, aerodrome 30, place_of_worship 9,344, bank 2,254, government_office 1,479, … (24 kinds) |
| `osm-road-network` | 237,699 | motorway 628 (`status` open 618 / under_construction 9 / proposed 1; 642 km of open carriageway), trunk 3,524, primary 6,501, secondary 1,626, tertiary 7,416, unclassified 32,210, residential 165,745, … |
| `osm-rail-network` | 1,601 | rail 1,308, station 261, halt 32 |
| `osm-waterways-waterbodies` | 15,581 | river 1,850, stream 5,372, canal 6,435, reservoir 734, lake 886, lagoon 177, river_area 127 |
| `osm-protected-areas` | 91 | national_park 21, nature_reserve 30, protected_area 40 (ways and relations) |
| `osm-parks-greenspace` | 2,378 | park 450, garden 921, playground 953, recreation_ground 54 |
| `osm-tourism-attractions` | 2,537 | waterfall 141, heritage 1,142, beach 396, viewpoint 390, museum 102, theme_park 40, nature 15, religious 11, park 3, other 297 |
| `osm-trails-cycling` | 25,831 | path 25,372 (6,422 km), hiking_route 26 (332 km), bicycle_route 2, cycleway 164 (44 km), footway (named) 256, bridleway 11 |
| `boi-zones` (class B, reviewed) | 23 | export_processing_zone 11, industrial_park 12 — every feature `UNOFFICIAL_GEOM` |

Kinds follow the codelists the count indicators will roll up on (`amenity_type`, `highway_class`, `waterway_type`,
`attraction_type`, `trail_type`); the roll-ups themselves are WP-06 recipes.

### Parity with v1 (`tests/osm_parity.rs`)

v1's layers are the 2026-04-19 Overpass pulls of ceylon-hub (`foundry/data/raw/geo`), with a bounding box that also
covers the southern tip of India and simplified geometry. The test counts both sides inside Sri Lanka's outline by
representative point, measures lines/areas after applying v1's simplification to v2, and re-implements v1's filters
independently over the raw extract (same snapshot):

| Layer | v1 | v2 | Δ | v1 length/area | Δ | v1 ids in v2 | same snapshot (v1 filter vs v2) |
|---|---|---|---|---|---|---|---|
| hospitals + clinics | 1,218 | 1,230 | +1.0 % | — | — | 97.5 % | 1,230 = 1,230 |
| schools / colleges / universities | 5,389 | 5,434 | +0.8 % | — | — | 98.9 % | 5,434 = 5,434 |
| aerodromes | 30 | 30 | 0 | — | — | 100 % | 30 = 30 |
| railway lines | 1,303 | 1,305 | +0.2 % | 15.95° | −0.0 % | 100 % | 1,308 = 1,308 |
| stations + halts | 293 | 293 | 0 | — | — | 99.3 % | 293 = 293 |
| water bodies | 1,556 | 1,586 | +1.9 % | 0.048°² | +1.8 % | 99.0 % | 1,586 = 1,586 |
| protected areas (ways) | 63 | 53 | −15.9 % | 0.022°² | −0.4 % | 84.1 % | 56 = 56 |
| roads motorway–secondary | 11,189 | 12,214 | +9.2 % | 121.5° | +7.0 % | 97.4 % | 12,266 = 12,266 |
| rivers | 2,359 | 1,846 | −21.7 % | 60.7° | −6.7 % | 77.3 % | 1,850 = 1,850 |

Asserted: the measure (count for points, length/area for lines/polygons) within 2 % for seven layers, and zero
missing/extra features on the same snapshot for all nine. Roads and rivers are **reported, not asserted** against
April: the test's breakdown attributes the difference to seven months of upstream edits (roads: 1,331 new way ids —
splits and tertiary→secondary/primary reclassification, only 194 v1 ids gone; rivers: 185 ways retagged `stream`,
28 `canal`, 323 merged away). Protected areas lost ways to relations upstream (area −0.4 %); v2 also reads the
relations (91 in total).

### Measured (release build, laptop shared with three other builds; 2026-11-22)

| Step | Time |
|---|---|
| First `lanka ingest run osm-geofabrik --from` (145 MB PBF → 310,629 features in 9 datasets) | 65 s (archive 0.3 s, extract 3.9 s, standardise 1.4 s, roads stage 42 s incl. unit assignment) |
| Re-run, same file | 1.4 s (`no_change` at the fingerprint) |
| `--force` re-run | 6.9 s, 9 × `unchanged`, 0 rows written |
| `lanka ingest restandardise osm-geofabrik` (offline from the extract) | 3.6 s, 0 rows written |
| Daily refresh at road scale (`pg_features::daily_refresh_writes_only_the_churn`, 240,000 features, +240 ~960 −240) | 1.24 s, 1,200 rows written (first load 5.7 s; identical delivery 0.19 s) |
| `lanka tiles build` (10 tilesets) | 43 s wall |

| Tileset | Layers / features | Bytes | Build (export + tippecanoe) |
|---|---|---|---|
| `admin` | adm0 1, adm1 9, adm2 25, adm3 339, adm4 14,043, centroids 14,416 | 14,636,424 | 1.6 + 19.3 s |
| `osm-road-network` | 237,699 | 35,561,312 | 1.7 + 9.5 s |
| `osm-waterways-waterbodies` | 15,581 | 9,459,701 | 0.2 + 2.2 s |
| `osm-pois-amenities` | 24,888 | 7,147,064 | 0.1 + 1.1 s |
| `osm-trails-cycling` | 25,831 | 4,155,376 | 0.1 + 1.2 s |
| `osm-protected-areas` | 91 | 1,119,225 | 0.5 s |
| `osm-parks-greenspace` | 2,378 | 771,529 | 0.3 s |
| `osm-tourism-attractions` | 2,537 | 754,634 | 0.3 s |
| `osm-rail-network` | 1,601 | 651,653 | 0.2 s |
| `boi-zones` | 23 | 37,340 | 0.1 s |

Endpoints (curl, 200 requests, p50 / p99): features hospitals in LK11 (100) 8.7 / 20.3 ms; roads in a Colombo bbox
(500) 13.1 / 21.9 ms; attractions `q=falls` 3.6 / 5.4 ms; nearest hospital 3.7 / 8.8 ms; MVT `admin/10/739/492`
(169 KB) 0.74 / 4.0 ms; MVT roads z12 0.55 / 0.95 ms; TileJSON 0.70 / 2.5 ms. Every tile read equals go-pmtiles'
bytes (`tests/pmtiles_reader.rs`).

### Decision note: WP-04 deviations

- *Unit assignment*: when WP-05's `grid.gnd_idx`/`grid.land_mask` exist for the boundary set, a representative
  point in an interior cell (`land_mask` 1, no unit boundary crosses it) takes the cell's unit (05 §4,
  `features::GridUnits`); every other point keeps the polygon path (`ST_Intersects` on the deepest level, KNN
  fallback). Lookups are never built by a features run; without them everything uses polygons.
- *Coverage drop instead of `UNIT_NULL`*: a feature whose representative point is not in the country (Geofabrik's
  polygon includes sea and the Palk Strait) is not the country's, so it is not published (counted in the run log
  and `validation.outside_coverage`); everything published has a unit.
- *Points for facilities*: POIs mapped as areas are published as their interior point (v1 used Overpass `center`);
  attractions too. Polygon footprints, when needed, come from a separate dataset rule set.
- *Expressways* are kind `motorway`/`motorway_link` of `osm-road-network` with `status` open / under_construction
  (`highway=construction` + `construction=motorway`) / proposed, not a separate dataset; `rda-expressway-programme`
  adds the official status later.
- *BOI zones*: OSM `landuse=industrial` polygons whose name says export processing zone / industrial zone or park
  (23 of the BOI's zones); class B (reviewed), `UNOFFICIAL_GEOM` on every feature, licence ODbL (geometry is OSM).
- *Tiles are built by the CLI* (`lanka tiles build` after a train), not yet by a `tiles` job the train enqueues
  (WP-09 adds export/tiles jobs). tippecanoe runs as a subprocess with a fixed argument list; WP-08 moves the same op
  into the sandbox. `--read-parallel` is off: it made builds non-deterministic.
- *`/v2/tiles/{id}/{z}/{x}/{y}` serves the current build* (floating, 300 s + SWR); the immutable form is the
  `/dl/tiles/{id}/{sha16}.pmtiles` file. TileJSON carries both.
- *Not in this package*: OGC API Features/Tiles (WP-13), columnar points `/v2/features/{ds}/points` (WP-27), the
  `geo_file` sources themselves (RDA A/B roads), feature-level `entity_id` conflation (WP-11), weighting `osm_pbf`
  ingest jobs as heavy in the job queue.

| Variable | Default | Purpose |
|---|---|---|
| `LANKA_EXTRACT_THREADS` | min(4, cores) | PBF decoding threads |
| `LANKA_TIPPECANOE` | `tippecanoe` on PATH | tippecanoe binary for `lanka tiles build` |
| `LANKA_TEST_OSM_PBF`, `LANKA_TEST_V1_DIR` | — | the parity test (`--release -p lanka-ingest --test osm_parity`) and the extract smoke test |
| `LANKA_TEST_PMTILES` | — | also compare a real archive with go-pmtiles in `tests/pmtiles_reader.rs` |

## Grid engine (WP-05)

Design: [05 §8](../docs/architecture-v2/05-storage-and-performance.md#8-grid-store),
[ADR 0013](../docs/architecture-v2/adr/0013-grid-arrays-and-high-frequency-table.md). Crate `lanka-grid`
(pure functions over arrays, no database), the `raster` driver + grid stage in `lanka-ingest`
(`drivers/raster.rs`, `grid.rs`), the grid routes in `lanka-server` (`grids.rs`), migration `0010_grid.sql`.

```
GeoTIFF (L0) ─► resample to the canonical 0.001° grid ─► grids/<layer>/<period>/<sha16>.bin (LKGRID01) + .tif (COG)
                split_sum | area_sum | centre_mean | centre_sample│  geo.grid_layers (+ serve, overview, style, stats)
                class_count | class_mode | slope_mean/share        ▼
grid-lookups: gnd_idx (u16) + land_mask (u8) ─► zonal pass (one scan, per-slot accumulators) ─► roll-up via anc_keys
                                                 ─► ObsRows ─► the usual validate → stage → gate → approve → train
```

- **Arrays**: `.bin` = 4 KiB header (`LKGRID01`, dtype, rows, cols, bbox, step, nodata, scale, offset, payload
  sha256, JSON with layer/period/unit) + little-endian payload at offset 4096. The server maps `serve: true` layers
  read-only (`lanka-grid/src/mmap.rs`, the one audited `unsafe`; workspace lint `unsafe_code = "deny"`). Every
  layer also gets a COG (512² deflate tiles, horizontal predictor for integers, overviews by the layer's
  `overview` rule, EPSG:4326 GeoKeys, `GDAL_NODATA`).
- **Lookups** (`lanka grid lookups`, built on demand by the first raster run): `grid.gnd_idx` = the deepest-level
  unit of the reference boundary set holding each cell centre, v1 `cell-lookup` semantics exactly (row-major,
  sticky last match, 0.03° bucket index with first-inserted-wins, 2 km nearest-boundary fallback), slots →
  `geo.units.key` in `geo.grid_lookup_units`. `grid.land_mask`: 0 none, 1 interior (centre inside a unit and no
  unit boundary crosses the cell), 2 boundary cell with centre inside, 3 near-shore (fallback), 4 boundary cell
  with centre outside. Boundaries are rasterised by grid traversal (6.98 M segments). 20 s for 11.34 M cells.
- **Zonal**: `lanka_grid::zonal::zonal(gnd_idx, layer, cols, row_areas, slots)` → per-slot `Acc {n, sum, wsum,
  area, max}`; `rollup(accs, parents)` adds each slot to its unit and every ancestor, so ADM1–ADM3 values are exact
  aggregates, not averages of averages. Stats: `sum`, area-weighted `mean`, `area_sum` (fraction × cell area),
  `share` (area over the sum of a set of layers), `max`, `density` (Σ v per km² of the unit's inside cells:
  `pop.grid_density`). Sums use every assigned cell (v1's population roll-up);
  means and areas use only cells whose centre is inside a unit (`land_mask` 1/2) so near-shore sea is not counted.
  Each pass checks leaves + unassigned = grid total. Cell area per row on the authalic sphere.
- **`raster` driver** (`registry/connectors/{worldpop,esa-worldcover,copernicus-dem,worldclim}.yaml`): `discover`
  lists files (`--from DIR` reads `DIR/<file name>`); the pipeline archives them (L0) and hands them to the grid
  stage instead of extract → standardise; one `records` extract lists the layers built (id, period, sha256, cells,
  sum) and every zonal observation's provenance points at its layer row (`method = spatial`, locator
  `{layer, period, stat, cells}`). Re-runs short-circuit on the fingerprint/artifact set; `--force` rebuilds, and
  identical content stages as `unchanged`.
- **Serving**: `/v2/point?lat=&lon=&layers=` (every served layer at the cell + units), `/v2/radius?lat=&lon=&km=&layer=
  [&period=]` (sum for count layers, else area-weighted mean; ≤ 50 km), `/v2/grids`, `/v2/grids/{id}`,
  `/v2/grids/{id}/tiles/{z}/{x}/{y}.png?period=` (256² RGBA; ramps from `spec/map/1/ramps.json` or class colours;
  204 outside the grid; `ETag` = layer sha + tile). `/v2/reverse` answers from `gnd_idx` when `land_mask = 1`
  (header `x-lanka-reverse: grid`), else the PostGIS polygon path (`polygon`). `/v2/health` `grids_mapped`.
  Layers are re-mapped on every catalogue reload.

```bash
lanka ingest run worldpop --from data/v2/inputs/worldpop          # or without --from: downloads (archived once)
lanka ingest run worldclim | copernicus-dem | esa-worldcover
lanka grid lookups | list
lanka grid parity --v1-dir DIR [--v1-geojson foundry/data/raw/cod-ab/lka_admin4.geojson]
lanka grid reverse-check --n 100000
curl -s 'localhost:8601/v2/point?lat=7.2906&lon=80.6337' | jq '.payload.values'
curl -s 'localhost:8601/v2/radius?lat=6.9271&lon=79.8528&km=5&layer=worldpop.population' | jq .payload.value
curl -s localhost:8601/v2/grids/worldpop.population/tiles/10/739/492.png -o t.png
```

`DIR` for `parity` holds `cells.csv` / `cell_lookup.csv` exported from the v1 `lanka.sqlite`
(`sqlite3 -csv lanka.sqlite "select cell_id, printf('%.17g', pop) from cells order by cell_id"`; `cell_id, gnd_pcode`
from `cell_lookup`). Tests: `lanka-grid` unit + property tests (zonal sum = grid total; COG/mmap/header
round-trips), `crates/lanka-grid/tests/v1_parity.rs` (set `LANKA_V1_PARITY_DIR` + `LANKA_WORLDPOP_TIF`),
`crates/lanka-ingest/tests/grid_pg.rs` (throwaway DB: raster run → lookups → zonal → no_change → grid reverse
= polygon reverse).

### Results (local, 2026-11-22, release build)

| Check | Result |
|---|---|
| WorldPop 2025 vs v1 `cells` | 5,468,498 positive cells, all 5,468,498 equal to v1 at f32 storage precision (max rel. diff 5.9e-8 = the f64 → f32 cast); none missing/extra; raster total 23,168,692.0 |
| `gnd_idx` vs v1 `cell_lookup` (same algorithm on v1's own input, `lka_admin4.geojson`, v1 cell set) | 5,468,498 / 5,468,498 identical (5,467,970 assigned + 528 unassigned) |
| `gnd_idx` vs v1 `cell_lookup` (production: reference set in PostGIS, loaded from the v1 layer export) | 5,467,811 / 5,467,970 equal; 159 differ (0.003 %) — all from the export's 6-decimal coordinate rounding: the same algorithm on `admin-adm4.geojson` gives the same 159 (centres within 1–4 cm of a boundary, or near-equidistant gap fallbacks) |
| `/v2/reverse` grid path vs polygon path, 100,000 random land points | 82,407 answered by the grid, all 82,407 equal to the polygon path (unit + ancestors + match); 17,593 fall back; ~1 µs vs ~0.6 ms per point |
| Zonal population | Σ ADM1 = Σ ADM4 = 23,168,691.79; + 0.21 in 1,179 cells > 2 km from any unit = the grid total 23,168,692.00 (checked every pass; property test over random grids) |
| Published (ADM1–ADM4, 14,416 units) | `pop.grid_total` 14,416 · `lc.class_area`/`lc.class_share` × 9 classes 259,488 · `terrain.{elevation_mean,slope_mean,steep_share}` 43,248 · `clim.wc_tavg`/`wc_prec` × 12 months + `wc_bio` × 19 619,458 |
| COGs | 60 / 60 valid (`validate_cloud_optimized_geotiff.py --full-check=yes`, GDAL 3.10.3 in `ghcr.io/osgeo/gdal`); GDAL reads the same values (WorldPop at Colombo 36.1177) |
| Zonal pass (one layer, 11.34 M cells, one core) | 29–43 ms (WorldPop 29, WorldClim 29–32, WorldCover 39–40, DEM 42–43) |
| Builds | WorldPop 0.03 s resample; WorldClim 43 layers 0.9 s; DEM 12 tiles (13 M px each, slope) 5.0 s; WorldCover 5 tiles (10 m) 3.7 s; lookups 20 s |
| Store | 59 layers: 1.23 GB `.bin` (WorldClim 975 MB of it) + 71 MB COG; L0 rasters 577 MB |

### Decision note: WP-05 choices and deviations

- *No GDAL at runtime.* Rasters are decoded in pure Rust (`tiff` crate + a raw reader for palette TIFFs it refuses)
  and resampled by our own rules, so WorldPop parity is exact and the distroless image needs no libgdal. GDAL (CLI or
  crate) was not usable here anyway (the Homebrew build fails to load), and is used only to *verify*: the COG
  validator and spot reads run in `ghcr.io/osgeo/gdal:ubuntu-small-3.10.3`. `lanka-extractd` gdal ops (WP-08) are
  needed only for formats beyond GeoTIFF (NetCDF/HDF: CHIRPS, ERA5).
- *Rasters skip Parquet L1*: the canonical array is the extract; the run still writes one `records` extract (the
  layer list) for provenance. `restandardise` of a raster source is refused with a pointer to `ingest run --force`.
- *Resampling* is centre-based everywhere (no interpolation): coarse counts split evenly over the cells whose centre
  the pixel holds (v1), coarse intensive values sampled at the cell centre, fine pixels averaged / counted by the cell
  holding their centre (WorldCover: exactly 144 pixels per cell). Slope: Horn on 30 m with per-row metric pixel width.
- *WorldPop is the 100 m constrained product* (connector 1.1.0, `area_sum`). Until 1.1.0 the connector loaded
  the ~1 km (30″) unconstrained UN-adjusted file with v1's `split_sum`, so every 8 × 8 (or 9 × 9) block of
  0.001° cells carried the same value and native-zoom value tiles (z10–z11) showed ~1 km squares, although the
  dataset card (`worldpop-pop-grid.yaml`) names the 100 m constrained product. `area_sum` redistributes each 3″
  pixel over the ≤ 2 × 2 cells it overlaps by overlap area: the total is conserved exactly, and a cell holds the
  population of its own ~110 m square (centre sampling at the 1.2 resolution ratio would alias: cells with 1 or 4
  pixel centres). The v1 parity check above refers to the 1 km file (`LANKA_WORLDPOP_TIF`); `split_sum` stays for it.
  Measured on a copy of the main database: 2,826,388 valid cells (constrained = nodata outside mapped settlement;
  1,724.8 persons max per cell), total 23,162,793.6 (the UN-adjusted 1 km total was 23,168,692.0); a z11 value tile
  over Colombo has 7,493 distinct values with a mean run of 2.0 px (one 0.001° cell ≈ 1.46 z11 px) against 356 and
  14.7 px from the 1 km file. `pop.grid_density` (zonal stat `density`, persons per km² of all the unit's inside
  cells, settled or not) covers ADM1–ADM4, so GN divisions have a modelled density while the census GN tables are
  not loaded; it is `third_party`/`estimated` and never completes `census24.*` or `derived.per_km2`.
  Consequences: `pop.grid_total` changes everywhere (constrained = zero outside mapped settlement; the national
  total is WorldPop's constrained total, not the UN-adjusted one), so the re-run goes through review.
- *Periods*: WorldPop `2025` (A); WorldCover `2021` (A; 2020 v100 is configurable but not loaded); DEM `2015` (P, end of
  the 2011–2015 acquisition); WorldClim normals dated by the window start (02 §5 multi-year rule): `1970-01…12` (M) and
  `1970` (P) for BIO1–19. The doc owner may prefer a climatology-period dimension.
- *Basis*: WorldPop zonal values are `estimated` (modelled input, 04 §2) without intervals (back-cast is WP-11); the
  others `computed`. Second runs after a rule change went through the review gate (> 1 % changed) and were approved.
- *Layers are current, not release-pinned*: `geo.grid_layers` is upserted per run (old `.bin`s stay in the store);
  release pinning of grids comes with WP-10/WP-27.
- *`gnd_idx` follows the v2 polygons*, not v1's full-precision file, so the grid path always agrees with the polygon
  path; exact v1 equality returns once COD-AB is ingested at full precision (ckan driver, WP-07).
- *WP-06*: `grid_zonal@1` (lanka-enrich) wraps `lanka_grid::zonal::{zonal, rollup}` + `Acc::stat` over a layer and
  the reference set's `gnd_idx`/`land_mask` (params `stat`, `cells: all|inside`, `factor`, `levels`); the raster
  driver's `zonal:` entries can move into recipes (follow-up; `share` is not in the method yet).
- Not done: `postal_idx` (needs GeoNames postal in v2, WP-07), `cell_area_m2` as a stored layer (computed per row),
  `min_cells`/`SMALL_N`, value tiles (WP-27), STAC.

## Layer data delivery (WP-27)

Design: [10 §3](../docs/architecture-v2/10-map-composition.md#3-serving-layer-data),
[ADR 0019](../docs/architecture-v2/adr/0019-frames-value-tiles-columnar-features.md),
[ui-contract-explore.md §4–§5](../docs/architecture-v2/ui-contract-explore.md). No migration.

- **Value tiles** (`lanka-grid/src/values.rs`, `GET /v2/grids/{id}/values/{z}/{x}/{y}.png?period=&v=<sha16>`):
  `ldl-v16` RGBA PNG, `q = R·256+G`, value = `q·scale+offset`, `q = 65535` no data, `B` = valid-cell share,
  `A` = 255; 204 outside the grid. Quantisation (`Quant::for_layer`): u8/u16 raw pass-through, i16 raw + 32768
  (`offset = offset − 32768·scale`) — **exact**; u32/f32 linear over the layer's `[min, max]`
  (`scale = (max−min)/65534`, `max_error = scale/2`; WorldPop 0.0076 persons). Pixels sample the cell under
  their centre at level `k = ⌊log₂(pixel width / cell width)⌋`: level 0 (native) at z ≥ 10, so z10/z11 read the
  cells themselves; z5–z9 read the **overview pyramid** (levels 1–7, 2ᵏ×2ᵏ cells, the layer's `overview`
  reducer over the native cells: mean/sum via exact `(Σ, n)` merges — `sum` is served as the mean per valid
  cell so one ramp holds at every zoom — max by merge, mode counted from native cells, ties → smaller). The
  pyramid is quantised (3 bytes/cell, 11.3 MB per canonical layer) and built on the first overview request
  (100–370 ms) then kept per layer sha256. `/v2/grids/{id}` gains `values` per mapped period
  (`values` URL template, `encoding, scale, offset, nodata, minzoom 5, maxzoom 11, max_error, exact, reducer,
  unit, sha16, categories` (q → class code for `mode` layers)); `/v2/grids` gains a `values` template.
  `v=<sha16>` matching the layer → `immutable`, else 300 s + SWR.
- **Columnar points** (`lanka-server/src/points.rs`, `GET /v2/features/{ds}/points?fields=&kind=&within=&bbox=&format=arrow|json`):
  one row per current feature at its representative point; Arrow IPC `lon, lat` Float32, `key` Utf8, then the
  requested fields (declared `fields:` plus `kind`, `name`; numbers Float32, others `Dictionary<Int32, Utf8>`),
  field metadata `ldl:field`, schema metadata `ldl:points` + `ldl:meta` (attribution travels). JSON form
  (`{lon[], lat[], key[], fields{}, field_info[]}` enveloped) ≤ 2,000 rows; > 250,000 → 413 pointing at the
  tileset. ETag `"<dsv>-<hash(query)>"`, 304 before any query, encoded bodies cached in process (96 MB).
- **Tilesets** (`lanka-export`): the `admin` tileset already carries `centroids` (one point per unit, `pcode`,
  `level`, names) and `promoteId`-able `pcode` on every level (WP-04). New **`dem`** raster-dem tileset
  (`dem.rs`, `lanka tiles build [--tileset dem]`): Terrarium PNG, z5–11 from the `terrain.elevation` layer
  (z11 = the cells, below = its mean pyramid; sea = 0 m; all-sea tiles omitted), written by a new PMTiles v3
  writer (`pmtiles::write`: single root directory, deduplicated contents, run-length entries; read back by our
  reader and `pmtiles verify`). `/v2/tiles/{id}/{z}/{x}/{y}.png|.mvt` sets the content type from the archive's
  tile type; TileJSON of raster sets carry `encoding`, `tileSize`, `format`.
- **The admin 204 of the merge smoke test**: every tile of the archive is served byte-for-byte as go-pmtiles
  reads it (1,794 z0–12 tiles over the island compared: 1,151 present, 643 genuinely absent = sea). The empty
  204s were **z13+ requests** against the z0–12 tileset from a client that did not read the TileJSON
  `maxzoom`. Requests outside the archive's zoom range now get **404** naming the range (+ `x-tileset-maxzoom`)
  instead of a silent 204, and resolved `frame`/`boundaries` shapes carry `tilejson` and `maxzoom`, so the
  renderer over-zooms. In-range empty tiles stay 204.
- **JRC flood hazard** (`registry/connectors/jrc-flood-hazard.yaml`, source now `active`): CEMS/JRC global river
  flood hazard v2.1.2 (3″ ≈ 90 m, depth in m), tiles ID186 `N10_E70` + ID194 `N10_E80` per RP10/20/50/100/200/500
  (12 files, 21 MB, archived once from `jeodpp.jrc.ec.europa.eu/ftp/jrc-opendata/CEMS-GLOFAS/flood_hazard/`;
  GDAL sparse files — absent chunks now read as nodata in `lanka-grid::geotiff`). New resamplers
  `centre_max` (deepest 90 m pixel per cell) and `centre_share` (wet share of the cell's pixels, nodata = dry),
  read chunk by chunk (`class_*`, `centre_mean`, `centre_max`, `centre_share` groups no longer read whole
  images). Published: grid layers `haz.flood_depth.rp{10,20,50,100,200,500}` (u16, 1 cm, `overview: max`,
  served) and `haz.flood_frac.rp*` (u8, not served); `haz.flood_area_share` (% of the unit's area in the
  modelled extent = area-weighted mean wet share over cells inside the unit × 100) at ADM1–ADM4 for all six
  return periods: 86,496 observations (54 + 150 + 2,034 + 84,258). Provinces RP10 → RP500: Eastern 15.9 → 23.3 %,
  Western 10.2 → 15.1 %, Central 1.8 → 2.1 %. Run: 9 s, 1.1 GB peak RSS.
- **Resolver** (`lanka_mapdoc::resolve`, still a pure function; `load_context` reads served grid layers,
  current tilesets, per-source feature counts (with `featureKind`/`within`) and declared fields): `grid`
  sources → `shape: "values"` (by layer id, or indicator + dims, dims values case-insensitive; `period` latest
  or a code) or `shape: "dem"` for `grid: "dem"`; `features` sources by the 10 §3.3 rules
  (`feature_shape`): ≤ 2,000 → `geojson` (`/v2/features/{ds}?…&format=geojson`), points ≤ 250,000 → `points`
  (Arrow URL with `kind,name` + requested fields), else `tiles` (tileset, `source_layers`, zooms) or
  `NOT_SERVED` when there is no tileset. Unknown grids/dims → `NOT_FOUND` with the available layers.
- **`grid_zonal@1`** gains `stat: share` with `of: [layers]` (area sum over the layer set's area sums, the
  driver's semantics); `MemInputs` binds `"<port>/<layer>"` for multi-layer tests.

```bash
lanka ingest run jrc-flood-hazard --from data/v2/inputs/jrc-flood-hazard   # or without --from: 12 downloads
lanka tiles build --tileset dem
curl -s localhost:8601/v2/grids/haz.flood_depth.rp100 | jq '.payload[0].values'
curl -s 'localhost:8601/v2/grids/haz.flood_depth.rp100/values/11/1478/984.png?period=2024&v=e3bd9c4226b91bc4' -o v.png
curl -s 'localhost:8601/v2/features/osm-pois-amenities/points?kind=school&fields=kind,name' -o schools.arrow
curl -s 'localhost:8601/v2/frame?indicators=haz.flood_area_share&level=3&dims=return_period=rp100'
lanka map resolve engine/crates/lanka-server/tests/browser/ref12.json | jq '.resolved.sources'
API=http://127.0.0.1:8601 LANKA=lanka engine/crates/lanka-server/tests/browser/run.sh   # 12-layer browser smoke
```

### Measured (release build, laptop, local stack `lanka_wp27`, 2026-11-22)

| Check | Result |
|---|---|
| Value tile decode, native zoom (`values::tests`, proptest 48 random grids × u8/u16/i16/f32) | integer layers: every pixel's `q` = the cell's raw value (i16 + 32768); f32 within `scale/2` |
| Overview = reducer (proptest, levels 1–4, mean/sum/max/mode, random nodata) | max/mode `q` exact; mean/sum within `scale/2`; `B` = valid share exactly |
| Value tiles (curl, 100 req) | z11 flood 1.3 / 2.6 ms (p50/p99), z8 WorldPop 1.1 / 2.6 ms; 3–100 KB per tile; first overview request per layer + pyramid build 100–370 ms |
| 250,000 points, Arrow (`points::tests`, kind + one numeric field, 11-char keys) | **6 ms** to serialise; 7.9 MB raw, **2.2 MB gzip** (the HTTP response is gzip-compressed) |
| `osm-road-network` 237,699 points, `/points` | cold 200–250 ms (Postgres read dominates), cached 75–85 ms incl. gzip; 7.6 MB raw / 2.84 MB gzip |
| `osm-pois-amenities` schools (5,051), Arrow | 289 KB, 2.3 / 4.4 ms |
| `dem` tileset | 507 tiles (335 distinct), 24.9 MB, built in 0.3 s; `pmtiles verify` OK; z11 tile 0.6 ms |
| Admin tiles vs go-pmtiles | 1,794 / 1,794 identical (1,151 tiles, 643 absent) |
| Browser smoke (`tests/browser/run.sh`, MapLibre 4.7.1, headless Chromium) | 12 / 12 layers draw data, 0 errors, idle in 0.7–0.9 s |

### Decision note: WP-27 deviations

- *Quantisation of f32 layers* is linear over the layer's range, not the `scale`/`offset` of `geo.grid_layers`
  (which is 1/0 for f32 and would round WorldPop to whole persons); the metadata states scale, offset and
  `max_error`. A log transform for count layers is a possible `/1` minor addition.
- *Pyramid in memory, not beside the array*: built from the mapped `.bin` on first use and cached per content
  hash (no store or migration change); writing it as a sidecar at ingest is a follow-up if start-up cost matters.
- *Native zoom*: level 0 is used from z10 (pixel ≈ 1.4 cells, nearest) — the declared `maxzoom` is 11.
- *250,000 points under 5 MB* holds on the wire (gzip 2.2–2.8 MB), not raw: Utf8 keys alone are ~4 MB at that
  count. *< 150 ms* holds for serialisation (6 ms) and for cached responses; a cold request is bound by the
  Postgres read (~200 ms for 238 k rows).
- *Grid layers are not release-pinned* (WP-05): `values` URLs carry the layer sha16 instead; a pinned document
  reads the current layer.
- *Value tiles carry one period*: a grid source bound to time resolves to the newest period.
- *Licence*: the JRC README says ODbL 1.0, the JRC copyright notice CC BY 4.0; the registry keeps CC BY 4.0
  (the notice is the legal text) — flag for the doc owner.
- *Not moved*: the raster specs' `zonal:` entries stay in the driver (moving them changes four datasets'
  publication path and needs recipe datasets); `grid_zonal@1` now has every statistic they use, including
  `share`.

## Analysis core and tier-1 methods (WP-29)

Design: [11](../docs/architecture-v2/11-analysis-engine.md), [ADR 0021](../docs/architecture-v2/adr/0021-analysis-engine.md),
[ADR 0022](../docs/architecture-v2/adr/0022-analysis-guard-rails.md), [ui-contract-explore.md §6–7](../docs/architecture-v2/ui-contract-explore.md).
Crates `lanka-stats`, `lanka-stats-wasm`, `lanka-analysis`; migration `0012_analysis.sql`; spec `spec/analysis/1/`;
method pages `docs/methods/analysis-*.md`.

```
POST /v2/analyses (ldl-analysis/1) ─► schema ─► RESOLVE pin → release, ids → dsvs, "latest"/"auto"/defaults explicit
   ─► PLAN support (coarsest native level; never down unless align: inherit), alignment per variable
   ─► fingerprint = sha256(JCS(resolved request − pin) ‖ method@ver+r<rev> ‖ engine r<rev> ‖ alignments ‖ sorted role|dataset|dsv|grid sha ‖ lanka-stats@ver)
   ─► stored?  yes → 303 to the stored bytes          id = an_<16 hex>, seed = first 8 bytes
   ─► ALIGN frames (identity) · rollup@1 · inherit · grid_zonal@1 · features_zonal@1 · exposure_overlay
   ─► GUARD (plan + frame guards; block → no estimates) ─► COMPUTE (lanka-stats, blocking pool 2 + queue 8)
   ─► PRESENT result doc + outputs ─► analysis/<id>/result.json (exact bytes) + analysis.results/result_inputs
```

- **`lanka-stats`** (no I/O, no async; native + `wasm32`): `classify` (quantile, equal, jenks, stddev, headtail,
  pretty, bivariate), `describe` (summary, type-7 quantiles, ranks, histogram, hexbin), `correlate` (Pearson,
  Spearman, Fisher-z intervals, adjusted p), `regress` (Householder QR OLS, HC3, leverage, Cook's D, VIF,
  Koenker–Breusch–Pagan, Huber IRLS), `compare` (Welch, Mann–Whitney, Hodges–Lehmann, Cohen's d, bootstrap),
  `effn` (Dutilleul effective n), `spatial` (kNN weights, Moran's I + seeded permutations), `rng` (SplitMix64
  counter stream keyed by (seed, stream)), `special` (incomplete beta/gamma, t/F/χ²/normal), `transform`.
  Transcendentals go through the pure-Rust `libm`, loops have a fixed order, no BLAS/SIMD dispatch — the same
  bits on every target.
- **`lanka-stats-wasm`**: a C-ABI module (no wasm-bindgen): `lsw_alloc/free`, `lsw_classify`, `lsw_bivariate`,
  `lsw_describe`, `lsw_correlate`, `lsw_regress`, `lsw_preview_relate`, `lsw_hexbin`; `js/lanka-stats.mjs` wraps
  it as the UI contract's `StatsKernel`. `build.sh` builds it, records the size and regenerates
  `tests/fixtures/wasm_bits.json` under node; `tests/wasm_parity.rs` requires the native kernels to give the same
  962 values bit for bit. The `extern "C"` exports are the second audited `unsafe` exception (after `mmap.rs`).
- **`lanka-analysis`**: `request` (types + embedded schemas, JSON-pointer problems), `pipeline` (resolve, plan,
  align over `lanka_core::frame` and `lanka-enrich` methods through an `Inputs` wrapper), `methods` (`describe`,
  `relate`, `regress`, `group_compare`, `exposure`, `zonal`, `classify`), `guards` (the registry: 25 codes, 21
  active in WP-29), `result` (guards, fingerprint, document), `engine` (sync executor, result store, plan),
  `bundle` (reproducible zip). `engine::evaluate` is the pure GUARD → COMPUTE → PRESENT step.
- **`lanka-grid::exposure`**: `exposure_overlay` — hazard ≥ threshold mask × exposed layer per unit, one pass
  over `gnd_idx`, exact roll-up (shares of sums, never averages of shares).
  In `lanka serve` the engine reads the hazard and exposed layers from the server's memory-mapped grid set
  (`Engine::with_mapped_grids`, served layers of the pinned dataset); others, and the CLI/tests, read the store
  file. Mapped cells are rounded to `f32` as the store path does, so the result is the same either way.
- **Migration 0012**: schema `analysis` (`results` — id checked against the fingerprint —, `result_inputs`,
  `aligned`, `spatial_weights`), role `lanka_analysis` (30 s statements; SELECT on catalog/geo/data/serve, DML on
  `analysis.*` only), `lanka_api` SELECT on `analysis.*`; `analysis.apply_grants()` runs after the other grant
  functions in `lanka db migrate`.
- **Server** (`lanka-server/src/analysis_api.rs`, own CORS with POST; the rest of `/v2` stays GET-only):

| Route | |
|---|---|
| `GET /v2/analysis/methods`, `/{id}` | roles, supports, params/estimates schemas, min n, guards, citations, page; the guard registry |
| `POST /v2/analyses` | `303` → `/v2/analyses/{id}` (`payload.computed` says whether it ran); 400 `{problems, hints}` (a period without data: `problems[].path` `/variables/<role>/period`, `hints.available_periods`; `/plan` answers the same); 422 over the sync budget (30 s estimate; async lane = WP-30); 503 busy or engine off |
| `POST /v2/analyses/plan` | id, resolved request, support + n_expected, alignment, cost (`sync`/`async`, expected ms), guards known in advance |
| `GET /v2/analyses/{id}` | the stored document inside the envelope, `immutable`, `ETag: "<id>"` (304) |
| `GET /v2/analyses/{id}/outputs/{output}?format=json\|csv\|arrow` | frames (`{codes, columns}`) and charts |
| `GET /v2/analyses/{id}/bundle` | zip: `result.json`, `outputs/*.json|csv`, `README.md`, `datapackage.json`, `prov.jsonld` |

  The engine connects as `lanka_analysis` (`LANKA_ANALYSIS_DATABASE_URL`, else the server's options +
  `SET ROLE`); outputs live in the store (`LANKA_STORE_DIR`, namespace `analysis/`). Either missing → the engine
  is off and those routes answer 503 (the method catalogue still works).

```bash
lanka db migrate
lanka serve --bind 127.0.0.1:8601            # "analysis engine on (/v2/analyses, role lanka_analysis)"
curl -s -X POST -H 'content-type: application/json' --data-binary @crates/lanka-analysis/tests/golden/builtup-share-vs-density-adm3/request.json \
     -o /dev/null -w '%{redirect_url}\n' localhost:8601/v2/analyses
curl -s localhost:8601/v2/analyses/an_868a0208aaadccac | jq '.payload | {guards: [.guards[].code], r: .estimates.pearson}'
curl -s 'localhost:8601/v2/analyses/an_868a0208aaadccac/outputs/bivariate?format=arrow' -o biv.arrow
crates/lanka-stats-wasm/build.sh             # WASM + size + parity fixture
```

### Acceptance evidence (2026-11-22, local `lanka_wp29`: COD-AB, census 2024, WorldPop, WorldCover, DEM)

| Item | Result |
|---|---|
| Reference fixtures | **Python** reference (R is not installed): numpy 2.5.5, scipy 1.18.1, statsmodels 0.15.0; `lanka-stats/tests/fixtures/gen_reference.py` → `reference.json`. 1e-8 (relative + 1e-12 abs) for special functions, describe, Pearson/Spearman (Anscombe I–IV, a tied 339-row set), OLS/HC3/leverage/Cook/VIF/BP/Huber (Stack loss, Anscombe I, a 400-row t₃ set; Longley at 1e-5 — condition number ~5e9), Welch/Mann–Whitney/HL/Cohen's d, class breaks (independent Jenks DP), Dutilleul M̂ against the explicit n×n matrices; **bit-exact**: seeded bootstrap (2 intervals) and Moran's I + 499 permutations against the ported RNG stream |
| Same id + bytes twice, after an unrelated release; new id after an input change | `tests/pg_analysis.rs` (throwaway DB, engine as `lanka_analysis`): twice → one id, identical bytes, second from the store; after a release of an unrelated dataset → same id, same bytes, and a store-bypassing recomputation gives the same id and numbers; a new version of x → new id, the old result still served |
| Guards | `src/tests.rs`: a positive and a negative fixture for each of the 21 codes active in WP-29 (a meta-test fails when a code has none); vocabulary scan of the registry and of every rendered message, summary and reading |
| Blocked → no estimates | MIN_N, PERIOD_MISMATCH > 5 y between time-varying inputs, LOW_COVERAGE < 30 %: `state: blocked`, `estimates`/`interpretation` null, no outputs, no `"p":` anywhere; override of PERIOD_MISMATCH recorded and cited; live: `relate` at ADM1 (9 provinces) blocked |
| Golden (substitute pair) | ESA WorldCover built-up share (`lc.class_share{lc_class=50}`, 2021) vs census population density (`derived.per_km2`, 2024, log), ADM3, island: `an_868a0208aaadccac`, r = 0.637 [0.569, 0.696], n_eff 62.2, p_adj 2.0e-8; ρ = 0.825; OLS log(y) ~ x b = 0.0621 [0.0527, 0.0715] (6.4 % per percentage point), R² 0.41; residual Moran's I 0.63 (p 0.001); guards ECOLOGICAL, PERIOD_MISMATCH (3 y), RESIDUAL_AUTOCORR, NOT_CAUSAL, MAUP (r 0.67 at districts), NO_CONTROLS. `tests/golden.rs`: `_offline` always (committed frames), `_live` with `LANKA_GOLDEN_DATABASE_URL`; **flood pair** (after the WP-27 merge, release 20261122.4): JRC `haz.flood_area_share{return_period=rp100}` vs the same density, ADM3, island: `an_61a6a1eac4e04ed3`, n 339, r = 0.159 [0.053, 0.261], n_eff 77.9, p_adj 0.16; ρ = 0.040 (p_adj 0.76); OLS b = 0.0151 (1.5 % per percentage point), R² 0.025; residual Moran's I 0.67 (p 0.001); r 0.03 at districts; guards ECOLOGICAL, RESIDUAL_AUTOCORR, NOT_CAUSAL, MAUP, NO_CONTROLS — a weak association that does not survive the spatial adjustment. Live comparisons ignore the recorded release (`pin`, `data_version`, citation), which the id leaves out |
| Native ≡ WASM | `classify`, `bivariate`, `correlate`, `regress`, `preview`, `hexbin`, `describe` on the fixtures: 962 values, identical bits (node 24 vs aarch64-apple-darwin) |
| WASM size | `lanka_stats_wasm.wasm` 133,418 bytes, **51,026 bytes gzip -9** (target ≤ 500 KB compressed) |

Timings (release build, laptop shared with other builds; `provenance.elapsed_ms` = align + compute; HTTP adds
~0.3 s of resolve queries):

| Request (11 §8) | n | Expected | Measured | Mode |
|---|---|---|---|---|
| `relate` at ADM3, island, 999 permutations | 339 | 30 ms | 97–235 ms (with the ADM2/ADM4 scale frames) | sync |
| `relate` at ADM4, one province (LK1), 999 permutations | 2,496 | 0.5–1 s | 0.42 s | sync |
| `relate` at ADM4, island, 999 permutations | 14,043 | 4–8 s | 7.5 s | sync (async lane WP-30) |
| `group_compare` on 50,000 points with rings | 50,000 | 0.3 s | kernels 85 ms; + 999 bootstrap medians 0.75 s (point support WP-27/33) | — |
| `exposure` for one district (LK21, slope ≥ 20° × WorldPop) | 20 | 50 ms | 0.62–0.70 s with both island layers read from the store per request; served layers are now read from the server's memory-mapped set (see below) | sync |
| stored result `GET /v2/analyses/{id}` | — | — | 8 ms | — |
| `hotspots`, `interpolate`, `gwr` | | | WP-30/33/37 | |

### Decision note: WP-29 choices and deviations

- *Fingerprint without `pin`*: the resolved request records the release it ran against, but the fingerprint
  uses the request minus `pin` plus the input dsvs, so an unrelated release keeps the id (11 §9's rule as
  intended). `provenance.data_version` is the release of the first computation; identical requests are served the
  stored bytes.
- *Time-invariant inputs* (migration `0014_indicator_time_invariant.sql`): the registry flag
  `indicators[].time_invariant` (`catalog.indicators.time_invariant`, `/v2` indicator metadata) marks terrain
  (Copernicus DEM, NASADEM, AW3D30, terrain zonal profiles), the JRC flood hazard maps and climate normals (WorldClim,
  station normals); a grid layer without an indicator inherits it when all of its dataset's indicators carry it.
  `PERIOD_MISMATCH` lists such inputs as `(time-invariant)`, reports the full gap (`detail.years`,
  `detail.time_invariant`, `detail.years_time_varying`) and blocks only when the time-varying inputs alone are more
  than 5 years apart; that block carries the action `{"request": {"override": ["PERIOD_MISMATCH"]}}` (the documented
  acknowledgement — no separate `params.allow_period_gap`). Results list `inputs[].time_invariant`; the flag enters
  the fingerprint (`|static`) and `ENGINE_REVISION` is 2.
- *Revisions in the fingerprint* (after the partial-correlation change was served from results stored by the
  previous code): `MethodInfo.revision` (`relate` r2: `estimates.partial`; `group_compare` r2: effective-n
  significance; the others r1) and `lanka_analysis::ENGINE_REVISION` (shared alignment/guards/presentation) enter the
  fingerprint and the document (`method.revision`, `provenance.engine_revision`). Bump the method's revision when
  its output changes for unchanged inputs, the engine revision when shared code does. `pin: "latest"` is resolved
  from `data.current_release` in the database, not the server's catalogue snapshot, so a request that arrives
  before the catalogue reload after a release still fingerprints against the new release. `tests/pg_analysis.rs`:
  release that changes an input → new id (also before the reload); unrelated release → same id and bytes; method
  revision bump → new id, recomputed. Golden ids changed with the fingerprint (estimates, n and guards unchanged):
  `an_868a0208aaadccac` (built-up), `an_61a6a1eac4e04ed3` (flood).
- *Exact bytes in the store*: `analysis/<id>/result.json` is what `GET` returns (jsonb would renormalise numbers);
  the row is the publication point.
- *Seed as a string*: `provenance.seed` (a u64, the first 8 bytes of the fingerprint) is a decimal string, since most
  seeds exceed 2^53 and a JSON number would reach a browser rounded. The id, the fingerprint and the seed value are
  unchanged, so no revision bump; results stored with a numeric seed are served with it rewritten to the string
  (`result::upgrade_seed`, the id's own seed only). Golden `result.json` fixtures changed only in that field.
- *Sync only*: no async lane yet; requests estimated ≤ 30 s run inline on the 2 + 8 pool, above → 422. The
  island ADM4 `relate` (7.5 s) is therefore sync here; WP-30 moves > 2 s to the `interactive` lane.
- *Spatial pieces*: residual Moran's I and the effective-n adjustment use kNN weights / centroid distances (no
  contiguity weights until WP-30), so `RESIDUAL_AUTOCORR` and `p_adjusted` are live now; Lee's L, local
  statistics and `uncertainty: "mc"` are refused with a pointer to WP-30.
- *Supports*: admin units only. Grid/points supports, `point_aggregate`, `sample`, distance rings, `dasymetric`,
  `crosswalk_rebase` and `spatial_interp` answer 400 naming their package; `inherit` is explicit-only (quality D,
  `PSEUDO_REPLICATION`).
- *`group_compare` on areas*: the zone is a variable thresholded on the same support (e.g. steep-slope share ≥
  10 %) until point layers exist.
- *Classes*: Jenks above 2,000 values runs on an evenly spaced sample of the sorted data (the web client's rule
  with a larger cap).
- *Reference*: Python instead of R (no R on the machine); statsmodels/scipy implement the same estimators, and
  the RNG-dependent checks are exact because the stream is ported.
- *WASM without wasm-bindgen*: a raw C-ABI module keeps the bundle at 51 KB and avoids a generated JS glue
  layer; the cost is the hand-written `js/lanka-stats.mjs`.
- *Migration number*: `0012`; `0011` was reserved for WP-27, which shipped without a migration, so the sequence
  goes 0010 → 0012 (sqlx applies by version; verified on a fresh and on the main database).
- *Shared-file edits*: `engine/Cargo.toml` (3 members, 2 path deps, `libm`/`crc32fast`), `lanka-core/src/db.rs`
  (`analysis.apply_grants()` after migrate), `lanka-grid/src/lib.rs` (`pub mod exposure`), `lanka-store/src/lib.rs`
  (`Ns::Analysis`), `lanka-server/{Cargo.toml, src/lib.rs, src/features_api.rs}` (engine field + mount + its CORS;
  the test `AppState` literal), `lanka-cli/src/main.rs` (`LANKA_ANALYSIS_PASSWORD` for `set-passwords`).
- Not here: async jobs/events, quotas, `/app/v1/ws/{ws}/analyses`, `/recipe`, parquet/geojson/grid outputs, the
  aligned-column cache (`analysis.aligned` exists, unused), MCP/CLI (`lanka analyze`, WP-34), eviction job.

```bash
cargo test -p lanka-stats -p lanka-stats-wasm -p lanka-analysis           # kernels, fixtures, parity, guards, golden offline
LANKA_TEST_DATABASE_URL=postgres://…/postgres cargo test -p lanka-analysis --test pg_analysis
LANKA_GOLDEN_DATABASE_URL=postgres://…/lanka LANKA_STORE_DIR=$PWD/../data/v2 cargo test -p lanka-analysis --test golden
cargo test --release -p lanka-stats --test bench -- --ignored --nocapture # kernel timings
```
