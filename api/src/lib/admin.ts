import type Database from "better-sqlite3";
import type { AdminLevel, AdminUnit, Lang } from "@geopub/shared";
import { prepared } from "./cache.ts";
import { resolveName } from "./lang.ts";

export interface AdminUnitRow {
  pcode: string;
  level: AdminLevel;
  name_en: string;
  name_si: string | null;
  name_ta: string | null;
  parent_pcode: string | null;
  area_km2: number | null;
  centroid_lat: number | null;
  centroid_lon: number | null;
}

export function mapAdminUnit(row: AdminUnitRow, lang: Lang): AdminUnit {
  return {
    pcode: row.pcode,
    level: row.level,
    name: resolveName(row, lang),
    name_si: row.name_si,
    name_ta: row.name_ta,
    parent_pcode: row.parent_pcode,
    area_km2: row.area_km2,
    centroid:
      row.centroid_lat != null && row.centroid_lon != null
        ? { lat: row.centroid_lat, lon: row.centroid_lon }
        : null,
  };
}

export function getAdminUnitRow(db: Database.Database, pcode: string): AdminUnitRow | undefined {
  const stmt = prepared<AdminUnitRow>(
    db,
    `SELECT pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon
     FROM admin_units WHERE pcode = ?`,
  );
  return stmt.get(pcode);
}

/**
 * Ancestor chain for `pcode`, ordered from the root (country, level 0) down
 * to the immediate parent. Does not include `pcode` itself. Stops early
 * (rather than looping forever) if it revisits a pcode, guarding against a
 * malformed parent_pcode cycle in the data.
 */
export function getParentChain(db: Database.Database, pcode: string): AdminUnitRow[] {
  const chain: AdminUnitRow[] = [];
  const seen = new Set<string>([pcode]);
  let current = getAdminUnitRow(db, pcode);
  while (current?.parent_pcode && !seen.has(current.parent_pcode)) {
    const parent = getAdminUnitRow(db, current.parent_pcode);
    if (!parent) break;
    chain.push(parent);
    seen.add(parent.pcode);
    current = parent;
  }
  return chain.reverse();
}

export interface ChildSummary {
  pcode: string;
  name: string;
}

export function getChildren(db: Database.Database, pcode: string, lang: Lang): ChildSummary[] {
  const stmt = prepared<AdminUnitRow>(
    db,
    `SELECT pcode, level, name_en, name_si, name_ta, parent_pcode, area_km2, centroid_lat, centroid_lon
     FROM admin_units WHERE parent_pcode = ? ORDER BY pcode`,
  );
  return stmt.all(pcode).map((row) => ({ pcode: row.pcode, name: resolveName(row, lang) }));
}

/**
 * Buckets a GN-division pcode's ancestor chain (plus itself) into the four
 * named levels /reverse returns. Any level missing from the chain (e.g. a
 * fixture or partial build that skips a level) resolves to null rather than
 * throwing.
 */
export function resolveAdminLevels(
  db: Database.Database,
  gndPcode: string,
  lang: Lang,
): { gnd: AdminUnit | null; ds_division: AdminUnit | null; district: AdminUnit | null; province: AdminUnit | null } {
  const gndRow = getAdminUnitRow(db, gndPcode);
  const byLevel = new Map<AdminLevel, AdminUnitRow>();
  if (gndRow) {
    byLevel.set(gndRow.level, gndRow);
    for (const ancestor of getParentChain(db, gndPcode)) {
      byLevel.set(ancestor.level, ancestor);
    }
  }
  const at = (level: AdminLevel): AdminUnit | null => {
    const row = byLevel.get(level);
    return row ? mapAdminUnit(row, lang) : null;
  };
  return { gnd: at(4), ds_division: at(3), district: at(2), province: at(1) };
}
