import { readFile } from "node:fs/promises";
import type { StepContext } from "../step.ts";
import { rawPath } from "../lib/paths.ts";
import { cellCenter } from "../grid.ts";
import { PolygonIndex, pointInGeometry, type PolygonalGeometry } from "../lib/pip.ts";
import { NearestIndex } from "../lib/nearest.ts";

export const name = "cell-lookup";

// ~ typical GN division extent (2.7 x 4.2deg bbox / ~14k GN divisions).
const ADM4_BUCKET_CELL_DEG = 0.03;
// Postal codes (~1.8k) and places (~550) are much sparser than GN divisions.
const POSTAL_BUCKET_CELL_DEG = 0.08;
const PLACE_BUCKET_CELL_DEG = 0.12;
// Coastal-sliver fallback radius (docs/architecture.md §2 `cell_lookup`).
const NEAREST_POLYGON_FALLBACK_M = 2000;

interface CodAbFeature {
  properties: { adm4_pcode?: string };
  geometry: PolygonalGeometry;
}
interface CodAbFeatureCollection {
  features: CodAbFeature[];
}

/**
 * cell_lookup (docs/architecture.md §2): the reverse-geocode table, one row
 * per land cell in `cells`. For each cell center: point-in-polygon against
 * ADM4 (GN division) polygons for gnd_pcode, plus nearest postal_codes row
 * and nearest places row by haversine.
 *
 * The postal lookup is district-constrained: a cell's candidate postal codes
 * are only the ones postal.ts resolved (via the dump's own district column,
 * not coordinates) to the cell's own district. Some GeoNames LK postal
 * points are badly misplaced — occasionally landing on top of a *different*
 * code's true location in a neighboring district — so an unconstrained
 * "nearest by distance" search can hand a cell a code that belongs many km
 * away just because its (wrong) coordinate happens to be close. Districts
 * with zero district-mapped postal codes fall back to the old country-wide
 * nearest-search behavior, same as codes whose district couldn't be
 * resolved at all.
 *
 * Two performance techniques keep ~5M cells x 3 lookups tractable:
 *  - Every lookup goes through a grid-bucket index (lib/pip.ts's
 *    PolygonIndex, lib/nearest.ts's NearestIndex) instead of scanning every
 *    polygon/point per cell. This now includes one NearestIndex per
 *    district for postal codes (in addition to the global fallback index),
 *    each built once up front — cheap, since there are only ~25 districts.
 *  - `cells` is walked in cell_id order, which is row-major over the grid
 *    (lib/grid.ts) — consecutive cells are spatially adjacent, so a cell
 *    almost always sits in the same GN division as the previous one. A
 *    single "sticky" last-match cache checks that one polygon first (cheap:
 *    one exact PIP test) before falling back to the bucket index, cutting
 *    the dominant cost (ADM4 PIP, ~500 vertices/polygon on average) down to
 *    roughly one test per cell instead of one bucket-query-plus-test.
 */
