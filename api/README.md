# @geopub/api

The Geopub API — an HTTP service that serves the foundry's read-only SQLite
artifact (`geopub.sqlite`). Node 22, [Hono](https://hono.dev), synchronous
`better-sqlite3` reads, `zod` validation. This package never writes to the
database and never fetches or computes spatial data itself — see
[`docs/architecture.md`](../docs/architecture.md) for the full contract
(grid formula §1, SQLite schema §2, API conventions §4).

## Running

```bash
pnpm install                # from the repo root
pnpm --filter @geopub/api dev     # tsx watch, reloads on change
pnpm --filter @geopub/api start   # single run, no watch
pnpm --filter @geopub/api test    # node:test against an in-memory fixture DB
pnpm --filter @geopub/api lint    # tsc --noEmit
```

The server ships and runs its TypeScript source directly via `tsx` — there
is no compiled `dist/`, matching `@geopub/shared`'s own no-build convention.

## Environment variables

| Variable          | Default                                    | Meaning                                                |
|--------------------|---------------------------------------------|---------------------------------------------------------|
| `PORT`             | `8600`                                      | HTTP port                                                |
| `GEOPUB_DB`        | `../foundry/data/artifacts/geopub.sqlite`   | Path to the built SQLite artifact, opened read-only      |
| `GEOPUB_TILES_DIR` | `../foundry/data/artifacts/tiles`           | Directory `/v1/tiles/:file` streams `*.pmtiles` files from |

If `GEOPUB_DB` doesn't point at an existing file, the process logs a clear
error and exits with status 1 instead of silently creating an empty database.
`GEOPUB_TILES_DIR` has no such check — the directory (or individual files in
it) may not exist yet if the foundry's tile-emission step hasn't run; missing
files are just a normal `404` on `/v1/tiles/:file`, not a startup error.

## Endpoints

All routes are under `/v1` and return the envelope
`{ success, message, payload, meta }`, where `meta.data_version` and
`meta.source` (dataset attribution) are attached to every response.
`lang=en|si|ta` (default `en`) resolves name fields, falling back to English
when a translation is missing. Every `GET` except `/v1/health` sets
`ETag: "<data_version>-<hash of path+query>"` and
`Cache-Control: public, max-age=86400, stale-while-revalidate=604800`, and
honors `If-None-Match` with a `304`.

| Method & path | Description |
|---|---|
| `GET /v1/health` | Liveness probe: `{ status: "ok" }`. No caching, no dataset attribution. |
| `GET /v1/datasets` | The full dataset catalog (source, license, feature count, download path). |
| `GET /v1/admin/:pcode` | An admin unit, its ancestor chain (root → immediate parent), and a children summary (id + name only). `?include=population,stats` adds the heavier breakdowns below. |
| `GET /v1/admin/:pcode/geometry` | The unit's simplified boundary as a bare GeoJSON `Feature` (**not** the usual envelope — see below). `404` if the pcode has no `admin_geometry` row. Cache-Control is `immutable` (a pcode's geometry never changes within a `data_version`). |
| `GET /v1/reverse?lat&lon` | Reverse geocode via the shared grid → `cell_lookup`. Falls back to the nearest named place within 25 km (bbox prefilter + haversine) when `cell_lookup` has no row for the cell; admin fields are `null` in that case. Out-of-bbox coordinates are a `404 not_in_coverage`. |
| `GET /v1/search?q&lang&limit&min_population` | FTS5 prefix search over `places_fts`, re-ranked by `score = matchQuality + populationBoost` (constants in `shared/src/search.ts`). `q` must be 2–80 characters, `limit` ≤ 50. |
| `GET /v1/lookup?q&lang&suggest&limit` | Universal lookup — classifies `q` as a coordinate pair, a 5-digit postal code, an `LK`-prefixed p-code, or free text, and dispatches accordingly. See below for the classifier and response shapes. |
| `GET /v1/population?lat&lon&radius` | Gridded population at a point, or summed over `radius` km (≤ 10). Degrades gracefully (`populated: false`) if the `cells` table hasn't been built yet. |
| `GET /v1/elections` | The election catalog. |
| `GET /v1/elections/:id/results/:entity` | Parsed results for one entity, with party metadata (`election_parties`) joined in. |
| `GET /v1/postal/:code` | A postal code record — `code`, `name`, `lat`/`lon`, and its admin chain (`gnd`/`ds_division`/`district`/`province`), derived from `cell_lookup` at the code's own coordinates (`postal_codes.admin_pcode` is unpopulated in the current artifact). `404` for an unknown code. |
| `GET /v1/postal?lat&lon` | The postal code covering a point, via `cell_lookup`. `postal_code` (and `name`) are `null` when the cell has no postal code on record. Out-of-bbox is a `404 not_in_coverage`. |
| `GET /v1/tiles/:file` | Streams a `*.pmtiles` file from `GEOPUB_TILES_DIR` with HTTP Range support (**not** the usual envelope — raw bytes, `Content-Type: application/octet-stream`). `:file` is whitelisted to `[a-z0-9-]+\.pmtiles`; anything else, or a missing file, is `404`. |

### `/v1/lookup` classifier and response shapes

`q` is classified in this order (contract §4):

