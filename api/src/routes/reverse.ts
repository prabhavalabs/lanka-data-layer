import type { Hono } from "hono";
import type Database from "better-sqlite3";
import { z } from "zod";
import { cellId } from "@geopub/shared";
import type { Lang, ReverseResult } from "@geopub/shared";
import { buildMeta, ok } from "../lib/envelope.ts";
import { prepared } from "../lib/cache.ts";
import { NotInCoverageError, ValidationError, formatZodIssues } from "../lib/errors.ts";
import { resolveAdminLevels } from "../lib/admin.ts";
import { resolveName } from "../lib/lang.ts";
import { haversineMeters, metersToLatDegrees, metersToLonDegrees } from "../lib/geo.ts";

const NEAREST_PLACE_RADIUS_M = 25_000;

const QuerySchema = z.object({
  lat: z.coerce.number().finite(),
  lon: z.coerce.number().finite(),
  lang: z.enum(["en", "si", "ta"]).optional().default("en"),
});

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

/**
 * GET /v1/reverse?lat&lon — cellId via the shared grid, then cell_lookup.
 * cell_lookup may be empty this phase, in which case we fall back to the
 * nearest named place within 25 km (bbox prefilter + haversine) and leave
 * the admin fields null.
 */
export function mountReverseRoute(app: Hono, db: Database.Database): void {
  app.get("/v1/reverse", (c) => {
    const parsed = QuerySchema.safeParse(c.req.query());
    if (!parsed.success) throw new ValidationError(formatZodIssues(parsed.error.issues));
    const { lat, lon, lang } = parsed.data;

    const id = cellId(lat, lon);
    if (id === null) throw new NotInCoverageError();

    const lookup = prepared<CellLookupRow>(
      db,
      "SELECT gnd_pcode, postal_code, nearest_place_id, nearest_place_dist_m FROM cell_lookup WHERE cell_id = ?",
    ).get(id);

    const datasetIds = new Set<string>(["gridded_population"]);
    let result: ReverseResult;

    if (lookup) {
      datasetIds.add("admin_boundaries");
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
      if (lookup.postal_code) datasetIds.add("postal_codes");
      result = {
        cell_id: id,
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
        cell_id: id,
        gnd: null,
        ds_division: null,
        district: null,
        province: null,
        postal_code: null,
        nearest_place: fallback.place,
      };
    }

    const meta = buildMeta(db, [...datasetIds]);
    return c.json(ok(result, meta));
  });
}
