# Architecture & Data Contract

This document is the binding contract between `foundry` (producer), `api` (server), and `web` (consumer). Changes here are breaking changes and need a version bump in the artifact manifest.

## 1. The canonical grid

All point→anything lookups run through one grid covering Sri Lanka:

```
bbox:  lat 5.800 … 10.000, lon 79.400 … 82.100   (WGS84)
step:  0.001° (~111 m)
rows:  4200   cols: 2700
cell_id = row * 2700 + col
row = floor((10.000 - lat) * 1000)
col = floor((lon - 79.400) * 1000)
```

- Valid cell_ids: `0 … 11,339,999`. Out-of-bbox coordinates → no cell → API returns 404 with `not_in_coverage`.
- Only land cells (and near-shore water cells) get rows in `cell_lookup`; a missing cell means "in bbox but not on land".
- The same formula must exist in exactly three places: `foundry/src/grid.ts`, `shared/src/grid.ts` (exported for api + web), and this document. Unit tests pin all corner cases (Colombo Fort, Point Pedro, Dondra Head, out-of-bounds).

## 2. SQLite schema (artifact: `lanka.sqlite`)

Built read-only by the foundry. The API never writes.

```sql
-- Build metadata: data_version (YYYYMMDD.N), built_at, per-source fetch dates
meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Admin hierarchy, p-code keyed. level: 0 country, 1 province, 2 district,
-- 3 DS division, 4 GN division. parent_pcode builds the chain.
-- p-codes for levels 1-4 are the official OCHA COD-AB codes (LK1, LK11,
-- LK1103, LK1103005, …). Interim GB:-prefixed ADM3 ids are retired once
-- COD-AB is ingested; a meta key records the pcode scheme version.
admin_units(
  pcode TEXT PRIMARY KEY, level INTEGER NOT NULL,
  name_en TEXT NOT NULL, name_si TEXT, name_ta TEXT,
  parent_pcode TEXT REFERENCES admin_units(pcode),
  area_km2 REAL, centroid_lat REAL, centroid_lon REAL
);

-- Simplified boundary geometry for instant map highlights (NOT for tiles —
-- tiles come from PMTiles). GeoJSON geometry only (no Feature wrapper),
-- simplified to ~0.0005° tolerance, 6 dp coordinates.
admin_geometry(
  pcode TEXT PRIMARY KEY REFERENCES admin_units(pcode),
  geojson TEXT NOT NULL
);

-- Postal codes serving each DS/GN division (levels 3-4), derived from
-- cell_lookup: share = fraction of the unit's land cells assigned to the
-- code. Served on /v1/admin/:pcode as `postal_codes`, dominant first.
admin_postal(
  pcode TEXT NOT NULL REFERENCES admin_units(pcode),
  code TEXT NOT NULL, share REAL NOT NULL,
  PRIMARY KEY (pcode, code)
);

-- Population by unit. sex: 'f'|'m'|'t'. age_bucket: '0-4' … '80+' | 'total'.
admin_population(
  pcode TEXT NOT NULL REFERENCES admin_units(pcode),
  year INTEGER NOT NULL, sex TEXT NOT NULL, age_bucket TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (pcode, year, sex, age_bucket)
);

-- Flexible per-unit stats (ethnicity, religion, later census 2024).
-- key examples: 'ethnicity.sinhala', 'religion.buddhist'
admin_stats(
  pcode TEXT NOT NULL, year INTEGER NOT NULL, key TEXT NOT NULL,
  value REAL NOT NULL,
  PRIMARY KEY (pcode, year, key)
);

-- Gridded population: WorldPop ~1 km (30 arc-sec) UN-adjusted raster,
-- distributed uniformly across the canonical fine cells each raster pixel
-- covers (pixel pop / covered-cell count). Point reads and radius sums are
-- both correct at raster granularity; only cells with pop > 0 are stored.
cells(cell_id INTEGER PRIMARY KEY, pop REAL NOT NULL);

-- The reverse-geocode table. One row per land cell. Everything above GN
-- level derives from admin_units.parent_pcode chain.
cell_lookup(
  cell_id INTEGER PRIMARY KEY,
  gnd_pcode TEXT NOT NULL,
  postal_code TEXT,
  nearest_place_id INTEGER,
  nearest_place_dist_m INTEGER
);

-- Named places for search (GeoNames LK + OSM cities merged, deduplicated).
places(
  id INTEGER PRIMARY KEY, source TEXT NOT NULL, source_id TEXT,
  kind TEXT NOT NULL,             -- city|town|suburb|village|...
  name_en TEXT NOT NULL, name_si TEXT, name_ta TEXT,
  lat REAL NOT NULL, lon REAL NOT NULL,
  population INTEGER, admin_pcode TEXT
);
-- FTS5 index over all three name columns for /search
places_fts (fts5: name_en, name_si, name_ta, content=places)

-- admin_pcode = the district-level (level 2) p-code, resolved from the
-- GeoNames dump's own district column by normalized name match (not from
-- lat/lon) — see foundry/src/steps/postal.ts. lat/lon are the dump's own
-- coordinates unless an admin unit inside that district has a name_en
-- matching the postal place name (normalized), in which case that unit's
-- centroid is used instead — GeoNames LK postal coordinates are frequently
-- inaccurate and occasionally badly misplaced into a neighboring district.
-- cell_lookup constrains its nearest-postal-code search per cell to the
-- codes mapped to the cell's own district (falling back to a country-wide
-- nearest search only for districts with zero district-mapped codes).
postal_codes(
  code TEXT PRIMARY KEY, name TEXT NOT NULL,
  admin_pcode TEXT, lat REAL, lon REAL
);

-- POIs from OSM: hospitals, schools, stations, airports, protected areas…
pois(
  id INTEGER PRIMARY KEY, category TEXT NOT NULL, kind TEXT,
  name_en TEXT, name_si TEXT, name_ta TEXT,
  lat REAL NOT NULL, lon REAL NOT NULL,
  admin_pcode TEXT, attrs TEXT    -- JSON: operator, beds, iata, …
);

elections(id TEXT PRIMARY KEY, type TEXT, year INTEGER, date TEXT, label TEXT);
election_parties(
  election_id TEXT, code TEXT, candidate TEXT, name TEXT, color TEXT,
  PRIMARY KEY (election_id, code)
);
-- entity kinds: 'ED' (EC-01), 'PD' (EC-01A), 'POSTAL' (EC-01P), 'NATIONAL' (LK)
election_entities(entity_id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT, ed_id TEXT);
election_results(
  election_id TEXT NOT NULL, entity_id TEXT NOT NULL,
  electors INTEGER, polled INTEGER, valid INTEGER, rejected INTEGER,
  winner_party TEXT, winner_votes INTEGER,
  results TEXT NOT NULL,          -- JSON: full per-party breakdown
  PRIMARY KEY (election_id, entity_id)
);

datasets(
  id TEXT PRIMARY KEY, title TEXT, category TEXT, description TEXT,
  source_name TEXT, source_url TEXT, license TEXT,
  feature_count INTEGER, download_path TEXT
);
```

