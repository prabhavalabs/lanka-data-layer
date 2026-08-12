# @geopub/foundry

Offline ETL pipeline. Fetches every source, normalizes it into the p-code-keyed
schema in [`docs/architecture.md`](../docs/architecture.md) §2, and emits
`data/artifacts/geopub.sqlite` + `manifest.json`.

`foundry` never imports from `api` or `web`, and the API never writes to the
SQLite file it reads — see architecture.md §5 for the full package contract.

## Running

```bash
pnpm install                                   # from the repo root

# First run (or any time data/raw/ is empty): the seed step needs a local
# checkout of the source project it copies from.
FOUNDRY_SEED_SOURCE=/path/to/ceylon-hub pnpm --filter @geopub/foundry run build

# Once data/raw/ is populated, FOUNDRY_SEED_SOURCE isn't needed again —
# seed only reads it for files still missing from data/raw/.
pnpm --filter @geopub/foundry run build
pnpm --filter @geopub/foundry run build -- --only admin,population
pnpm --filter @geopub/foundry run build -- --only seed,layers,pois-extend,downloads,datasets,emit
pnpm --filter @geopub/foundry run build -- --only fetch-worldpop,fetch-gnd,admin,cells,admin-geometry,cell-lookup
pnpm --filter @geopub/foundry run test         # unit tests (node:test)
```

Raw downloads cache in `data/raw/` (gitignored); every fetch/seed step skips
work it already did, so re-running `build` is cheap and safe. Artifacts land
in `data/artifacts/` (also gitignored) — `geopub.sqlite` and `manifest.json`.

## Pipeline

Steps run in this fixed order (`--only` filters the list, it never reorders
it — later steps assume earlier ones already ran at least once against the
same `data/artifacts/geopub.sqlite`):

| # | Step | `src/steps/*.ts` | Produces |
|---|---|---|---|
| 1 | `seed` | `seed.ts` | Copies the source files this build needs into `data/raw/` |
| 2 | `fetch-postal` | `fetch-postal.ts` | Downloads + extracts the GeoNames LK postal dump |
| 3 | `fetch-worldpop` | `fetch-worldpop.ts` | Downloads the WorldPop Sri Lanka 1km UN-adjusted population GeoTIFF |
| 4 | `fetch-gnd` | `fetch-gnd.ts` | Downloads + extracts OCHA COD-AB Sri Lanka ADM3/ADM4 GeoJSON |
| 5 | `admin` | `admin.ts` | `admin_units` (levels 0-4) |
| 6 | `population` | `population.ts` | `admin_population`, `admin_stats` |
| 7 | `elections` | `elections.ts` | `elections`, `election_parties`, `election_entities`, `election_results` |
| 8 | `places` | `places.ts` | `places`, `places_fts` |
| 9 | `postal` | `postal.ts` | `postal_codes` |
| 10 | `pois` | `pois.ts` | `pois` |
| 11 | `layers` | `layers.ts` | `data/artifacts/layers/*.geojson` (roads, railways, waterways, protected areas, country) |
| 12 | `pois-extend` | `pois-extend.ts` | Adds railway stations/halts to `pois` (category=`transport`) |
| 13 | `cells` | `cells.ts` | `cells` (gridded population) |
| 14 | `admin-geometry` | `admin-geometry.ts` | `admin_geometry` (levels 1-4) + `data/artifacts/layers/admin-adm{1..4}.geojson`, `electoral-divisions.geojson`, `polling-divisions.geojson` |
| 15 | `cell-lookup` | `cell-lookup.ts` | `cell_lookup` (reverse-geocode grid index) |
| 16 | `downloads` | `downloads.ts` | `data/artifacts/downloads/*.csv` + `*.geojson.gz` |
| 17 | `tiles` | `tiles.ts` | `data/artifacts/tiles/*.pmtiles` (+ `data/artifacts/layers/places.geojson`, `postal.geojson`) |
| 18 | `datasets` | `datasets.ts` | `datasets` (the bulk-download catalog) |
| 19 | `emit` | `emit.ts` | `meta` rows, VACUUM + `journal_mode=OFF`, `manifest.json` (incl. tile artifact hashes) |

