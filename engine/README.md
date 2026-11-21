# engine — Lanka Data Layer v2 (Rust)

The v2 data platform: ingestion, Postgres/PostGIS store, REST v2 server and the `lanka` CLI.
Design and data model: [docs/data-platform-v2.md](../docs/data-platform-v2.md).

| Crate | What |
|---|---|
| `lanka-core` | canonical grid (pinned to the v1 test vectors), p-codes, periods, envelope, Postgres pool + embedded migrations, catalog snapshot, every read query |
| `lanka-ingest` | registry loader + JSON-Schema validation + sync, connector trait, raw archive (sha256), run tracking + alerts, validate → stage (COPY) → gate → publish/review, connectors (`worldbank-wdi`, `dcs-census-2024`, `hdx-cod-ab-lka`) |
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

# 2. schema + registry
lanka db migrate
lanka registry validate        # every registry/*.yaml against registry/schema/*.json
lanka registry sync            # mirror into catalog.* (ingest runs do this too)

# 3. ingest (order matters: boundaries first — other sources key to p-codes)
lanka ingest run hdx-cod-ab-lka --from ../lanka-data-layer/foundry/data/artifacts/layers   # v1 layer exports
lanka ingest run worldbank-wdi                 # live API, class A → auto-published
lanka ingest run dcs-census-2024               # live XLSX, class B → waits for review
lanka review list
lanka review approve <dsv> --by "<your name>" --note "checked against the report"
lanka ingest status

# 4. serve
lanka serve --bind 127.0.0.1:8601
curl -s localhost:8601/v2/areas/LK11 | jq '.payload.indicators[0], .meta.dataset_versions'
curl -s 'localhost:8601/v2/indicators/wdi.ny_gdp_mktp_cd/series?geo=LK&from=2020'
curl -s 'localhost:8601/v2/indicators/wdi.series/series?geo=LK&dims=series_code=SP.POP.TOTL'
curl -s 'localhost:8601/v2/indicators/census24.pop_total/map?level=3&parent=LK1&dims=sex=total'
curl -s localhost:8601/v2/observations/<obs_id>/provenance
curl -s 'localhost:8601/v2/reverse?lat=6.9344&lon=79.8428'
curl -s localhost:8601/v2/openapi.json

# CLI queries use the same core
lanka reverse 6.9344 79.8428
lanka area LK1103
```

`--from` on `hdx-cod-ab-lka` points at the v1 foundry's `foundry/data/artifacts/layers/` (gitignored; produced by `pnpm foundry run build`). The census connector reads statistics.gov.lk directly; pass `--from <dir>` with `a5-sex-age.xlsx`, `a6-ethnicity.xlsx`, `a7-religion.xlsx` to work offline.

### Environment

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | local dev URL | Postgres |
| `LANKA_REGISTRY_DIR` | nearest `registry/` above cwd | registry root |
| `LANKA_DATA_DIR` | `<repo>/data/v2` | raw archive (`raw/ab/cd/<sha256>`, gitignored) |
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

# + the end-to-end Postgres test (creates and drops a throwaway database):
LANKA_TEST_DATABASE_URL=postgres://lanka:<password>@127.0.0.1:55432/postgres cargo test --workspace
# + parse the real census workbook from the v1 raw cache:
LANKA_TEST_CENSUS_DIR=../../lanka-data-layer/foundry/data/raw/census-2024 cargo test -p lanka-ingest
```

## Adding a source

1. Write `registry/sources/<id>.yaml` and `registry/datasets/<id>.yaml` (validated by `lanka registry validate`).
2. Add `crates/lanka-ingest/src/connectors/<id>.rs` implementing `Connector` (`discover` + `parse`; no DB writes, no network in `parse`) and register it in `connector::connector_for`.
3. Every `ObsRow` needs a locator, the verbatim token and a method; the pipeline handles archive, validation, staging, the publication gate, releases and alerts.
