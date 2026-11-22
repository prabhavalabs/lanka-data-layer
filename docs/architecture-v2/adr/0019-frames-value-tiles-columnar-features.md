# ADR 0019 — Layer data as frames, value tiles and columnar features

Status: proposed · 2026-11-22 · Extends [ADR 0014](0014-pmtiles-and-http-caching.md) · Detail: [10 §3](../10-map-composition.md#3-serving-layer-data)

## Context

ADR 0014 serves boundaries and features as PMTiles, choropleth values as a columnar response joined in the client, and grids as server-coloured PNG tiles. Composable maps need several indicators and periods per layer, restyling without requests, time animation, and large point sets for heat maps, bins and clusters.

## Decision

Three delivery shapes, chosen by the resolver:

1. **Frame** — `GET /v2/frame`: one code array plus one value column per (indicator, dims, period), JSON or Arrow IPC. The client writes all columns into MapLibre `feature-state` on the shared boundary tileset once; restyling and time stepping change a paint expression only.
2. **Value tiles** — `GET /v2/grids/{id}/values/{z}/{x}/{y}.png` in encoding `ldl-v16` (value quantised to 16 bits in R,G; valid share in B; opaque alpha), from the mmap arrays plus an overview pyramid. The colour ramp, range mask and breaks are shader uniforms in the browser. Server-coloured PNG tiles remain for simple clients.
3. **Features** — GeoJSON up to 2,000 features; **columnar points** (Arrow IPC) up to 250,000; PMTiles beyond that and for lines and polygons; server-side count grids (as value tiles) for density of very large sets.

Classification is computed by the same `lanka-stats` code in the browser (WebAssembly) and on the server.

## Consequences

- No per-indicator tiles, no dynamic `ST_AsMVT`, no tile server — ADR 0014 holds.
- Style changes and animation cost no request; nginx caches every shape.
- Arrow becomes a wire format the web client must decode (in a worker).
- An overview pyramid adds about a third to grid storage.

## Alternatives rejected

- **Values baked into vector tiles per indicator**: tile builds per indicator × period × level.
- **Server-coloured PNG tiles only**: every ramp or range change refetches; no client-side masking or blending of values.
- **COG range reads from the browser**: many small requests, a TIFF decoder in the client, no nginx tile cache.
- **Raw typed-array tiles**: need a custom decoder; PNG decodes natively and off the main thread.
- **GeoJSON for large point sets**: 5–10× the bytes and main-thread parsing.