export async function run({ db, log }: StepContext): Promise<void> {
  const gn = JSON.parse(await readFile(rawPath("cod-ab", "lka_admin4.geojson"), "utf8")) as CodAbFeatureCollection;

  const adm4Index = new PolygonIndex<string>(ADM4_BUCKET_CELL_DEG);
  for (const f of gn.features) {
    if (!f.properties.adm4_pcode) continue;
    adm4Index.insert(f.properties.adm4_pcode, f.geometry);
  }
  const geometryByPcode = new Map<string, PolygonalGeometry>();
  for (const f of gn.features) {
    if (f.properties.adm4_pcode) geometryByPcode.set(f.properties.adm4_pcode, f.geometry);
  }
  log(`cell-lookup: indexed ${geometryByPcode.size} ADM4 polygons`);

  // GN division -> district, by walking the admin_units parent chain (the
  // ground truth) rather than assuming p-code prefixes are hierarchical.
  // Spot-verified separately against a handful of rows (LK6175085 ->
  // LK6175 -> LK61, LK6248025 -> LK6248 -> LK62) and, across all 14k+ GN
  // divisions checked here, the first-4-chars prefix shortcut turns out to
  // agree with the walk every time — logged below so a future COD-AB
  // release that broke that agreement wouldn't go unnoticed. The walk
  // result (not the prefix) is what's actually used.
  const adminRows = db.prepare(`SELECT pcode, level, parent_pcode FROM admin_units WHERE level IN (2, 3, 4)`).all() as {
    pcode: string;
    level: number;
    parent_pcode: string | null;
  }[];
  const adminByPcode = new Map(adminRows.map((r) => [r.pcode, r]));
  const gndToDistrict = new Map<string, string>();
  let prefixAgrees = 0;
  let prefixDisagrees = 0;
  for (const r of adminRows) {
    if (r.level !== 4) continue;
    let cur = r;
    let district: string | null = null;
    for (let hop = 0; hop < 5; hop++) {
      const parent = cur.parent_pcode ? adminByPcode.get(cur.parent_pcode) : undefined;
      if (!parent) break;
      if (parent.level === 2) {
        district = parent.pcode;
        break;
      }
      cur = parent;
    }
    if (district) {
      gndToDistrict.set(r.pcode, district);
      if (r.pcode.slice(0, 4) === district) prefixAgrees++;
      else prefixDisagrees++;
    }
  }
  log(
    `cell-lookup: resolved district for ${gndToDistrict.size} GN divisions via parent-chain walk ` +
      `(pcode-prefix shortcut agreed for ${prefixAgrees}, disagreed for ${prefixDisagrees})`,
  );

  const postalRows = db
    .prepare(`SELECT code, lat, lon, admin_pcode FROM postal_codes WHERE lat IS NOT NULL AND lon IS NOT NULL`)
    .all() as { code: string; lat: number; lon: number; admin_pcode: string | null }[];

  // Global fallback index (unconstrained, country-wide nearest — the old
  // behavior) for districts with no district-mapped postal codes, and for
  // any postal code whose own district couldn't be resolved.
  const globalPostalIndex = new NearestIndex<string>(POSTAL_BUCKET_CELL_DEG);
  for (const r of postalRows) globalPostalIndex.insert(r.code, r.lat, r.lon);

  // Per-district indexes — the fixed behavior (see module doc comment).
  const postalIndexByDistrict = new Map<string, NearestIndex<string>>();
  let postalCodesWithoutDistrict = 0;
  for (const r of postalRows) {
    if (!r.admin_pcode) {
      postalCodesWithoutDistrict++;
      continue;
    }
    let idx = postalIndexByDistrict.get(r.admin_pcode);
    if (!idx) {
      idx = new NearestIndex<string>(POSTAL_BUCKET_CELL_DEG);
      postalIndexByDistrict.set(r.admin_pcode, idx);
    }
    idx.insert(r.code, r.lat, r.lon);
  }
  const districtCount = (db.prepare(`SELECT COUNT(*) AS n FROM admin_units WHERE level = 2`).get() as { n: number }).n;
  log(
    `cell-lookup: ${postalIndexByDistrict.size}/${districtCount} districts have district-mapped postal codes; ` +
      `${postalCodesWithoutDistrict} postal codes have no admin_pcode (global-fallback only)`,
  );

  const placeRows = db.prepare(`SELECT id, lat, lon FROM places`).all() as { id: number; lat: number; lon: number }[];
  const placeIndex = new NearestIndex<number>(PLACE_BUCKET_CELL_DEG);
  for (const r of placeRows) placeIndex.insert(r.id, r.lat, r.lon);
  log(`cell-lookup: indexed ${postalRows.length} postal codes, ${placeRows.length} places`);

  const cellIds = (db.prepare(`SELECT cell_id FROM cells ORDER BY cell_id`).all() as { cell_id: number }[]).map(
    (r) => r.cell_id,
  );
  log(`cell-lookup: resolving ${cellIds.length} cells`);

  const insert = db.prepare(`
    INSERT INTO cell_lookup (cell_id, gnd_pcode, postal_code, nearest_place_id, nearest_place_dist_m)
    VALUES (@cell_id, @gnd_pcode, @postal_code, @nearest_place_id, @nearest_place_dist_m)
    ON CONFLICT(cell_id) DO UPDATE SET
      gnd_pcode = excluded.gnd_pcode, postal_code = excluded.postal_code,
      nearest_place_id = excluded.nearest_place_id, nearest_place_dist_m = excluded.nearest_place_dist_m
  `);

  let assignedDirect = 0;
  let assignedFallback = 0;
  let skippedNoGnd = 0;
  let stickyHits = 0;
  let postalDistrictConstrained = 0;
  let postalGlobalFallback = 0;

  let stickyPcode: string | null = null;
  let stickyGeometry: PolygonalGeometry | null = null;

  const tx = db.transaction(() => {
    db.prepare(`DELETE FROM cell_lookup`).run();

    for (const cellId of cellIds) {
      const c = cellCenter(cellId);
      if (!c) continue; // cell_id always came from a valid grid write; defensive only

      let gnd: string | null = null;

      if (stickyGeometry && pointInGeometry(c.lon, c.lat, stickyGeometry)) {
        gnd = stickyPcode;
        stickyHits++;
      } else {
        gnd = adm4Index.contains(c.lon, c.lat);
        if (gnd) {
          stickyPcode = gnd;
          stickyGeometry = geometryByPcode.get(gnd) ?? null;
        }
      }

      if (gnd) {
        assignedDirect++;
      } else {
        gnd = adm4Index.nearestWithin(c.lon, c.lat, NEAREST_POLYGON_FALLBACK_M);
        if (gnd) {
          assignedFallback++;
          stickyPcode = gnd;
          stickyGeometry = geometryByPcode.get(gnd) ?? null;
        }
      }

      if (!gnd) {
        skippedNoGnd++;
        continue;
      }

      const district = gndToDistrict.get(gnd);
      const districtPostalIndex = district ? postalIndexByDistrict.get(district) : undefined;
      if (districtPostalIndex) postalDistrictConstrained++;
      else postalGlobalFallback++;
      const postal = (districtPostalIndex ?? globalPostalIndex).nearest(c.lat, c.lon);
      const place = placeIndex.nearest(c.lat, c.lon);

      insert.run({
        cell_id: cellId,
        gnd_pcode: gnd,
        postal_code: postal ? postal.id : null,
        nearest_place_id: place ? place.id : null,
        nearest_place_dist_m: place ? Math.round(place.distM) : null,
      });
    }
  });
  tx();

  const total = assignedDirect + assignedFallback;
  log(
    `cell-lookup: assigned ${total} rows (${assignedDirect} direct PIP incl. ${stickyHits} sticky-cache hits, ` +
      `${assignedFallback} nearest-polygon fallback <= ${NEAREST_POLYGON_FALLBACK_M}m), skipped ${skippedNoGnd} ` +
      `(no ADM4 polygon within range)`,
  );
  log(
    `cell-lookup: postal assignment was district-constrained for ${postalDistrictConstrained} cells, ` +
      `${postalGlobalFallback} used the country-wide global fallback (unresolved district or zero district-mapped codes)`,
  );
}