Search notes: FTS5 `unicode61` tokenizer handles Sinhala/Tamil; ranking = FTS rank blended with `log(population)` boost (weights in `shared/src/search.ts`, ported from prior work).

## 3. Artifacts (foundry output → `foundry/data/artifacts/`)

| Artifact | Consumed by | Notes |
|---|---|---|
| `lanka.sqlite` | api | schema above, `PRAGMA journal_mode=OFF`, fully vacuumed |
| `tiles/<layer>.pmtiles` | web (+ api passthrough) | layers: admin (all levels, zoom-gated), electoral, roads, railways, water, pois |
| `downloads/<dataset>.<fmt>.gz` | public bulk downloads | GeoJSON (6 dp coordinates), CSV for tabular |
| `manifest.json` | api + web | `{ data_version, built_at, sources: [{id, fetched_at, license, url}], artifacts: [{path, bytes, sha256}] }` |

Raw source downloads cache in `foundry/data/raw/` (gitignored, re-fetchable). Builds must be deterministic for a given raw snapshot.

## 4. API conventions

- Base path `/v1`. Envelope: `{ success, message, payload, meta }`; `meta` always carries `data_version` and `source` attribution for the datasets touched.
- Errors: 400 validation, 404 `not_found` / `not_in_coverage`, 500 masked internals. Same envelope, `success: false`.
- `lang=en|si|ta` (default `en`) selects name fields; missing translation falls back to `en`.
- Caching: every GET sets `ETag: "<data_version>-<route-hash>"` and `Cache-Control: public, max-age=300, stale-while-revalidate=86400` (tiles: max-age=3600). Freshness is deliberately short: URLs carry no version, so clients must revalidate (cheap 304 via ETag) for data corrections to propagate. Never mark unversioned URLs `immutable`. Data release ⇒ new ETags everywhere.
- Pagination: cursor-based (`?cursor=`, opaque), never offset.
- Runtime: Node 22 + Hono + better-sqlite3 (sync reads are fine: the DB is read-only, queries are indexed point lookups). OpenAPI via zod schemas, served at `/v1/docs`.
- **Universal lookup** `GET /v1/lookup?q=`: classifies the query before searching —
  coordinate pair (`lat,lon` or `lat lon`, with optional °/N/E noise) → reverse geocode;
  5-digit number → postal code (exact, then prefix); p-code pattern (`LK\d+`) → admin unit;
  anything else → blended place + postal-name fuzzy search. Response is a typed list:
  `{ type: 'place'|'postal'|'admin'|'coordinate', score, payload }`, best match first.
  Suggestion mode `?suggest=1` returns lightweight rows (id, label, sublabel, lat, lon)
  capped at 10 for typeahead use.
- **Geometry for highlights** `GET /v1/admin/:pcode/geometry` serves the admin_geometry
  row as GeoJSON. Postal codes and places highlight as points (lat/lon already in
  their payloads).
- **Tiles**: the api serves `foundry/data/artifacts/tiles/*.pmtiles` as static files
  with range-request support at `/v1/tiles/<layer>.pmtiles`; clients use the pmtiles
  protocol, no per-tile endpoint.

## 5. Package responsibilities

- **shared** — grid math, p-code helpers, API types (zod schemas exported as types), search ranking constants. No runtime deps beyond zod.
- **foundry** — fetchers (one module per source, cached, resumable), transforms, emitters. CLI: `pnpm foundry run build [--only <step>]`. Never imports from api/web.
- **api** — serves artifacts. Imports shared only. No fetching, no spatial libs.
- **web** — imports shared only, talks to api over HTTP, loads tiles from PMTiles.

## 6. Deployment target

Single VPS (Docker Compose behind host nginx + Cloudflare orange-cloud). API container binds `127.0.0.1`; web ships as static files. Artifacts rsync'd (or baked into the api image) per data release.
