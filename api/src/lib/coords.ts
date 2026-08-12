/**
 * Free-text coordinate-pair parsing for /v1/lookup's classifier
 * (docs/architecture.md §4, first branch): "lat,lon" or "lat lon", with
 * optional degree symbols and N/E/S/W hemisphere suffixes on either
 * number — e.g. "6.93,79.84", "6.93 79.84", "6.9344° N, 79.8428° E",
 * "6.9344N,79.8428E". Order is always lat first, lon second, matching
 * /v1/reverse's own lat/lon query params.
 *
 * This is deliberately a strict, whole-string match (^...$): the classifier
 * needs to distinguish "this whole query is a coordinate pair" from free
 * text that merely contains numbers, so a query like "flat 6 lane 2" must
 * never be misread as a coordinate.
 */
const COORD_PAIR_RE =
  /^(-?\d{1,3}(?:\.\d+)?)\s*°?\s*([NSns])?\s*[,\s]\s*(-?\d{1,3}(?:\.\d+)?)\s*°?\s*([EWew])?$/;

export interface ParsedCoordinate {
  lat: number;
  lon: number;
}

export function parseCoordinatePair(input: string): ParsedCoordinate | null {
  const match = COORD_PAIR_RE.exec(input.trim());
  if (!match) return null;
  const [, latRaw, latDir, lonRaw, lonDir] = match;

  let lat = Number.parseFloat(latRaw ?? "");
  let lon = Number.parseFloat(lonRaw ?? "");
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) return null;

  // A hemisphere letter overrides the sign rather than combining with it —
  // "6.93 S" means -6.93 regardless of whether the digits carried a "-".
  if (latDir) lat = /s/i.test(latDir) ? -Math.abs(lat) : Math.abs(lat);
  if (lonDir) lon = /w/i.test(lonDir) ? -Math.abs(lon) : Math.abs(lon);

  return { lat, lon };
}
