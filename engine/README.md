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
| `lanka-ingest` | registry loader + JSON-Schema validation + sync (profile, units, codelists, connector specs), connector SDK v2 (`discover` → `extract` → `standardise`), declarative drivers (`http_json`; `registry/connectors/*.yaml`), code connectors (`dcs-census-2024`, `hdx-cod-ab-lka`), L1 Parquet extracts, layered change detection, run tracking + alerts, validate → stage (binary COPY + set-based diff, incremental dsv, range-versioned) → gate → publish/review, the WP-01 registry migration |
| `lanka-server` | axum `/v2/*` + OpenAPI (utoipa) |
| `lanka-cli` | the `lanka` binary |

Toolchain: Rust 1.90+ (edition 2024). Crate versions are pinned exactly in `Cargo.toml`.

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
lanka ingest run worldbank-wdi                 # live API (YAML driver), class A → auto-published
lanka ingest run dcs-census-2024               # live XLSX, class B → waits for review
lanka ingest run worldbank-wdi                 # again: no_change at the fingerprint, nothing fetched
lanka ingest run worldbank-wdi --force         # skip the fingerprint/artifact short-circuits
lanka ingest restandardise worldbank-wdi       # offline: standardise → publish from the stored extracts
lanka review list
lanka review approve <dsv> --by "<your name>" --note "checked against the report"
lanka ingest status

# 4. serve
lanka serve --bind 127.0.0.1:8601
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

### Environment

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | local dev URL | Postgres |
| `LANKA_REGISTRY_DIR` | nearest `registry/` above cwd | registry root |
| `LANKA_COUNTRY_PROFILE` | the only `country/*/profile.yaml` beside the registry | country profile (ingest/sync; servers read `catalog.profile`) |
| `LANKA_GIT_REV` | `unknown` | git revision recorded on dataset versions |
| `LANKA_STORE_DIR` | `<repo>/data/v2` | blob store root (`raw/`, `extract/`, `tmp/`, …; gitignored). `LANKA_DATA_DIR` is still accepted for one release |
| `LANKA_FETCH_RPS` | `1` | request starts per second per upstream host (plus 2 in flight per host) |
| `LANKA_BIND` | `127.0.0.1:8601` | server bind (never 0.0.0.0) |
| `LANKA_ALERT_WEBHOOK` | — | POSTed JSON on every failed run |
| `LANKA_HEARTBEAT_URL` | — | GET after every successful run (external dead-man's switch) |
| `LANKA_LOG_JSON` | false | JSON log lines |
| `RUST_LOG` | `info,sqlx=warn` | tracing filter |

## Tests and checks

```bash
cd engine
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# + the Postgres tests (create and drop throwaway databases): full cycle on a
#   synthetic country profile, and 0001 → 0002 migration parity:
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
   validation, staging, the publication gate, releases and alerts.
