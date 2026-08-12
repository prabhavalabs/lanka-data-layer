/**
 * Small geo helpers that are NOT part of the canonical grid contract
 * (docs/architecture.md §1). The grid formula itself must only ever live in
 * shared/src/grid.ts — this file only holds the plain great-circle distance
 * math needed for the /reverse nearest-place fallback and the /population
 * radius sum, neither of which are pinned by the contract.
 */

const EARTH_RADIUS_M = 6_371_000;
const METERS_PER_DEGREE_LAT = 111_320;

function toRadians(deg: number): number {
  return (deg * Math.PI) / 180;
}

/** Great-circle distance between two WGS84 points, in meters. */
export function haversineMeters(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const dLat = toRadians(lat2 - lat1);
  const dLon = toRadians(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRadians(lat1)) * Math.cos(toRadians(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_M * c;
}

/** Degrees of latitude spanning `meters` — constant everywhere on the ellipsoid (to our precision needs). */
export function metersToLatDegrees(meters: number): number {
  return meters / METERS_PER_DEGREE_LAT;
}

/** Inverse of metersToLatDegrees. */
export function latDegreesToMeters(degrees: number): number {
  return degrees * METERS_PER_DEGREE_LAT;
}

/** Degrees of longitude spanning `meters` at a given latitude (shrinks toward the poles). */
export function metersToLonDegrees(meters: number, atLat: number): number {
  const metersPerDegreeLon = METERS_PER_DEGREE_LAT * Math.cos(toRadians(atLat));
  if (metersPerDegreeLon <= 0) return 180;
  return meters / metersPerDegreeLon;
}
