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
| `lanka-ingest` | registry loader + JSON-Schema validation + sync (profile, units, codelists, connector specs, schedules), connector SDK v2 (`discover` → `extract` → `standardise`), declarative drivers (`http_json`; `registry/connectors/*.yaml`), code connectors (`dcs-census-2024`, `hdx-cod-ab-lka`), L1 Parquet extracts, layered change detection, run tracking + the event feed, validate → stage (binary COPY + set-based diff, incremental dsv, range-versioned) → gate → approve/review, the job queue (`jobs.rs`), scheduler (`scheduler.rs`), release train (`train.rs`) and `lanka worker` (`worker.rs`), the WP-01 registry migration |
| `lanka-server` | axum `/v2/*` (data, health, status) + OpenAPI (utoipa) |
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
curl -s localhost:8601/v2/observations/<obs_id>/provenance
curl -s 'localhost:8601/v2/reverse?lat=6.9344&lon=79.8428'
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
| `RUST_LOG` | `info,sqlx=warn` | tracing filter |

## Tests and checks

```bash
cd engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# + the Postgres tests (create and drop throwaway databases): full cycle on a
#   synthetic country profile, 0001 → 0002 migration parity, and WP-03 operations (roles,
#   approval → train, job queue crash/retry/cancel, scheduler, worker --once, status, event order):
LANKA_TEST_DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/postgres cargo test --workspace
#   (+ the range-version property test and the 1 M-row staging test; the < 10 s budget is
#   asserted in release builds: add --release)
# + parse the real census workbook from the v1 raw cache:
LANKA_TEST_CENSUS_DIR=$PWD/../../lanka-data-layer/foundry/data/raw/census-2024 cargo test -p lanka-ingest
```

## Adding a source

1. Write `registry/sources/<id>.yaml` and `registry/datasets/<id>.yaml` (validated by `lanka registry validate`);
   units must be in `registry/units.yaml` and every dimension needs a `registry/codelists/<codelist>.yaml`.
2. **Regular API** → `registry/connectors/<id>.yaml` on a driver (today `http_json`; see
   `worldbank-wdi.yaml` and the header of `drivers/http_json.rs`). No Rust.
   **Irregular source** → `crates/lanka-ingest/src/connectors/<id>.rs` implementing `Connector`
   (`discover` + `extract` + `standardise`; no DB writes, no network outside `discover`) and one line in
   `connector::code_connector`.
3. Every `ObsRow` carries `basis`/`status`/`flags` (and `lo`/`hi` for estimates), a locator, the verbatim
   token, a method and its extract row (`prov`); periods must be canonical for the indicator's frequency
   and geography codes are checked against the country profile. The pipeline handles archive, extracts,
   validation, staging and the publication gate; the worker schedules it and the train releases it.