The SQLite schema itself (`CREATE TABLE`/`CREATE INDEX IF NOT EXISTS`) lives
in `src/db.ts` and is applied idempotently on every DB open — not only in
`emit` — which is what lets any single step run standalone via `--only`.

## Map layers

`layers.ts` writes one GeoJSON `FeatureCollection` per source into
`data/artifacts/layers/`, properties trimmed to what each layer needs for
styling and coordinates rounded to 6 dp (`docs/architecture.md` §3):

| File | Source | Properties kept |
|---|---|---|
| `roads.geojson` | `geo/roads.geojson` | `id`, `highway`, `name`, `ref` |
| `railways.geojson` | `geo/railways.geojson` | `id`, `name`, `name_si`, `name_ta`, `kind` |
| `waterways.geojson` | `geo/waterways.geojson` | `id`, `name`, `kind` |
| `protected-areas.geojson` | `geo/protected-areas.geojson` | `id`, `name`, `kind`, `protectionType` |
| `country.geojson` | `geo/country.geojson` | passed through as-is |

`railways.geojson` carries both the LineString track segments (`kind='rail'`)
and the Point station/halt features (`kind` in `station`/`halt`) from the
same raw file in one layer — `kind` is the styling discriminant a consumer
filters on (line layer vs. point layer). This step only writes GeoJSON and
does not touch any SQLite table; turning these files (plus `admin-geometry`'s
and `tiles`' own places/postal GeoJSON) into PMTiles is the `tiles` step's
job — see "Vector tiles" below.

The station/halt points are *also* fed into `pois` (see below) — that is a
separate artifact for a separate use case (search/lookup vs. map rendering
from the same source data), not a duplicate of this layer.

## Admin boundaries: official p-codes via COD-AB (`fetch-gnd` + `admin`)

Levels 0-2 (country, provinces, districts) still come from geoBoundaries —
their p-codes already agree with OCHA COD-AB's. Levels 3-4 (DS divisions,
GN divisions) come from **OCHA COD-AB Sri Lanka** (HDX dataset `cod-ab-lka`),
which carries the official Survey Department p-codes geoBoundaries never
had for these levels. `fetch-gnd.ts` downloads HDX's ready-made GeoJSON zip
(`lka_admin_boundaries.geojson.zip`) and extracts only the two files this
build needs — `lka_admin3.geojson` (339 DS divisions) and
`lka_admin4.geojson` (~14k GN divisions) — into `data/raw/cod-ab/`. HDX also
publishes a shapefile bundle for the same dataset, but the GeoJSON resource
means no shapefile-parser dependency is needed.

`admin.ts` reads COD-AB's `adm3_pcode`/`adm4_pcode` (and `_name`/`_name1`
si/`_name2` ta/`area_sqkm`/`center_lat`/`center_lon`) directly — COD-AB's own
area/centroid figures are official Survey Department numbers, more accurate
than this pipeline's bbox-center approximation (`src/lib/geo.ts`), so levels
3-4 use those as-is instead of computing from geometry. This fully replaces
the old `GB:<geoBoundaries id>` interim DS-division pcode scheme (see
"Design notes" below): `meta.pcode_scheme = 'cod-ab-v1'` and the old
`meta.adm3_pcode_interim` key are gone. Re-running `admin` against a
database where `admin-geometry` has already populated rows referencing
these pcodes needs `PRAGMA defer_foreign_keys = ON` around the
delete-then-reinsert (this connection enforces foreign keys) — see the
comment in `admin.ts`.

## Gridded population (`fetch-worldpop` + `cells`)

`fetch-worldpop.ts` downloads the WorldPop "Global 2000-2020, 1km UNadj"
Sri Lanka raster (`lka_ppp_2020_1km_Aggregated_UNadj.tif`, ~30 arc-sec /
~1km pixels, UN-adjusted to match national population totals) into
`data/raw/worldpop/`.

`cells.ts` parses it with the `geotiff` package (pure JS, no native deps)
and resamples it onto the canonical 0.001deg grid (`src/grid.ts`): for every
pixel with population > 0, `src/lib/raster.ts` computes the exact set of
fine-grid cells whose *center* falls inside that pixel's bounds (closed-form
from the raster's own affine transform — no per-cell scanning), and the
pixel's population is split evenly across them. A fine cell always belongs
to exactly one pixel (pixels tile the plane the same way the grid does), so
there's no double-counting; the insert still accumulates on conflict as a
defensive measure. A guard compares the sum of all inserted `cells.pop`
against the raster's own total and fails the build if they diverge by more
than 0.5% — both totals are logged either way. In this build: 78,731
positive pixels distribute across 5,467,524 fine cells, 0.0000% deviation,
in ~3-5s.

