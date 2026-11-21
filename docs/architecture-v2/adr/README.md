# Architecture decision records

Status of all records: **proposed** (2026-11-21), pending the owner's review of this design set.

| ADR | Decision |
|---|---|
| [0001](0001-postgres-single-primary-store.md) | PostgreSQL + PostGIS is the single primary store |
| [0002](0002-filesystem-blob-store-garage-later.md) | Filesystem blob store behind a trait; Garage when a threshold is crossed |
| [0003](0003-six-layers-persisted-extract.md) | Six lifecycle layers with a persisted extract layer |
| [0004](0004-range-versioned-narrow-observations.md) | Narrow observations with integer keys and version ranges |
| [0005](0005-provenance-side-table-and-derivations.md) | Provenance in a side table; derived lineage through derivations |
| [0006](0006-sdmx-like-model-basis-and-status.md) | SDMX information model without an SDMX service; `basis` and `status` split |
| [0007](0007-coded-geographies-and-boundary-vintages.md) | All coded geographies in one table; vintaged boundaries; one reference vintage |
| [0008](0008-declarative-drivers-rust-only.md) | Declarative drivers for regular sources; code connectors in Rust; no TypeScript |
| [0009](0009-sandboxed-extraction-and-class-c.md) | Sandboxed document tools; class C is operator-run and human-signed |
| [0010](0010-enrichment-methods-and-recipes.md) | Enrichment as a method library plus declarative recipes |
| [0011](0011-postgres-job-queue-worker.md) | One worker process with a Postgres job queue |
| [0012](0012-daily-release-train.md) | Releases are cut by a train, not by each publish |
| [0013](0013-grid-arrays-and-high-frequency-table.md) | Grid layers as memory-mapped arrays; sub-daily series in their own table |
| [0014](0014-pmtiles-and-http-caching.md) | Pre-generated PMTiles and HTTP caching in nginx; no CDN, no tile server |
| [0015](0015-one-query-core-two-backends.md) | One query core with a Postgres backend and an offline pack backend |
| [0016](0016-status-apis-only.md) | Monitoring: emit health, status and run events; no alert delivery |
| [0017](0017-licensing-and-country-profile.md) | Apache-2.0 engine, open spec, tiered data redistribution, country as configuration |
