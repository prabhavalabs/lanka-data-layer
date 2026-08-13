# Lanka Data Layer

**Open geo-data infrastructure for Sri Lanka.** A fast, free, openly licensed API for Sri Lankan geographic and statistical data — **Lanka Data Layer** — plus a visualization platform built on top of it.

Query cities, administrative divisions, postal codes, population, reverse geocoding, elections, and points of interest for Sri Lanka. All spatial computation happens offline, ahead of time, so the API itself is just a fast, cacheable read.

---

## Why

Sri Lankan geographic and statistical data is scattered across census PDFs, government portals, OSM extracts, and one-off datasets. Developers who need "which district is this coordinate in?" or "population of this DS division by age" end up either paying for global APIs that know little about Sri Lanka below district level, or hand-rolling their own extracts.

Lanka Data Layer fixes this with one principle: **Sri Lanka's data is small enough to precompute everything.** The entire country is on the order of ~70K populated 1 km grid cells, ~30K named places, 330 DS divisions, and ~14K GN divisions. That fits in a single SQLite file. So instead of running expensive spatial queries per request, we do all the spatial work offline in a data pipeline and serve precomputed lookups — reverse geocoding becomes a single indexed read.

## What's in the box

| Package | Name | Purpose |
|---|---|---|
| [`foundry/`](foundry/) | `@lanka-data-layer/foundry` | Offline ETL pipeline: fetches every source, normalizes into a canonical p-code-keyed schema, emits build artifacts (SQLite database, lookup tables, vector tiles, downloads) |
| [`api/`](api/) | `@lanka-data-layer/api` | The Lanka Data Layer API: HTTP service serving the foundry's artifacts |
| [`web/`](web/) | `@lanka-data-layer/web` | Visualization platform: maps, charts, dashboards — the API's first consumer |
| [`shared/`](shared/) | `@lanka-data-layer/shared` | Shared TypeScript types: API contracts, p-code and grid conventions |
| [`infra/`](infra/) | — | Docker Compose, reverse-proxy config, deployment scripts |
| [`docs/`](docs/) | — | Architecture, data contract, source catalog |

Package-level documentation: [`foundry/README.md`](foundry/README.md) covers the ETL pipeline and how to add a new source; [`api/README.md`](api/README.md) covers endpoints, environment variables, and Docker.

## Data sources & credits

