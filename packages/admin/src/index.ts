/**
 * @lanka-data-layer/admin — offline lookup library for Sri Lanka's
 * administrative hierarchy (province / district / DS division / GN division),
 * keyed by official OCHA COD-AB p-codes. Data is bundled at build time; there
 * is no network access and no runtime dependency.
 */
import { lookupUnit, lookupChildren, lookupLevel, DATA_VERSION, UNIT_COUNT, SOURCES } from "./store.ts";
import { searchByName as search } from "./search.ts";
import type { AdminUnit } from "./types.ts";

export type { AdminUnit, Source } from "./types.ts";
export { DATA_VERSION, UNIT_COUNT, SOURCES };

/** Looks up a single unit by p-code. Case-insensitive; returns null when not found. */
export function getUnit(pcode: string): AdminUnit | null {
  return lookupUnit(pcode);
}

/** Direct children of a unit (e.g. the districts of a province). Empty array when none. */
export function getChildren(pcode: string): AdminUnit[] {
  return lookupChildren(pcode);
}

/**
 * The chain of ancestor units from the country root down to (but excluding)
 * the unit itself. Empty array for an unknown pcode or the country root.
 */
export function getParentChain(pcode: string): AdminUnit[] {
  const chain: AdminUnit[] = [];
  const start = lookupUnit(pcode);
  if (!start) return chain;

  let current = start;
  while (current.parentPcode) {
    const parent = lookupUnit(current.parentPcode);
    if (!parent) break;
    chain.unshift(parent);
    current = parent;
  }
  return chain;
}

/** All units at a given level (0 country … 4 GN division). */
export function unitsAtLevel(level: number): AdminUnit[] {
  return lookupLevel(level);
}

/**
 * Case-insensitive name search across English, Sinhala and Tamil names.
 * Exact match ranks above prefix match above substring match. Defaults to
 * the top 10 results; pass `opts.level` to restrict to one admin level.
 */
export function searchByName(q: string, opts?: { level?: number; limit?: number }): AdminUnit[] {
  return search(q, opts);
}
