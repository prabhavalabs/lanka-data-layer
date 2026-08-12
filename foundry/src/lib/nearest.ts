/**
 * Nearest-point-by-haversine over a small-to-medium point set (postal codes,
 * places), backed by the same grid-bucket index as `pip.ts` so a lookup
 * touches a handful of candidates instead of scanning every point. Used by
 * `cell-lookup` for ~5M queries against ~1-2k points each — an O(n) scan per
 * cell would dominate the build; bucketing keeps it to O(1)-ish.
 */
import { haversineMeters } from "./geo.ts";
import { GridIndex } from "./spatial-index.ts";

interface Point<T> {
  id: T;
  lat: number;
  lon: number;
}

const METERS_PER_DEGREE = 111_320; // conservative (equatorial) upper bound, used only for stop conditions

export class NearestIndex<T> {
  private readonly grid: GridIndex<Point<T>>;
  private count = 0;

  /**
   * `cellSizeDeg` should be roughly the point set's median nearest-neighbor
   * spacing — too small means many empty rings to expand through, too large
   * means large candidate lists per bucket.
   */
  constructor(cellSizeDeg: number) {
    this.grid = new GridIndex({ cellSizeDeg });
  }

  insert(id: T, lat: number, lon: number): void {
    this.grid.insert({ west: lon, east: lon, south: lat, north: lat }, { id, lat, lon });
    this.count++;
  }

  /** Nearest point to (lat, lon) by great-circle distance, or null if the index is empty. */
  nearest(lat: number, lon: number): { id: T; distM: number } | null {
    if (this.count === 0) return null;
    const cellSizeDeg = this.grid.cellSizeDeg;
    // Country-scale safety cap: a ~6deg span (comfortably more than Sri
    // Lanka's ~4.2 x 2.7deg bbox diagonal) is always enough rings to find
    // *some* candidate if any exist.
    const maxRadius = Math.ceil(6 / cellSizeDeg) + 1;

    let best: { id: T; dist: number } | null = null;
    for (let radius = 0; radius <= maxRadius; radius++) {
      const candidates = this.grid.queryRing(lon, lat, radius);
      for (const c of candidates) {
        const d = haversineMeters(lat, lon, c.lat, c.lon);
        if (!best || d < best.dist) best = { id: c.id, dist: d };
      }
      if (best) {
        // Anything in a still-unsearched ring is at least `radius` whole
        // buckets away from the query bucket's edge — once that floor
        // exceeds the best distance found so far, no wider ring can improve it.
        const safeFloorMeters = radius * cellSizeDeg * METERS_PER_DEGREE;
        if (safeFloorMeters >= best.dist) break;
      }
    }
    return best ? { id: best.id, distM: best.dist } : null;
  }
}