1. **Coordinate pair** — `"6.93,79.84"`, `"6.93 79.84"`, or with hemisphere/degree
   decoration (`"6.9344° N, 79.8428° E"`). Runs the same logic as `/v1/reverse`
   (including its `404 not_in_coverage` for out-of-bbox points) and returns a
   single `type: "coordinate"` result.
2. **Postal code** — exactly 5 digits (`/^\d{5}$/`). Exact `postal_codes.code`
   match first, then any codes sharing that prefix. `type: "postal"`.
3. **P-code** — `LK` followed by digits (`/^LK\d+$/i`, case-insensitive). A
   direct `admin_units` lookup. `type: "admin"`. (The bare country pcode `"LK"`
   has no digit suffix, so it never matches this branch — it falls through to
   free text.)
4. **Free text** — anything else. Blends `places` FTS search (same ranking as
   `/v1/search`) with a `postal_codes.name` prefix/contains match, merged and
   sorted by score. Rows are `type: "place"` or `type: "postal"`.

Every branch except coordinate treats "no match" as a normal empty `payload`
array, not an error — only the coordinate branch can 404.

Full mode (`?suggest` unset) returns `payload: [{ type, score, payload }, ...]`
sized by `?limit` (same 1–50 bounds as `/v1/search`, default 10), best match
first. `?suggest=1` switches to lightweight typeahead rows capped at 10:
`{ type, id, label, sublabel, lat, lon, pcode? }` — `label` is the best-lang
name (or the code, for postal, with the covered place's name as `sublabel`);
a place's `sublabel` is its admin parent resolved cheaply via `cell_lookup`
when the point is covered, falling back to its `kind` otherwise; an admin
row's `sublabel` is `"<level name> · <parent name>"`.

### `/v1/admin/:pcode` response shape

```jsonc
{
  "unit": { "pcode": "LK1101", "level": 3, "name": "Colombo DS Division", "...": "AdminUnit fields" },
  "parent_chain": [/* AdminUnit[], root (country) first, immediate parent last */],
  "children": [{ "pcode": "LK110101", "name": "Fort" }],
  // present only when requested via ?include=
  "population": {
    "year": 2023,
    "buckets": { "0-4": { "f": 100, "m": 100, "t": 200 }, "...": "17 age buckets total" },
    "total": { "f": 15300, "m": 15300, "t": 30600 }
  },
  "stats": { "year": 2023, "values": { "ethnicity.sinhala": 65.5 } }
}
```

`population`/`stats` are `null` (not omitted) when `?include=` was requested
but the unit has no rows for either table — most useful early on, before a
full census/stats build lands for every level.

### `/v1/population` response shape

```jsonc
{
  "cell_id": 8275942,
  "populated": true,        // does the `cells` table have any data at all in this build?
  "point": { "population": 50, "in_coverage": true }, // in_coverage: does *this* cell have a row?
  "radius": { "radius_km": 0.5, "population": 620, "cell_count": 13 } // null unless ?radius was given
}
```

### `/v1/postal/:code` response shape

```jsonc
{
  "code": "10250",
  "name": "Nugegoda",
  "lat": 6.8735,
  "lon": 79.8899,
  // derived from cell_lookup at (lat, lon) — all null if that point isn't covered
  "gnd": { "pcode": "LK1124070", "level": 4, "name": "Nugegoda West", "...": "AdminUnit fields" },
  "ds_division": { "pcode": "LK1124", "...": "..." },
  "district": { "pcode": "LK11", "...": "..." },
  "province": { "pcode": "LK1", "...": "..." }
}
```

### Errors

Zod validation failures → `400`. Unknown routes, unknown admin pcodes, and
unknown election/entity ids → `404` (`not_found`); out-of-coverage
coordinates → `404` (`not_in_coverage`). Anything else is masked as a `500`
with the real error logged server-side only — internals are never leaked in
the response body.

## Tests

`tests/fixture.ts` builds a small in-memory SQLite database implementing the
full contract schema (10 admin units across all 5 levels with a handful of
`admin_geometry` rows — one unit, LK21, deliberately left without geometry
to exercise that 404 path — population/stats for one DS division, 3 places +
FTS index, 4 postal codes, `cells`/`cell_lookup` covering a 5×5 block around
Colombo Fort plus two more GN divisions, 1 election with 2 entities). Every
route is integration-tested through `app.request()`, including the
`404 not_in_coverage` path, the `ETag`/`304` flow, `lang` fallback to
English, every `/v1/lookup` classifier branch (coordinate, postal
exact/prefix, p-code, blended free text) in both full and `suggest` mode,
and `/v1/tiles`' Range/If-None-Match/traversal-rejection behavior against a
small file written to a temp directory. 89 tests total.

## Docker

Build from the **repository root** (the image needs the `@geopub/shared`
workspace package):

```bash
docker build -f api/Dockerfile -t geopub-api .
docker run --rm -p 8600:8600 \
  -v /path/to/geopub.sqlite:/app/foundry/data/artifacts/geopub.sqlite:ro \
  geopub-api
```

Multi-stage build: resolves the workspace from `pnpm-lock.yaml`, typechecks
as a build gate, then ships a slim `node:22-slim` runtime image running as
the image's built-in non-root `node` user. The process binds `0.0.0.0`
inside the container; restricting external exposure is done at the host /
Docker Compose layer (`docs/architecture.md` §6), not in the app.
