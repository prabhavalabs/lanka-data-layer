# ADR 0013 — Grid layers as memory-mapped arrays; sub-daily series in their own table

Status: proposed · 2026-11-21 · Detail: [05 §2.5, §8](../05-storage-and-performance.md#8-grid-store)

## Context

The canonical grid has 11,340,000 cells; 32 raster datasets are registered. Separately, a few sources are sub-daily (quarter-hourly generation, river gauges, station weather) and would not fit a date-keyed, release-pinned observation model.

## Decision

Grid layers stay as raw little-endian arrays (now with a 4 KiB self-describing header) plus a COG, on the blob store, indexed by `geo.grid_layers`, mapped read-only by the server. Zonal results are ordinary observations. Time-dense rasters are aggregated at ingest rather than stored per day. A registry rule forbids ADM4 × daily observations by default. Sub-daily values go to `data.obs_hf` (timestamptz, monthly range partitions, BRIN, append-only with a revisions table), are labelled `live`, and are rolled up into daily/monthly observations by a recipe.

## Consequences

- O(1) point reads and ~10^1 ms zonal passes with no database involvement.
- Row counts stay in the 10^8 range.
- `lanka-grid` needs one audited `unsafe` (mmap) module; workspace lint moves from `forbid` to `deny`.
- Live data has weaker guarantees (no dsv, no release pin) and says so in `meta`.

## Alternatives rejected

- **PostGIS raster / one row per cell**: ~5.5 M rows per layer, slower reads, bloated backups (the foundation's own measurement).
- **Zarr / TileDB**: more capable than needed for a fixed 2-D grid.
- **TimescaleDB for sub-daily**: an extension for one small table family.
- **Sub-daily values as versioned observations**: a release per reading.
