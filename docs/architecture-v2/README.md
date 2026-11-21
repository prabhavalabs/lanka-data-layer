# Lanka Data Layer — target architecture (v2)

Status: **design, not yet implemented**. Date: 2026-11-21.
Supersedes the open parts of [`../data-platform-v2.md`](../data-platform-v2.md) (the foundation that `engine/` was built against). Where the two disagree, this set wins; every disagreement is listed in [09-roadmap.md §2](09-roadmap.md#2-what-changes-in-the-existing-engine).
The v1 contract in [`../architecture.md`](../architecture.md) stays binding for `/v1` and for three things v2 inherits unchanged: the canonical 0.001° grid formula, p-codes as the only admin join keys, and ≤ 6 decimal places on emitted coordinates.

Markers used throughout: **[A]** = assumption not verified by measurement or against the upstream; each must be checked by the work package that first depends on it. Numbers without [A] were read from the repository (registry counts, migration, parity audit).

## What this is

One standardised, enriched, versioned, provenance-tracked data layer for Sri Lanka, built from ~174 sources / 263 datasets / 512 indicators, that anyone can query, download, self-host, extend or fork for another country. Two people must be able to run it; the design therefore prefers one database, one binary, one disk, and adds a moving part only where a named threshold is crossed.

## Document set

| Doc | Contents |
|---|---|
| [01-layers.md](01-layers.md) | The six lifecycle layers L0–L5, what is stored where, immutability, retention, reproducibility |
| [02-standard-model.md](02-standard-model.md) | **The standard**: identifiers, measure/dimension model, time, geometry, labels, licences, provenance, quality flags, conformance to external standards |
| [03-ingestion.md](03-ingestion.md) | Connector SDK, declarative drivers, document extraction, validation, review, scheduling, backfills |
| [04-enrichment.md](04-enrichment.md) | The enrichment module: method library, recipes, DAG, incremental recompute, uncertainty, labelling of estimates |
| [05-storage-and-performance.md](05-storage-and-performance.md) | Postgres schema evolution, indexes, grid store, tiles, caching, capacity plan, scale-out triggers |
| [06-serving.md](06-serving.md) | REST v2, OGC API, MCP, CLI, bulk downloads, versioned URLs, embeds |
| [07-operations.md](07-operations.md) | Compose topology, worker, status APIs, backups, security, sandboxing, CI/CD, tests |
| [08-openness-and-governance.md](08-openness-and-governance.md) | Licensing, contribution model, self-hosting and forking, versioning policy, takedown |
| [09-roadmap.md](09-roadmap.md) | Work packages WP-01…WP-25 and the keep/extend/rewrite list for `engine/` |
| [adr/](adr/) | One short record per major decision |

## Principles

1. **A number has one definition and one implementation.** Every transport (REST, OGC, MCP, CLI, exports) calls the same Rust query core; every dataset passes the same pipeline.
2. **Nothing is published that cannot be traced.** Every observation resolves to either a cell in an archived file or a recipe run over pinned input versions.
3. **Estimates never impersonate official figures.** A modelled value is a different indicator, carries `basis = estimated|forecast`, and ships with an interval or a quality class. This is enforced by validators, not by convention.
4. **Immutable content, movable pointers.** Raw bytes, extracts, dataset versions, distributions and releases are immutable and content-hashed. "Current" is a pointer.
5. **Codes, not names.** p-codes for admin units, namespaced codes for every other geography, codelists for every dimension, UCUM for units.
6. **Boring technology.** PostgreSQL + PostGIS, a filesystem, nginx, one Rust binary, three external C/C++ tools run in a sandbox (poppler, GDAL, tippecanoe) and one optional batch container (Valhalla).
7. **Country is configuration.** Everything Sri Lanka-specific lives in `country/lk/profile.yaml` and `registry/`; the engine has no `LK` literal.
8. **Open by construction.** Open-source engine, open written spec, standard formats, anonymous read access, no account anywhere.

## Decision summary

| # | Area | Decision | ADR |
|---|---|---|---|
| 1 | Primary store | PostgreSQL 18 + PostGIS 3.6 for all tabular, vector and catalogue data. No second database. | [0001](adr/0001-postgres-single-primary-store.md) |
| 2 | Object store | **Local filesystem**, content-addressed, behind a `BlobStore` trait. Scale-out backend is **Garage** (S3-compatible, single Rust binary). MinIO and SeaweedFS rejected. | [0002](adr/0002-filesystem-blob-store-garage-later.md) |
| 3 | Layers | Six layers: L0 raw → L1 extract → L2 standardised → L3 enriched → L4 release → L5 serving. L1 is a new persisted layer (Parquet extracts). | [0003](adr/0003-six-layers-persisted-extract.md) |
| 4 | Observation storage | One narrow `data.observations` table, LIST-partitioned by dataset, **surrogate integer keys**, **version-range rows** (`vfrom`/`vto`) instead of a full copy per version. | [0004](adr/0004-range-versioned-narrow-observations.md) |
| 5 | Provenance | Cell-level provenance moves to a 1:1 side table `data.obs_provenance`; derived rows resolve lineage through `data.derivations`. PROV-O on the wire. | [0005](adr/0005-provenance-side-table-and-derivations.md) |
| 6 | Standard model | SDMX information model (concept, codelist, measure, attribute) without an SDMX web service; UCUM units; two orthogonal flags `basis` and `status`; intervals `lo`/`hi`. | [0006](adr/0006-sdmx-like-model-basis-and-status.md) |
| 7 | Geography | `geo.units` holds every coded geography (admin, electoral, local authority, MOH, station). Admin p-codes stay bare; others are `scheme:code`. Boundaries are vintaged; one reference vintage. | [0007](adr/0007-coded-geographies-and-boundary-vintages.md) |
| 8 | Connectors | Rust only. **Declarative drivers** (YAML in `registry/connectors/`) for regular sources, code connectors for the rest. No TypeScript. | [0008](adr/0008-declarative-drivers-rust-only.md) |
| 9 | Documents | PDF/OCR/GDAL/tippecanoe run in a network-less sandbox sidecar. Class C LLM extraction is operator-run, never scheduled, and its output is a persisted, reviewed extract. | [0009](adr/0009-sandboxed-extraction-and-class-c.md) |
| 10 | Enrichment | Method library in Rust (`lanka-enrich`) + declarative recipes (`registry/recipes/`). Derived datasets are ordinary dataset versions with recorded inputs; recompute is fingerprint-driven. | [0010](adr/0010-enrichment-methods-and-recipes.md) |
| 11 | Jobs | One `lanka worker` process: scheduler tick + Postgres job queue (`ops.jobs`, `SKIP LOCKED`). No Airflow/Dagster/Temporal, no systemd timers. | [0011](adr/0011-postgres-job-queue-worker.md) |
| 12 | Releases | **Release train**: one scheduled release per day, intraday cuts (≤ 1/h) only for `live` datasets, plus manual cuts. Derived datasets are recomputed inside the train, so a release is always internally consistent. | [0012](adr/0012-daily-release-train.md) |
| 13 | Grid | Memory-mapped arrays + COG on the blob store; Postgres holds the index and zonal results. Sub-daily series get their own append-only table. | [0013](adr/0013-grid-arrays-and-high-frequency-table.md) |
| 14 | Tiles and cache | Pre-generated PMTiles per release (tippecanoe), served by nginx range requests and by `/v2/tiles/{z}/{x}/{y}`; choropleths join values client-side. HTTP ETag + nginx `proxy_cache`; immutable versioned URLs. No CDN. | [0014](adr/0014-pmtiles-and-http-caching.md) |
| 15 | Interfaces | REST v2 (OpenAPI 3.1) + OGC API Features/Tiles subset + MCP + CLI + bulk, all on one query core with two backends (Postgres, offline pack). | [0015](adr/0015-one-query-core-two-backends.md) |
| 16 | Monitoring | Emit only: `/v2/health`, `/v2/status*`, run-event feed. Webhook alerts and the heartbeat ping are removed from the engine. | [0016](adr/0016-status-apis-only.md) |
| 17 | Openness | Engine Apache-2.0, spec + registry metadata CC BY 4.0, data under per-source licences with four redistribution tiers; country profile makes the engine forkable. | [0017](adr/0017-licensing-and-country-profile.md) |

## The system on one page

```
                                         UPSTREAM  (~174 sources)
  APIs (WB, IMF, FAO, NSO…)   XLSX/CSV/HTML (CBSL, DCS…)   PDF (DCS, Treasury, MoH…)   OSM PBF   rasters   GTFS   git
        └──────────────────────────┴──────────────┬───────────────┴────────────────────┴─────────┴───────┴──────┘
                                                  │ HTTPS, conditional GET, per-host politeness
┌──────────────────────────────────────── HETZNER VPS  8 vCPU / 15 GB / 150 GB ──────────────────────────────────────┐
│                                                                                                                    │
│  lanka worker (1 process)                                   lanka-tools sidecar  (network: none, read-only rootfs) │
│  ┌───────────────────────────────────────────────┐          ┌──────────────────────────────────────────────┐       │
│  │ scheduler tick 60 s → ops.jobs (SKIP LOCKED)  │   UDS    │ lanka-extractd: pdftotext/pdftoppm, tesseract│       │
│  │ job kinds: ingest · recipe · release · export │◄────────►│ gdalwarp/gdal_translate, tippecanoe, pmtiles │       │
│  │            tiles · backfill · check           │          │ rlimits + timeouts per request               │       │
│  └───────┬───────────────────────────────────────┘          └──────────────────────────────────────────────┘       │
│          │                                                   valhalla (profile "batch", started by a recipe job)   │
│          ▼                                                                                                         │
│  L0 RAW ARCHIVE          L1 EXTRACTS              L2 STANDARDISED            L3 ENRICHED                           │
│  store/raw/<sha256>      store/extract/<sha256>   data.observations          same tables, kind = derived           │
│  bytes as fetched        Parquet: cells, rows,    geo.units / unit_geoms     written by recipes                    │
│  ops.artifacts           features + locators      geo.features               data.derivations (inputs, method,     │
│                          ops.extracts             store/grids/*.bin          params, code rev, fingerprint)        │
│     discover→fetch→archive ─► extract ─► standardise ─► validate ─► stage ─► gate ─► (review) ─► approved          │
│                                                                                   │                                │
│                                         ┌─────────────────────────────────────────┘                                │
│                                         ▼   release train (≤ 1/day, or manual)                                     │
│                         recompute derived datasets whose input fingerprint changed (topological order)             │
│                                         ▼                                                                          │
│  L4 RELEASE  data.releases (data_version YYYYMMDD.N) = immutable map dataset → dsv                                 │
│              store/datasets/<id>/<dsv>/…  (Parquet, CSV, GeoParquet, COG)   store/tiles/*.pmtiles                  │
│              store/releases/<data_version>/manifest.json · catalog.jsonld · datapackage.json · stac/ · packs/      │
│                                         ▼   NOTIFY lanka_release                                                   │
│  L5 SERVING  serve.area_latest · serve.indicator_coverage · serve.search   (rebuilt for changed datasets only)     │
│              mmap grids · catalogue snapshot in RAM (Arc, hot-swapped)                                             │
│                                                                                                                    │
│  ┌──────────────── PostgreSQL 18 + PostGIS 3.6   127.0.0.1:55432 ────────────────┐                                 │
│  │ catalog.*  ops.*  geo.*  data.*  serve.*  stage.*                             │  roles: lanka_owner,            │
│  └───────────────────────────────────────────────────────────────────────────────┘  lanka_ingest, lanka_api (RO)   │
│                                                                                                                    │
│  lanka serve  127.0.0.1:8601   one query core ─┬─ /v2/*  REST (OpenAPI 3.1)                                        │
│                                                ├─ /v2/ogc/*  OGC API Features + Tiles                              │
│                                                ├─ /v2/tiles/*  MVT from PMTiles, PNG from grids                    │
│                                                ├─ /mcp  MCP (streamable HTTP, anonymous, read-only)                │
│                                                └─ /v2/health · /v2/status* · /v2/status/events                     │
│  api (Hono)   127.0.0.1:8600   /v1/*  unchanged until parity, then retired                                         │
│                                                                                                                    │
│  host nginx :443 (certbot) — the only public listener                                                              │
│     /v2/, /mcp → 8601 with proxy_cache + rate-limit zones      /dl/ → store/{datasets,releases,tiles} (sendfile,   │
│     /v1/ → 8600                                                        range requests, immutable cache headers)    │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
        │ restic (store) + pgBackRest (Postgres) → off-site SSH/SFTP target          ▲ polled by the user's own
        ▼                                                                            │ external uptime monitor
   browsers · REST/OGC clients · MCP clients · `lanka` CLI (online, or offline against a downloaded pack)
```

## What changes against the foundation, in one list

Kept as built: Postgres/PostGIS as the store; the four schemas; the registry format and its JSON Schemas; content-addressed raw archive; `discover → … → publish` stage order; publication classes A/B/C; `data.releases` as an immutable dataset → version map; `NOTIFY lanka_release`; the catalogue snapshot hot-swap; the envelope; grid arrays + COG; the three connectors' parsing logic and their tests.

Changed (details and rationale in the ADRs):

- `data.observations` is rebuilt: integer keys, `vfrom`/`vto` ranges, `basis` + `status` + `flags`, `lo`/`hi`, provenance moved to `data.obs_provenance`. Migration `0002` rewrites the ~10,000 existing rows.
- `geo.admin_units` becomes a view over `geo.units` + `geo.unit_geoms` (all coded geographies, vintaged geometry).
- `dsv` becomes an incremental, order-independent multiset hash, so append-only datasets do not rehash their history.
- A release is cut by a train, not by each publish; publishing a version marks it `approved`.
- A persisted extract layer (L1) sits between raw bytes and observations; the `Connector` trait splits `parse` into `extract` + `standardise`.
- `LANKA_ALERT_WEBHOOK`, `LANKA_HEARTBEAT_URL` and `ops.alerts` delivery are removed; `ops.incidents` and the status endpoints replace them.
- New crates: `lanka-store`, `lanka-grid`, `lanka-enrich`, `lanka-export`, `lanka-mcp`; `lanka-core::query` moves to `lanka-query` with a backend trait.
- New schema `serve` for release-time materialisations; new schema `stage` for unlogged load tables.
- Code licence MIT → Apache-2.0 (needs the owner's confirmation; see open questions).

## Open questions for the owner

Collected in [09-roadmap.md §5](09-roadmap.md#5-open-questions).