Everything this project serves originates from open data. This table is the honest, current list of what we use and under what terms. It's kept in sync with the dataset catalog served at `GET /v1/datasets` and the machine-readable source list in every build's `manifest.json` (see [Transparency & data lineage](#transparency--data-lineage) below).

| Source | What we use | License |
|---|---|---|
| [geoBoundaries](https://www.geoboundaries.org) | Administrative boundaries: country, 9 provinces, 25 districts (ADM0-ADM2) | [CC BY 3.0 IGO](https://creativecommons.org/licenses/by/3.0/igo/) |
| [OCHA / HDX — cod-ab-lka](https://data.humdata.org/dataset/cod-ab-lka) | DS divisions (339, ADM3) and GN divisions (~14,000, ADM4) with official p-codes | CC BY-IGO |
| [OCHA / HDX — cod-ps-lka](https://data.humdata.org/dataset/cod-ps-lka) | Population projections by admin unit, age bucket, and sex | CC BY-IGO |
| [OpenStreetMap](https://www.openstreetmap.org) contributors, via the Overpass API | Places, points of interest — hospitals, schools, stations, airports | [ODbL](https://opendatacommons.org/licenses/odbl/1-0/) — share-alike, see note below |
| [GeoNames](https://www.geonames.org) | Sri Lanka postal-code dump today; the place-name gazetteer with Sinhala and Tamil alternates is planned | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |
| Survey Department of Sri Lanka, via [nuuuwan/sl-topojson](https://github.com/nuuuwan/sl-topojson) | Electoral district and polling division boundaries | Open |
| Election Commission of Sri Lanka, via [nuuuwan/lk_elections](https://github.com/nuuuwan/lk_elections) | Presidential and parliamentary election results, 2015–2024 | Open |
| [Department of Census and Statistics](http://www.statistics.gov.lk) | 2012 census (ethnicity, religion); 2024 releases as published | Open |
| [WorldPop](https://www.worldpop.org) | ~1 km UN-adjusted gridded population, distributed onto the canonical grid | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) |

**A note on OpenStreetMap and ODbL.** OSM data is licensed under the Open Database License, which is share-alike: if you produce and redistribute a derivative database built from the OSM-sourced tables in this project (places, points of interest, and related layers), that derivative must also be released under ODbL, with attribution to OpenStreetMap contributors preserved. Querying the data through the API, or displaying it on a map, doesn't by itself trigger that obligation — redistributing the underlying data does. See the [ODbL summary](https://opendatacommons.org/licenses/odbl/1-0/) if you're unsure.

**Acknowledgements.** This project leans heavily on two communities in particular. The **OpenStreetMap Sri Lanka** contributor community has spent years mapping the country's places, roads, and infrastructure by hand — that patient, ongoing work is the backbone of our places and points-of-interest data. And the open-data projects maintained by **[nuuuwan](https://github.com/nuuuwan)** — [`sl-topojson`](https://github.com/nuuuwan/sl-topojson) and [`lk_elections`](https://github.com/nuuuwan/lk_elections) — did the hard, thankless work of turning Sri Lankan electoral geometry and Election Commission results into clean, usable open data. Without that curation, sourcing this project's electoral boundaries and results directly would have been far harder. Thank you.

## Transparency & data lineage

Every foundry build emits a `manifest.json` alongside the SQLite artifact, recording `data_version`, the fetch date of each upstream source, and a SHA-256 checksum for every artifact produced. Every API response also carries attribution: `meta.source` and `meta.data_version` travel with every payload that touches a dataset, and the full catalog — source, license, feature count — is queryable at `GET /v1/datasets`.

### Known limitations

We'd rather be upfront about the rough edges than hide them:

- **Admin-unit counts differ between sources.** COD-AB reports 339 DS divisions where other sources say 330-331; we serve COD-AB as published. Levels 0-2 geometry comes from geoBoundaries while levels 3-4 come from COD-AB, so boundaries can disagree slightly where the two sources differ.
- **2012 religion figures are rounded in the source.** The Department of Census and Statistics' 2012 ethnicity/religion tables are reproduced as published, rounding included.
- **Gridded population is a model, not a count.** The cells table distributes WorldPop's ~1 km modeled estimates uniformly across the fine grid cells each raster pixel covers — good for density visualization and radius sums, but not a source of truth for any individual 111 m cell.
- **Postal-vote election entities have derived names.** The Election Commission's result files for the postal-vote entities carry vote totals but no display name, so the foundry constructs one from the sourced electoral district name — it won't match a name that appears verbatim in the source.

If something you're building depends on any of the above, check `GET /v1/datasets` and the build's `manifest.json` for current status before relying on it.

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
                                                        │    api     │ ◄── third-party apps
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

See [`foundry/README.md`](foundry/README.md) for the full pipeline (steps, `--only` filtering, environment variables needed for a first run) and [`api/README.md`](api/README.md) for endpoint details and Docker.

## API preview

```
GET /v1/lookup?q=nugegoda&suggest=1      # universal search: names, postal codes,
GET /v1/lookup?q=6.9344,79.8428          # coordinates, and p-codes in one box
GET /v1/reverse?lat=6.9271&lon=79.8612   # point → GN/DS/district/province + postal
GET /v1/search?q=nugegoda&lang=si
GET /v1/postal/10250
GET /v1/postal?lat=6.9271&lon=79.8612
GET /v1/admin/LK1103?include=children,population
GET /v1/admin/LK1103/geometry            # boundary GeoJSON for map highlights
GET /v1/population?lat=6.9271&lon=79.8612&radius=5
GET /v1/population/grid?res=0.02         # density buckets for 3D map rendering
GET /v1/elections/pres-2024/results/EC-01
GET /v1/datasets
GET /v1/tiles/admin.pmtiles              # vector tiles, range requests
```

All endpoints return `{ success, message, payload, meta }`, where `meta` carries the data version and source attribution. Interactive documentation with a live playground for every endpoint ships in the web app under `/docs`, including a postal-code demo that answers and maps a query in one view.

## Development

- **Branches**: work happens on `feat/*` / `fix/*` branches off `develop`; `main` is production.
- **Commits**: `type: brief description` — `feat`, `fix`, `refactor`, `test`, `docs`, `chore`.
- **Monorepo**: pnpm workspaces; run any package's scripts via `pnpm --filter @lanka-data-layer/<pkg> run <script>`.

## Roadmap

- **Phase 0 — Data foundry**: source fetchers, canonical schema, SQLite + lookup + tile artifacts
- **Phase 1 — Lanka Data Layer API**: reverse geocode, search, postal, admin, population, elections, datasets; OpenAPI; benchmark suite
- **Phase 2 — Platform**: map explorer with vector tiles, election atlas with swing analysis, age pyramids, density surfaces, accessibility maps
- **Phase 3 — Community**: developer portal, bulk downloads, election-night updates, census 2024 integration, economy module

## Contributing

Issues and pull requests are welcome. Please open PRs against `develop`, not `main`, and follow the branch and commit conventions above (`feat/*` / `fix/*`, commit messages as `type: brief description`).

Data corrections are especially welcome — Sri Lankan open data is scattered and occasionally wrong. If you spot an error (a misplaced boundary, a stale figure, an incorrect Sinhala or Tamil name), please open an issue with a citation to the correct source so it can be verified and fixed at the foundry level.

## License

Code in this repository is MIT-licensed — see [LICENSE](LICENSE).

Data remains under each upstream source's own license, as listed in [Data sources & credits](#data-sources--credits) above and in each dataset's `license` field at `GET /v1/datasets`. Tables derived from OpenStreetMap (places, points of interest, and related layers) carry ODbL's share-alike terms. Whatever you build with this project's data, please preserve attribution to the original source.
