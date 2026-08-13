/**
 * @lanka-data-layer/postal — offline lookup library for Sri Lankan postal
 * codes, cross-referenced against admin p-codes. Data is bundled at build
 * time; there is no network access and no runtime dependency.
 */
import { lookupCode, lookupDivision, lookupReverse, DATA_VERSION, CODE_COUNT, SOURCES } from "./store.ts";
import { searchOffices as search } from "./search.ts";
import { nearestPostalCode as nearest } from "./geo.ts";
import type { PostalCode } from "./types.ts";

export type { PostalCode, Source } from "./types.ts";
export { DATA_VERSION, CODE_COUNT, SOURCES };

/** Looks up a single postal code. Returns null when not found. */
export function getPostalCode(code: string): PostalCode | null {
  return lookupCode(code);
}

/**
 * Case-insensitive search over post office / place names. Exact match ranks
 * above prefix match above substring match. Defaults to the top 10 results.
 */
export function searchOffices(q: string, opts?: { limit?: number }): PostalCode[] {
  return search(q, opts);
}

/**
 * Postal codes serving an admin division (DS or GN division pcode), derived
 * from the reverse-geocode grid: share = fraction of the division's land
 * cells assigned to that code. Sorted by share, descending. Empty array when
 * the division has no mapped codes.
 */
export function codesForDivision(pcode: string): { code: string; share: number }[] {
  return lookupDivision(pcode).sort((a, b) => b.share - a.share);
}

/**
 * The reverse of {@link codesForDivision}: every admin division a postal
 * code serves, sorted by share, descending. Empty array when the code is
 * unknown or unmapped to any division.
 */
export function divisionsForCode(code: string): { pcode: string; share: number }[] {
  return lookupReverse(code).sort((a, b) => b.share - a.share);
}

/**
 * Nearest postal codes to a coordinate, closest first. A plain distance scan
 * over the full dataset (1,833 rows) — no spatial index needed at this
 * size. Codes with no known location are skipped. Defaults to the single
 * closest match; distances are haversine kilometers rounded to 2dp.
 */
export function nearestPostalCode(
  lat: number,
  lon: number,
  opts?: { limit?: number },
): (PostalCode & { distanceKm: number })[] {
  return nearest(lat, lon, opts);
}