## Admin geometry (`admin-geometry`)

Fills `admin_geometry` (docs/architecture.md §2) for every admin unit at
levels 1-4: `src/lib/simplify.ts` runs Douglas-Peucker simplification
(~0.0005deg tolerance) per ring — holes simplified independently from their
outer ring, and any ring that would degenerate below a valid polygon falls
back to its original (unsimplified) points rather than emit broken geometry
— then `src/lib/geojson.ts` rounds coordinates to 6 dp and the bare geometry
(no Feature wrapper, per the schema) is stored as a JSON string.

The same step also writes the **full-detail** (unsimplified, 6 dp) source
data as one `FeatureCollection` per level to
`data/artifacts/layers/admin-adm{1..4}.geojson` — for the `tiles` step, same
directory `layers.ts` writes into, disjoint filenames — with properties
`pcode`, `name_en`, `name_si`, `name_ta`, `level`, `parent_pcode`. si/ta
aren't on every level's raw source (geoBoundaries provinces/districts only
carry `name`; `admin.ts` backfills si/ta for those from
`population-2023.json`), so this step reads them back from `admin_units`
instead of re-deriving them — that table is already fully populated by the
`admin` step, which runs earlier in `PIPELINE`. It also copies
`electoral-divisions.geojson`/`polling-divisions.geojson` from
`data/raw/geo/` into the same directory at 6 dp (they're already at their
final resolution; this just relocates + rounds them for the artifact
bundle).

## Reverse geocode (`cell-lookup`)

Fills `cell_lookup` (docs/architecture.md §2), one row per cell in `cells`
(cells doubles as the land mask — see the schema comment). For each cell
center: point-in-polygon against ADM4 (GN division) polygons for
`gnd_pcode`, plus nearest `postal_codes` row and nearest `places` row by
haversine distance.

Three techniques keep ~5.5M cells x 3 lookups tractable (all in
`src/lib/`, unit-tested in `test/pip.test.ts`/`test/nearest.test.ts`):

