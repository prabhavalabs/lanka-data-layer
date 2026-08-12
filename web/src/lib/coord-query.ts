/**
 * Mirrors (client-side only, not authoritative) the lookup API's
 * coordinate-pair grammar — docs/architecture.md §4 / api/src/lib/coords.ts:
 * "lat,lon" or "lat lon", with optional degree symbols and N/E/S/W
 * hemisphere suffixes, e.g. "6.93,79.84", "6.93 79.84",
 * "6.9344° N, 79.8428° E". The server's classifier has the final say on
 * what actually counts as a coordinate query; this just lets the omnibox
 * decide whether to fire a search below its normal 2-char threshold (a
 * short pair like "1,1" is a complete, meaningful query even though it's
 * only 3 characters).
 */
const COORD_PAIR_RE =
  /^(-?\d{1,3}(?:\.\d+)?)\s*°?\s*([NSns])?\s*[,\s]\s*(-?\d{1,3}(?:\.\d+)?)\s*°?\s*([EWew])?$/;

export function looksLikeCoordinatePair(input: string): boolean {
  return COORD_PAIR_RE.test(input.trim());
}
