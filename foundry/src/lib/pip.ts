/**
 * Point-in-polygon (ray casting, even-odd rule) with hole support, plus a
 * bbox-indexed candidate lookup so `cell-lookup` never does O(polygons) work
 * per cell. GeoJSON winding order is irrelevant here — holes are just "any
 * ring after the first" and are always subtracted, regardless of CW/CCW.
 */
import { bboxOf } from "./geo.ts";
import { GridIndex } from "./spatial-index.ts";

export type Ring = [number, number][];
/** GeoJSON Polygon coordinates: [outer ring, ...hole rings]. */
export type PolygonCoords = Ring[];
export interface PolygonGeometry {
  type: "Polygon";
  coordinates: PolygonCoords;
}
export interface MultiPolygonGeometry {
  type: "MultiPolygon";
  coordinates: PolygonCoords[];
}
export type PolygonalGeometry = PolygonGeometry | MultiPolygonGeometry;

/** Ray-casting even-odd test against a single ring. */
export function pointInRing(lon: number, lat: number, ring: Ring): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const pi = ring[i]!;
    const pj = ring[j]!;
    const xi = pi[0];
    const yi = pi[1];
    const xj = pj[0];
    const yj = pj[1];
    const straddles = yi > lat !== yj > lat;
    if (!straddles) continue;
    const xIntersect = ((xj - xi) * (lat - yi)) / (yj - yi) + xi;
    if (lon < xIntersect) inside = !inside;
  }
  return inside;
}

/** Inside the outer ring and outside every hole ring. */
export function pointInPolygon(lon: number, lat: number, polygon: PolygonCoords): boolean {
  const outer = polygon[0];
  if (!outer || !pointInRing(lon, lat, outer)) return false;
  for (let i = 1; i < polygon.length; i++) {
    if (pointInRing(lon, lat, polygon[i]!)) return false;
  }
  return true;
}

export function pointInGeometry(lon: number, lat: number, geometry: PolygonalGeometry): boolean {
  if (geometry.type === "Polygon") return pointInPolygon(lon, lat, geometry.coordinates);
  for (const polygon of geometry.coordinates) {
    if (pointInPolygon(lon, lat, polygon)) return true;
  }
  return false;
}

/**
 * Distance from (lon,lat) to segment (a,b), in meters. Uses a local
 * equirectangular projection (accurate at the small, ~kilometer scale this
 * is used at — coastal-sliver snapping — and much cheaper than exact
 * geodesic point-to-segment distance).
 */
function pointToSegmentMeters(lon: number, lat: number, a: [number, number], b: [number, number]): number {
  const mPerDegLat = 111_320;
  const mPerDegLon = 111_320 * Math.cos((lat * Math.PI) / 180);
  const px = lon * mPerDegLon;
  const py = lat * mPerDegLat;
  const ax = a[0] * mPerDegLon;
  const ay = a[1] * mPerDegLat;
  const bx = b[0] * mPerDegLon;
  const by = b[1] * mPerDegLat;
  const dx = bx - ax;
  const dy = by - ay;
  const lenSq = dx * dx + dy * dy;
  let t = lenSq === 0 ? 0 : ((px - ax) * dx + (py - ay) * dy) / lenSq;
  t = Math.max(0, Math.min(1, t));
  const cx = ax + t * dx;
  const cy = ay + t * dy;
  return Math.hypot(px - cx, py - cy);
}

/**
 * Minimum distance in meters from (lon,lat) to the polygon's *boundary*
 * (every edge of every ring, not just vertices — a coastal-sliver point can
 * sit meters from the middle of a long edge and degrees from the nearest
 * vertex, so vertex-only distance badly overestimates proximity).
 */
export function minBoundaryDistanceMeters(lon: number, lat: number, geometry: PolygonalGeometry): number {
  const polygons = geometry.type === "Polygon" ? [geometry.coordinates] : geometry.coordinates;
  let min = Infinity;
  for (const polygon of polygons) {
    for (const ring of polygon) {
      for (let i = 0; i < ring.length - 1; i++) {
        const d = pointToSegmentMeters(lon, lat, ring[i]!, ring[i + 1]!);
        if (d < min) min = d;
      }
    }
  }
  return min;
}

export interface PolygonRecord<T> {
  id: T;
  geometry: PolygonalGeometry;
}

/**
 * Bbox-bucketed candidate index over a set of polygonal features. `contains`
 * runs the precise ring test only on bucket candidates; `nearestWithin`
 * widens the bucket search ring-by-ring (for coastal slivers whose center
 * falls outside every polygon) until a candidate within `maxMeters` is
 * found or the search gives up.
 */
export class PolygonIndex<T> {
  private readonly grid: GridIndex<PolygonRecord<T>>;

  constructor(cellSizeDeg: number) {
    this.grid = new GridIndex({ cellSizeDeg });
  }

  insert(id: T, geometry: PolygonalGeometry): void {
    const bbox = bboxOf(geometry as never);
    const record: PolygonRecord<T> = { id, geometry };
    this.grid.insert(bbox, record);
  }

  /** First candidate (bucket-restricted) whose polygon precisely contains the point, or null. */
  contains(lon: number, lat: number): T | null {
    for (const candidate of this.grid.queryPoint(lon, lat)) {
      if (pointInGeometry(lon, lat, candidate.geometry)) return candidate.id;
    }
    return null;
  }

  /**
   * Nearest polygon (by min boundary distance) within `maxMeters`, searching
   * outward from the point's own bucket. Stops expanding once the closest
   * candidate found so far is provably closer than anything a wider ring
   * could contain (ring radius in degrees converted to a conservative meter
   * floor via 1 deg ~= 111km), or once `maxMeters` worth of rings have been
   * exhausted without finding a candidate.
   */
  nearestWithin(lon: number, lat: number, maxMeters: number): T | null {
    const cellSizeDeg = this.grid.cellSizeDeg;
    const metersPerDegree = 111_320; // conservative (equator upper bound) for a safe stop condition
    const maxRadius = Math.ceil(maxMeters / (cellSizeDeg * metersPerDegree)) + 1;

    let best: { id: T; dist: number } | null = null;
    for (let radius = 0; radius <= maxRadius; radius++) {
      const candidates = this.grid.queryRing(lon, lat, radius);
      for (const candidate of candidates) {
        const d = minBoundaryDistanceMeters(lon, lat, candidate.geometry);
        if (!best || d < best.dist) best = { id: candidate.id, dist: d };
      }
      if (best) {
        // Any polygon in an as-yet-unsearched ring is at least `radius *
        // cellSizeDeg` degrees away from the query bucket's edge.
        const safeFloorMeters = radius * cellSizeDeg * metersPerDegree;
        if (safeFloorMeters >= best.dist) break;
      }
    }
    if (!best || best.dist > maxMeters) return null;
    return best.id;
  }
}
