# Lanka Data Layer

**Open geo-data infrastructure for Sri Lanka** — a fast, free, open-licensed API for Sri Lankan geographic data (the **Geopub API**) and a modern visualization platform built on top of it.

Query cities, administrative divisions, postal codes, population, reverse geocoding, elections, and points of interest for Sri Lanka — faster than commercial geocoders, with permissive caching and no billing.

---

## Why

Sri Lankan geographic and statistical data is scattered across census PDFs, government portals, OSM extracts, and one-off datasets. Developers who need "which district is this coordinate in?" or "population of this DS division by age" end up either paying for global APIs that know little about Sri Lanka below district level, or hand-rolling their own extracts.

Lanka Data Layer fixes this with one principle: **Sri Lanka's data is small enough to precompute everything.** The entire country is ~70K populated 1 km grid cells, ~30K named places, 330 DS divisions, and ~14K GN divisions. That fits in a single SQLite file. So instead of running expensive spatial queries per request, we do all the spatial work offline in a data pipeline and serve precomputed lookups — reverse geocoding becomes a single indexed read.

## What's in the box

| Package | Name | Purpose |
|---|---|---|
| [`foundry/`](foundry/) | `@geopub/foundry` | Offline ETL pipeline: fetches every source, normalizes into a canonical p-code-keyed schema, emits build artifacts (SQLite database, lookup tables, vector tiles, downloads) |
| [`api/`](api/) | `@geopub/api` | The Geopub API: HTTP service serving the foundry's artifacts |
| [`web/`](web/) | `@geopub/web` | Visualization platform: maps, charts, dashboards — the API's first consumer |
| [`shared/`](shared/) | `@geopub/shared` | Shared TypeScript types: API contracts, p-code and grid conventions |
| [`infra/`](infra/) | — | Docker Compose, reverse-proxy config, deployment scripts |
| [`docs/`](docs/) | — | Architecture, data contract, source catalog |

## Data

All data comes from open sources and stays open:

| Dataset | Source | License |
|---|---|---|
| Admin boundaries ADM0–ADM3 (country → DS divisions) | [geoBoundaries](https://www.geoboundaries.org) | CC BY 3.0 IGO |
| GN divisions (ADM4, ~14K units) | HDX / OCHA COD-AB Sri Lanka | CC BY-IGO |
| Population projections 2023 (age × sex, p-coded) | [HDX cod-ps-lka](https://data.humdata.org/dataset/cod-ps-lka) (OCHA) | CC BY-IGO |
| Gridded population raster | [WorldPop](https://www.worldpop.org) Sri Lanka 100 m | CC BY 4.0 |
| Electoral district / polling division boundaries | Survey Department of Sri Lanka (via nuuuwan/sl-topojson) | open |
| Election results (2015, 2019, 2024 ×2) | Election Commission of Sri Lanka (via nuuuwan/lk_elections) | open |
| Places, roads, rail, hospitals, schools, waterways, protected areas | [OpenStreetMap](https://www.openstreetmap.org) via Overpass | ODbL |
| Place names incl. Sinhala / Tamil | GeoNames (LK + alternateNames) | CC BY 4.0 |
| Postal codes | GeoNames postal dump + Sri Lanka Post directory | CC BY 4.0 |
| Census tables (2012; 2024 releases as published) | Department of Census & Statistics | open |

Every API response carries `meta.source` attribution. OSM-derived tables remain under ODbL share-alike.

## Architecture

```
     sources (geoBoundaries, HDX, OSM, GeoNames, EC results, WorldPop)
        │
        ▼
   ┌──────────┐     SQLite DB + cell→admin lookup + PMTiles + downloads
   │ foundry   │ ──────────────────────────────────────────────┐
   │ (offline) │                                               │
   └──────────┘                                               ▼
                                                        ┌───────────┐
                                                        │ Geopub API │ ◄── third-party apps
                                                        └───────────┘
                                                               ▲
                                                        ┌───────────┐
                                                        │    web     │
                                                        └───────────┘
```

Key design decisions:

- **All spatial work happens offline.** The foundry precomputes a fine grid over Sri Lanka mapping every cell to its GN division, DS division, district, province, postal code, and nearest city. At request time, reverse geocoding is one indexed lookup — no point-in-polygon.
- **SQLite at runtime, PostGIS never.** The API ships with a read-only SQLite file built by the foundry. Data updates are a file swap, not a migration.
- **p-codes are the join keys.** Every admin unit is keyed by its OCHA p-code (`LK1` … `LK1103` …), the stable identifier used across all tables.
- **Geometry is served as vector tiles** (PMTiles, HTTP range requests), never as multi-megabyte GeoJSON payloads.
- **Trilingual by default.** Names in Sinhala, Tamil, and English wherever sources provide them; `lang` parameter throughout the API.
- **Aggressive HTTP caching.** Responses are immutable per data release (`ETag` + long `Cache-Control`), so a CDN in front serves repeat queries without touching the origin.

Details: [docs/architecture.md](docs/architecture.md)

## Quickstart

Requires Node ≥ 22, pnpm, Docker.

```bash
pnpm install

# Build the data (downloads sources, builds SQLite + artifacts)
pnpm foundry run build

# Run the API against the built artifacts
pnpm api run dev

# Run the web platform
pnpm web run dev
```

## API preview

```
GET /v1/reverse?lat=6.9271&lon=79.8612
GET /v1/search?q=nugegoda&lang=si
GET /v1/postal/10250
GET /v1/admin/LK11?include=children,population
GET /v1/population?lat=6.9271&lon=79.8612&radius=5
GET /v1/elections/pres-2024/results/EC-01
GET /v1/datasets
```

All endpoints return `{ success, message, payload, meta }`. OpenAPI docs served at `/v1/docs`.

## Development

- **Branches**: work happens on `feat/*` / `fix/*` branches off `develop`; `main` is production.
- **Commits**: `type: brief description` — `feat`, `fix`, `refactor`, `test`, `docs`, `chore`.
- **Monorepo**: pnpm workspaces; run any package's scripts via `pnpm --filter @geopub/<pkg> run <script>`.

## Roadmap

- **Phase 0 — Data foundry**: source fetchers, canonical schema, SQLite + lookup + tile artifacts
- **Phase 1 — Geopub API**: reverse geocode, search, postal, admin, population, elections, datasets; OpenAPI; benchmark suite
- **Phase 2 — Platform**: map explorer with vector tiles, election atlas with swing analysis, age pyramids, density surfaces, accessibility maps
- **Phase 3 — Community**: developer portal, bulk downloads, election-night updates, census 2024 integration, economy module

## License

Code: MIT. Data: per-dataset licenses as listed above — attribution required, OSM-derived data under ODbL.
