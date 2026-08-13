import raw from "../data/admin.json" with { type: "json" };
import type { AdminUnit, Source } from "./types.ts";

/**
 * Row shape emitted by the foundry: a flat tuple per unit, positional to keep
 * the bundled JSON small. Kept in sync with packages/admin/data/admin.json.
 */
type UnitRow = [
  pcode: string,
  level: 0 | 1 | 2 | 3 | 4,
  nameEn: string,
  nameSi: string | null,
  nameTa: string | null,
  parentPcode: string | null,
  areaKm2: number | null,
  lat: number | null,
  lon: number | null,
];

interface AdminData {
  data_version: string;
  generated: string;
  sources: Source[];
  units: UnitRow[];
}

const data = raw as unknown as AdminData;

export const DATA_VERSION: string = data.data_version;
export const UNIT_COUNT: number = data.units.length;
export const SOURCES: Source[] = data.sources;

function rowToUnit(row: UnitRow): AdminUnit {
  const [pcode, level, name, nameSi, nameTa, parentPcode, areaKm2, lat, lon] = row;
  return {
    pcode,
    level,
    name,
    nameSi,
    nameTa,
    parentPcode,
    areaKm2,
    centroid: lat !== null && lon !== null ? { lat, lon } : null,
  };
}

interface Index {
  byPcode: Map<string, AdminUnit>;
  byParent: Map<string, AdminUnit[]>;
  byLevel: Map<number, AdminUnit[]>;
}

let index: Index | null = null;

/** Builds the lookup maps on first use so import cost stays near-zero. */
function getIndex(): Index {
  if (index) return index;
  const byPcode = new Map<string, AdminUnit>();
  const byParent = new Map<string, AdminUnit[]>();
  const byLevel = new Map<number, AdminUnit[]>();

  for (const row of data.units) {
    const unit = rowToUnit(row);
    byPcode.set(unit.pcode, unit);

    if (unit.parentPcode) {
      const siblings = byParent.get(unit.parentPcode);
      if (siblings) siblings.push(unit);
      else byParent.set(unit.parentPcode, [unit]);
    }

    const levelUnits = byLevel.get(unit.level);
    if (levelUnits) levelUnits.push(unit);
    else byLevel.set(unit.level, [unit]);
  }

  index = { byPcode, byParent, byLevel };
  return index;
}

function normalizePcode(pcode: string): string {
  return pcode.trim().toUpperCase();
}

export function lookupUnit(pcode: string): AdminUnit | null {
  return getIndex().byPcode.get(normalizePcode(pcode)) ?? null;
}

export function lookupChildren(pcode: string): AdminUnit[] {
  return getIndex().byParent.get(normalizePcode(pcode)) ?? [];
}

export function lookupLevel(level: number): AdminUnit[] {
  return getIndex().byLevel.get(level) ?? [];
}

export function allUnits(): IterableIterator<AdminUnit> {
  return getIndex().byPcode.values();
}
