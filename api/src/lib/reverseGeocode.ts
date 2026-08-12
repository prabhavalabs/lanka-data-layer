import type Database from "better-sqlite3";
import type { Lang, ReverseResult } from "@geopub/shared";
import { prepared } from "./cache.ts";
import { resolveAdminLevels } from "./admin.ts";
import { resolveName } from "./lang.ts";
import { haversineMeters, metersToLatDegrees, metersToLonDegrees } from "./geo.ts";

const NEAREST_PLACE_RADIUS_M = 25_000;

interface CellLookupRow {
  gnd_pcode: string;
  postal_code: string | null;
  nearest_place_id: number | null;
  nearest_place_dist_m: number | null;
}

interface PlaceRow {
  id: number;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  lat: number;
  lon: number;
}

function nearestPlaceFallback(
  db: Database.Database,
  lat: number,
  lon: number,
  lang: Lang,
): { place: { id: number; name: string; distance_m: number } | null; touched: boolean } {
  const latBuffer = metersToLatDegrees(NEAREST_PLACE_RADIUS_M);
  const lonBuffer = metersToLonDegrees(NEAREST_PLACE_RADIUS_M, lat);
  const candidates = prepared<PlaceRow>(
    db,
    `SELECT id, name_en, name_si, name_ta, lat, lon FROM places
     WHERE lat BETWEEN ? AND ? AND lon BETWEEN ? AND ?`,
  ).all(lat - latBuffer, lat + latBuffer, lon - lonBuffer, lon + lonBuffer);

  let best: { row: PlaceRow; distance_m: number } | null = null;
  for (const row of candidates) {
    const distance_m = haversineMeters(lat, lon, row.lat, row.lon);
    if (distance_m <= NEAREST_PLACE_RADIUS_M && (!best || distance_m < best.distance_m)) {
      best = { row, distance_m };
    }
  }
  if (!best) return { place: null, touched: candidates.length > 0 };
  return {
    place: { id: best.row.id, name: resolveName(best.row, lang), distance_m: Math.round(best.distance_m) },
    touched: true,
  };
}

export interface ReverseGeocodeOutcome {
  result: ReverseResult;
  datasetIds: Set<string>;
}

/**
 * Core of GET /v1/reverse (docs/architecture.md §4): given a cell id already
 * resolved from lat/lon via the shared grid, reads `cell_lookup` and walks
 * the admin parent chain. Falls back to the nearest named place within 25 km
 * (bbox prefilter + haversine) when `cell_lookup` has no row for the cell —
 * this happens for off-grid/water cells, not for "the table is empty",
 * which was only true in early development; the real artifact has full
 * coverage (5,467,317 of 5,467,524 land cells) so the `if (lookup)` branch
 * is now the common case.
 *
 * Factored out of routes/reverse.ts so /v1/lookup's coordinate-pair branch
 * (docs/architecture.md §4 — "runs the reverse-geocode logic") can call the
 * exact same logic instead of duplicating it.
 */
export function reverseGeocodeAtCell(
  db: Database.Database,
  cellIdValue: number,
  lat: number,
  lon: number,
  lang: Lang,
): ReverseGeocodeOutcome {
  const lookup = prepared<CellLookupRow>(
    db,
    "SELECT gnd_pcode, postal_code, nearest_place_id, nearest_place_dist_m FROM cell_lookup WHERE cell_id = ?",
  ).get(cellIdValue);

  const datasetIds = new Set<string>(["cells"]);
  let result: ReverseResult;

  if (lookup) {
    datasetIds.add("admin-units");
    const levels = resolveAdminLevels(db, lookup.gnd_pcode, lang);
    let nearestPlace: ReverseResult["nearest_place"] = null;
    if (lookup.nearest_place_id != null) {
      const place = prepared<PlaceRow>(
        db,
        "SELECT id, name_en, name_si, name_ta, lat, lon FROM places WHERE id = ?",
      ).get(lookup.nearest_place_id);
      if (place) {
        datasetIds.add("places");
        nearestPlace = {
          id: place.id,
          name: resolveName(place, lang),
          distance_m: lookup.nearest_place_dist_m ?? Math.round(haversineMeters(lat, lon, place.lat, place.lon)),
        };
      }
    }
    if (lookup.postal_code) datasetIds.add("postal-codes");
    result = {
      cell_id: cellIdValue,
      gnd: levels.gnd,
      ds_division: levels.ds_division,
      district: levels.district,
      province: levels.province,
      postal_code: lookup.postal_code,
      nearest_place: nearestPlace,
    };
  } else {
    const fallback = nearestPlaceFallback(db, lat, lon, lang);
    if (fallback.touched) datasetIds.add("places");
    result = {
      cell_id: cellIdValue,
      gnd: null,
      ds_division: null,
      district: null,
      province: null,
      postal_code: null,
      nearest_place: fallback.place,
    };
  }

  return { result, datasetIds };
}
