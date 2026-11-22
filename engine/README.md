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
  URLs of the deployment's origin (`LANKA_PUBLIC_URL`, else the profile's `site.base_url`) are answered.
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
  `derived-census-2024-rollups`, not computed rows inside `census24.pop_total` as 04 §8.1 allows; a serve-time union for
  levels the source does not print is a follow-up. `admin.area_km2`/`admin.child_count` moved to `derived-admin-metrics`
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
- *Features* (`features_zonal`, WP-04): reads the 05 §4 shape of `geo.features` (`dataset_key`, `vfrom`/`vto`,
  `feature_key`, `kind`, `unit_key`, `attrs`, `flags`, `geom`). Until 0007 lands the facility recipe waits for its input
  and `lanka recipe check` warns that `osm-pois-amenities` is not yet `kind: features`; the facility acceptance is proven
  by `tests/pg_enrich.rs` on a fixture table of that shape. Lines/polygons count whole in their `unit_key`.

### How WP-05 plugs `grid_zonal` in

Implement the pure kernel and register the adapter — no runner, recipe or Postgres code involved:

```rust
pub trait ZonalKernel: Send + Sync {                       // lanka-enrich/src/zonal.rs
    fn info(&self) -> &MethodInfo;                         // id "grid_zonal", params schema, basis, flags
    fn zonal(&self, values: &[f32], unit_idx: &[u32], n_units: usize, params: &Value) -> Result<Vec<ZonalStat>>;
}
// methods/mod.rs: Methods::builtin() gains Arc::new(GridZonal(GridZonalKernel)); `Inputs::grid(port)` returns a
// `GridInput { values, unit_idx, units, basis, flags }` (implement it on `pg::PgInputs` from lanka-grid's mmap arrays).
```

`GridZonal` emits one row per deepest unit with a `cells` contributor; coarser levels are a following `rollup` step.
`SumKernel` and its conservation property test (Σ units + unassigned = total) are the template.

```bash
# tests (DB tests create and drop throwaway databases)
LANKA_TEST_DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/postgres cargo test -p lanka-ingest --test pg_enrich
cargo test -p lanka-enrich      # methods, propagation (proptest), expr, DAG/check, runner, fingerprint, zonal
```