- **`pip.ts`**: ray-casting point-in-polygon with hole support (a point is
  inside iff inside the outer ring and outside every hole ring — winding
  order doesn't matter), plus `PolygonIndex`, a bbox-bucketed candidate
  index so a lookup only ray-casts against the handful of polygons whose
  bbox could plausibly contain the point, not all ~14k. Coastal-sliver cells
  (center falls in no ADM4 polygon — a real mismatch between WorldPop's land
  mask and COD-AB's coastline) get the nearest polygon within 2km by
  point-to-*segment* distance (not just nearest vertex — a point can sit
  meters from the middle of a long edge and degrees from the nearest
  vertex), searching outward bucket-ring by bucket-ring; a cell with no
  polygon within 2km is skipped entirely (no `cell_lookup` row).
- **`nearest.ts`**: the same grid-bucket idea specialized for point sets
  (`postal_codes`, `places`) — expanding ring search with a provable stop
  condition, so nearest-of-~1800 / nearest-of-~550 stays sub-microsecond
  per query instead of an O(n) scan.
- **Sticky last-match cache** (in `cell-lookup.ts` itself, not a lib): cells
  are walked in `cell_id` order, which is row-major over the grid — so a
  cell is almost always in the same GN division as the previous one. Each
  cell first re-tests the *previous* cell's matched polygon (one exact PIP
  test) before falling back to the bucket index; in this build that's a
  ~92% hit rate, which is most of why the whole step finishes in ~2 minutes
  rather than much longer.

This build: 14,043 ADM4 polygons, 1,833 postal codes, 553 places indexed;
5,467,317 of 5,467,524 cells resolved (207 skipped, no polygon within 2km);
~119s end to end.

## POIs: railway stations (`pois-extend`)

`pois-extend.ts` adds the Point features from `geo/railways.geojson` with
`kind` `station` or `halt` into `pois` with `category='transport'` (the
LineString track segments are not pois — those go to `layers` only).

`pois` has no natural key (`pois.ts` lets SQLite assign a plain autoincrement
rowid — see that file), so there's nothing to upsert against. `pois-extend`
stays idempotent the same way `places.ts` does: it deletes the subset it
owns (`category='transport' AND kind IN ('station','halt')` — disjoint from
the `aerodrome` transport rows `pois.ts` inserts) and reinserts, rather than
forcing a key that doesn't exist onto the table. Running `pois` again
afterwards is still safe too: it unconditionally clears the whole table
before its own insert.

## Vector tiles (`tiles`)

Builds the PMTiles vector tilesets (`docs/architecture.md` §3,
`tiles/<layer>.pmtiles`) from the GeoJSON `layers`/`admin-geometry` already
wrote, plus a `places`/`postal` GeoJSON this step generates itself from the
`places`/`postal_codes` tables. It shells out to the host **tippecanoe**
binary (v2.79.0) via `node:child_process` — `tippecanoe --version` runs
first and fails the step with an actionable message
(`brew install tippecanoe`) if the binary isn't on `PATH`.

| Tileset | Layers (zoom range) | Properties kept |
|---|---|---|
| `admin.pmtiles` | `adm1` (z0-14), `adm2` (z6-14), `adm3` (z8-14), `adm4` (z10-14) | `pcode`, `name_en`, `name_si`, `name_ta` |
| `electoral.pmtiles` | `ed` (z0-14), `pd` (z7-14) | `id`, `name` |
| `transport.pmtiles` | `roads`, `railways` (both z6-14, `--drop-densest-as-needed`) | `highway`, `ref`, `name`, `kind` |
| `water.pmtiles` | `waterways` (z6-14, `--drop-densest-as-needed`) | `id`, `name`, `kind` |
| `protected.pmtiles` | `protected` (z6-14) | `id`, `name`, `kind`, `protectionType` |
| `places.pmtiles` | `places`, `postal` (both z0-14) | places: `id`, `name_en`, `name_si`, `name_ta`, `kind`, `population`; postal: `code`, `name` |

`admin.pmtiles` and `electoral.pmtiles` each merge several levels into one
tileset via a single tippecanoe invocation with multiple `-L` named layers.
tippecanoe has no per-`-L`-source zoom option, so per-layer minzoom (`adm2`
starting at z6, `pd` at z7, etc.) comes from stamping every feature in the
lower layers with tippecanoe's `tippecanoe.minzoom` GeoJSON extension
(`src/lib/tiles.ts:withMinzoom`) before handing a staged copy to tippecanoe
— the top-level `-Z`/`-z` only sets the tileset's outer bound. Layers whose
intended range already matches that outer bound (`adm1`, `ed`, `roads`,
`railways`) are read straight from `data/artifacts/layers/`, unstamped.
Property allowlists are enforced with tippecanoe's `--include`, dropping
everything else per feature — this is also what keeps the large nested
per-division election JSON out of `electoral.pmtiles`
(`electoral-divisions.geojson`/`polling-divisions.geojson` carry it
unfiltered; only `id`/`name` survive `--include`).

`places.geojson`/`postal.geojson` (`src/lib/tiles.ts:placesFeatureCollection`
/`postalFeatureCollection`) are written into `data/artifacts/layers/`
alongside the other layer files, not just staged for tippecanoe — coordinates
rounded to 6 dp per `docs/architecture.md` §3, `postal_codes` rows missing
`lat`/`lon` (nullable in the schema) dropped since they can't be placed on a
map.

**Idempotent per tileset**: each tileset is skipped (tippecanoe isn't
invoked) when its output `.pmtiles` file is newer than every one of its
declared inputs (`src/lib/tiles.ts:needsRebuild`, mtime-based) — set
`FOUNDRY_FORCE_TILES=1` to force a full rebuild regardless. Declared inputs
are the real upstream sources, not any staged/stamped copy (staged copies
are always freshly regenerated when a tileset does need rebuilding, so their
own mtime would never usefully signal staleness): the four
`admin-adm{1..4}.geojson` files for `admin.pmtiles`, the matching
`layers/*.geojson` file(s) for `electoral`/`transport`/`water`/`protected`,
and `geopub.sqlite` itself for `places.pmtiles` (its data comes from the DB,
not a `layers/*.geojson` file — coarser than per-table freshness, since any
table write touches the same file's mtime, but never a stale skip).

Common flags across every invocation: `--force` (overwrite instead of
erroring if the output already exists — safe because it's this step's own
freshness check that decides whether to invoke tippecanoe at all, not
tippecanoe's own file-exists guard), `--projection=EPSG:4326` (explicit,
matches this pipeline's GeoJSON), default tile compression (no
`--no-tile-compression` — PMTiles handles gzip tiles natively), and
`--no-progress-indicator` rather than `--quiet` — the former still reports
warnings on stderr, which this step parses line-by-line and re-logs through
its own `log()`, while `--quiet` would silently swallow them too.

Staged/stamped copies live under `data/artifacts/tiles/.staging/` for the
duration of a tileset's build and are removed once the whole step finishes
(success or failure) — they're never a permanent artifact.

`emit` (see below) discovers every `data/artifacts/tiles/*.pmtiles` file and
hashes it into `manifest.json`'s `artifacts` array alongside `geopub.sqlite`.

## Bulk downloads (`downloads` + `datasets`)

`downloads.ts` writes the public bulk-download artifacts to
`data/artifacts/downloads/`:

- One CSV per tabular dataset: `admin-units.csv` (all `admin_units`
  columns), `population-2023.csv` (`admin_population` joined with
  `admin_units` for names), `elections.csv` (`election_results` flattened
  with `entity_name`/`kind` joined in from `election_entities` —
  `election_id`, `entity_id`, `entity_name`, `kind`, `electors`, `polled`,
  `valid`, `rejected`, `winner_party`, `winner_votes`), `places.csv`,
  `postal-codes.csv`, `pois.csv`.
- One `<layer>.geojson.gz` per file `layers` wrote (`node:zlib` gzip of the
  already-rounded GeoJSON, byte-identical to it once decompressed).
- `cells.csv`/`cell-lookup.csv` are exported the same way as every other
  table-backed dataset now that `cells`/`cell-lookup` run earlier in
  `PIPELINE` (see the pipeline table above) — a dataset whose source table
  has 0 rows (e.g. a `--only` run that skipped them) is still skipped with a
  log line rather than failing the build.

CSV encoding is RFC 4180-ish (`src/lib/csv.ts`): a field is quoted only if
it contains a comma, quote, or newline, with embedded quotes doubled. Text
columns (Sinhala/Tamil names in particular) are written as plain UTF-8 —
nothing is escaped beyond the CSV structural characters, so they round-trip
exactly.

`datasets.ts` then builds the `datasets` catalog table (`docs/architecture.md`
§2) — title/category/description/source attribution plus a `feature_count`
(row count for table-backed datasets, feature count for layer-backed ones)
and `download_path`. **`datasets` fully owns its table**: every run clears
it and rebuilds from its dataset list, so a dataset that becomes empty (or a
stray row from unrelated manual testing against the same artifact) doesn't
linger — same delete-then-rebuild convention `pois.ts` uses.

`downloads` and `datasets` intentionally don't call into each other:
`datasets` runs immediately after `downloads` in `PIPELINE` and stamps
`download_path` by checking, per dataset, whether the file `downloads`
would have written for it exists on disk at their shared naming convention
(`downloads/<id>.csv` or `downloads/<id>.geojson.gz`) — `null` if not. The
two steps only need to agree on that naming convention, not on call order
beyond `downloads` running first.

## Data sources (this build)

Seeded from a local clone of the predecessor project (`prabhavalabs/ceylon-hub`)
via `seed.ts` — see that file for the exact file list. `fetch-postal.ts`,
`fetch-worldpop.ts`, and `fetch-gnd.ts` hit the network directly (GeoNames,
WorldPop, HDX respectively) rather than being seeded.

| Source | License |
|---|---|
| geoBoundaries admin boundaries (ADM0-ADM2) | CC BY 3.0 IGO |
| OCHA COD-AB Sri Lanka (`cod-ab-lka`) — DS/GN division boundaries, ADM3-ADM4 | CC BY-IGO |
| WorldPop Sri Lanka 1km UN-adjusted population | CC BY 4.0 |
| HDX `cod-ps-lka` population projections 2023 | CC BY-IGO |
| Dept. of Census & Statistics ethnicity/religion 2012 | Open data |
| OpenStreetMap places, hospitals, education, airports | ODbL |
| OpenStreetMap roads, railways, waterways, protected areas (map layers) | ODbL |
| Survey Department electoral/polling division boundaries (via `nuuuwan/sl-topojson`) | open |
| Election Commission results (via `nuuuwan/lk_elections`) | open |
| GeoNames LK postal codes | CC BY 4.0 |

`manifest.json`'s `sources` array carries the machine-readable version of
this table (id, license, url, `fetched_at` from the newest raw-file mtime
behind that source).

## Design notes / deviations worth knowing about

- **DS/GN-division (levels 3-4) p-codes are official**, sourced from OCHA
  COD-AB (see "Admin boundaries" above) — `meta.pcode_scheme = 'cod-ab-v1'`
  records this. An earlier build minted interim `GB:<geoBoundaries id>`
  level-3 pcodes (`meta.adm3_pcode_interim`) before COD-AB was ingested;
  both the interim pcodes and that meta key are gone now. COD-AB reports 339
  DS divisions, not the commonly-cited 330 — the source data is simply more
  current than that older figure.
- **`election_entities.name` for POSTAL entities is derived**, not sourced:
  the election result files carry stats only, no display name, for the 22
  `EC-xxP` postal-vote entities. `elections.ts` builds it as
  `"<parent ED name> Postal"` from the ED's name (which *is* sourced, from
  `electoral-divisions.geojson`). `ed_id` for an ED entity itself is set to
  its own id (not null) — it makes "everything under this ED" queries a
  single `WHERE ed_id = ?` regardless of entity kind.
- **`admin_pcode` is still null** on every `places`, `postal_codes`, and
  `pois` row — and stays that way by design now, not just for lack of
  boundaries. `cell_lookup` exists as of this build (see "Reverse geocode"
  above) and is the intended way to resolve a point's containing admin
  unit: snap the point to its grid cell (`src/grid.ts`), look up
  `cell_lookup.gnd_pcode`, then walk `admin_units.parent_pcode` — a runtime
  join, not a backfilled column, so it stays correct if `cell_lookup` is
  ever rebuilt from newer boundaries without needing to also rewrite every
  table that references a point.
- **Admin unit `area_km2`/centroid**: levels 0-2 use this pipeline's own
  bbox-center approximation (`src/lib/geo.ts:bboxCenter`, no turf
  dependency per docs/architecture.md §5) since geoBoundaries doesn't always
  carry a usable centroid; the country row's `area_km2` is the sum of
  province areas. Levels 3-4 use COD-AB's own `area_sqkm`/`center_lat`/
  `center_lon` properties directly (official Survey Department figures) —
  more accurate than computing an approximation from geometry, so this
  pipeline doesn't.
- **`geo/ds-divisions.geojson` is fetched but unused.** `seed.ts` still
  copies it (harmless — it's cheap and other tooling may still want it) but
  `admin.ts` no longer reads it now that DS divisions come from COD-AB.

## Adding a new source

1. Add the raw file(s) to `seed.ts`'s `GEO_FILES`/`DATA_FILES` (or write a
   dedicated `fetch-*.ts` step next to `fetch-postal.ts` if it should be
   fetched live instead of seeded).
2. Add or extend a step in `src/steps/` that reads from `data/raw/` and
   writes into the tables it owns via `db.prepare(...).run(...)` — follow
   the transaction + `ON CONFLICT DO UPDATE` (or delete-then-insert, for
   tables without a natural key) pattern already used by the existing steps
   so the step stays safe to re-run.
3. Register the step in `PIPELINE` in `src/build.ts`, in dependency order.
4. If the source needs attribution, add an entry to `SOURCES` in
   `src/steps/emit.ts` so it shows up in `manifest.json`.
5. Add its tables/columns to `docs/architecture.md` §2 first if the schema
   needs to change — that file is the binding contract, not `src/db.ts`.

## Grid

`src/grid.ts` is a deliberate byte-for-byte duplicate of `shared/src/grid.ts`
(see docs/architecture.md §1 — the formula must exist in exactly those two
source files plus the doc). `test/grid.test.ts` pins the same test vectors
as `shared/src/grid.test.ts`; if you change one, change both and re-run both
test suites.
