# ADR 0014 — Pre-generated PMTiles and HTTP caching in nginx; no CDN, no tile server

Status: proposed · 2026-11-21 · Detail: [05 §9–§10](../05-storage-and-performance.md#9-tiles)

## Context

No Cloudflare and no CDN. Maps need vector tiles for boundaries and features and a way to show ~500 indicators at ADM1–ADM4.

## Decision

Vector tilesets are built per changed dataset with tippecanoe into PMTiles, stored by content hash, served as static files with range requests and also at `/v2/tiles/{tileset}/{z}/{x}/{y}.mvt` (and OGC Tiles) by reading the PMTiles file. Choropleths are not tiled: the map endpoint returns columnar values joined to the `admin` tileset by p-code in the client. Grid layers are rendered to PNG tiles on demand and cached. Caching is ETag (derived from the dsvs touched) + in-memory 304s + nginx `proxy_cache` (2 GB, background revalidation, stale-while-updating); release-pinned URLs and hash-named files are `immutable`.

## Consequences

- No per-indicator tile generation and no tile-server process.
- A release invalidates only responses whose datasets changed.
- First request after expiry costs one in-memory 304.
- Global latency depends on distance to one VPS; acceptable for the primary audience, and mirrors are easy (`lanka pull`).

## Alternatives rejected

- **Dynamic `ST_AsMVT` / Martin / pg_tileserv**: CPU on the database host per uncached tile; another service.
- **Per-indicator choropleth tiles**: 500 indicators × periods × levels of tiles to build and store.
- **Varnish in front**: a second cache daemon; nginx already provides what is needed.
- **A third-party CDN**: excluded by constraint.
