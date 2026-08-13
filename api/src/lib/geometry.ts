import type Database from "better-sqlite3";
import type { AdminLevel, Lang } from "@lanka-data-layer/shared";
import { prepared } from "./cache.ts";
import { resolveName } from "./lang.ts";

interface AdminGeometryRow {
  pcode: string;
  geojson: string;
  level: AdminLevel;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
}

export interface AdminGeometryFeature {
  type: "Feature";
  properties: {
    pcode: string;
    level: AdminLevel;
    name: string;
    name_si: string | null;
    name_ta: string | null;
    parent_pcode: string | null;
    area_km2: number | null;
  };
  geometry: unknown;
}

/**
 * GET /v1/admin/:pcode/geometry payload builder. `admin_geometry` stores
 * bare GeoJSON geometry (no Feature wrapper — contract §2), simplified to
 * ~0.0005° tolerance for instant map highlights, distinct from the
 * full-resolution PMTiles layers. Joined against admin_units for the name
 * fields the Feature's `properties` carries. Returns null when either the
 * geometry or the unit row is missing (14,416 of 14,417 admin_units rows
 * currently have geometry — the one gap is expected to 404, not throw).
 */
export function getAdminGeometryFeature(
  db: Database.Database,
  pcode: string,
  lang: Lang,
): AdminGeometryFeature | null {
  const row = prepared<AdminGeometryRow>(
    db,
    `SELECT ag.pcode, ag.geojson, au.level, au.name_en, au.name_si, au.name_ta, au.parent_pcode, au.area_km2
     FROM admin_geometry ag JOIN admin_units au ON au.pcode = ag.pcode
     WHERE ag.pcode = ?`,
  ).get(pcode);
  if (!row) return null;

  let geometry: unknown;
  try {
    geometry = JSON.parse(row.geojson);
  } catch {
    return null;
  }

  return {
    type: "Feature",
    properties: {
      pcode: row.pcode,
      level: row.level,
      name: resolveName(row, lang),
      name_si: row.name_si,
      name_ta: row.name_ta,
      parent_pcode: row.parent_pcode,
      area_km2: row.area_km2,
    },
    geometry,
  };
}
