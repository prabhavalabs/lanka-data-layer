import type { PostalCode } from "./types.ts";
import { allCodes } from "./store.ts";

const EARTH_RADIUS_KM = 6371;

/** Great-circle distance between two WGS84 coordinates, in kilometers. */
function haversineKm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const dLat = ((lat2 - lat1) * Math.PI) / 180;
  const dLon = ((lon2 - lon1) * Math.PI) / 180;
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos((lat1 * Math.PI) / 180) * Math.cos((lat2 * Math.PI) / 180) * Math.sin(dLon / 2) ** 2;
  return EARTH_RADIUS_KM * 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
}

/**
 * Nearest postal codes to a coordinate, closest first. A plain O(n) scan
 * over 1,833 rows — no spatial index needed at this size. Codes with no
 * known location are skipped. Defaults to the single closest match.
 */
export function nearestPostalCode(
  lat: number,
  lon: number,
  opts?: { limit?: number },
): (PostalCode & { distanceKm: number })[] {
  const limit = opts?.limit ?? 1;
  const withDistance: (PostalCode & { distanceKm: number })[] = [];

  for (const c of allCodes()) {
    if (!c.location) continue;
    const distanceKm = Math.round(haversineKm(lat, lon, c.location.lat, c.location.lon) * 100) / 100;
    withDistance.push({ ...c, distanceKm });
  }

  withDistance.sort((a, b) => a.distanceKm - b.distanceKm);
  return withDistance.slice(0, limit);
}
