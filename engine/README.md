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

## Grid engine (WP-05)

Design: [05 §8](../docs/architecture-v2/05-storage-and-performance.md#8-grid-store),
[ADR 0013](../docs/architecture-v2/adr/0013-grid-arrays-and-high-frequency-table.md). Crate `lanka-grid`
(pure functions over arrays, no database), the `raster` driver + grid stage in `lanka-ingest`
(`drivers/raster.rs`, `grid.rs`), the grid routes in `lanka-server` (`grids.rs`), migration `0010_grid.sql`.

```
GeoTIFF (L0) ─► resample to the canonical 0.001° grid ─► grids/<layer>/<period>/<sha16>.bin (LKGRID01) + .tif (COG)
                split_sum | centre_mean | centre_sample           │  geo.grid_layers (+ serve, overview, style, stats)
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
  `share` (area over the sum of a set of layers), `max`. Sums use every assigned cell (v1's population roll-up);
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
- *Periods*: WorldPop `2025` (A); WorldCover `2021` (A; 2020 v100 is configurable but not loaded); DEM `2015` (P, end of
  the 2011–2015 acquisition); WorldClim normals dated by the window start (02 §5 multi-year rule): `1970-01…12` (M) and
  `1970` (P) for BIO1–19. The doc owner may prefer a climatology-period dimension.
- *Basis*: WorldPop zonal values are `estimated` (modelled input, 04 §2) without intervals (back-cast is WP-11); the
  others `computed`. Second runs after a rule change went through the review gate (> 1 % changed) and were approved.
- *Layers are current, not release-pinned*: `geo.grid_layers` is upserted per run (old `.bin`s stay in the store);
  release pinning of grids comes with WP-10/WP-27.
- *`gnd_idx` follows the v2 polygons*, not v1's full-precision file, so the grid path always agrees with the polygon
  path; exact v1 equality returns once COD-AB is ingested at full precision (ckan driver, WP-07).
- *WP-06*: `grid_zonal` should wrap `lanka_grid::zonal::{zonal, rollup}` + `Acc::stat` and the `ObsRow` mapping in
  `lanka_ingest::grid::zonal_rows` (inputs: a grid layer + `gnd_idx`/`land_mask` of the pinned boundary set; params
  `stat`, `cells: all|inside`, `factor`, `levels`); the raster driver's `zonal:` entries then become recipes.
- Not done: `postal_idx` (needs GeoNames postal in v2, WP-07), `cell_area_m2` as a stored layer (computed per row),
  `min_cells`/`SMALL_N`, value tiles (WP-27), STAC.
