/**
 * GeoJSON coordinate rounding + polygon simplification helpers shared by
 * `admin-geometry` (and any other step that needs to emit GeoJSON at a
 * bounded coordinate precision — docs/architecture.md's "6 dp max" rule).
 */
import { simplifyRing } from "./simplify.ts";
import type { PolygonalGeometry } from "./pip.ts";

function round(n: number, dp: number): number {
  const f = 10 ** dp;
  return Math.round(n * f) / f;
}

/**
 * Recursively rounds every coordinate value in a GeoJSON `coordinates` array
 * to `dp` decimal places, regardless of geometry type/nesting depth
 * (Point -> [lon,lat], up through MultiPolygon -> [[[[lon,lat],...]]]).
 */
export function roundCoordinates<T>(coords: T, dp: number): T {
  if (!Array.isArray(coords)) return coords;
  if (typeof coords[0] === "number") {
    return coords.map((n) => round(n as number, dp)) as unknown as T;
  }
  return coords.map((c) => roundCoordinates(c, dp)) as unknown as T;
}

/** Rounds a full geometry object's coordinates in place (returns a new object; input untouched). */
export function roundGeometry<G extends { type: string; coordinates: unknown }>(geometry: G, dp: number): G {
  return { ...geometry, coordinates: roundCoordinates(geometry.coordinates, dp) };
}

/**
 * Simplifies every ring of a Polygon/MultiPolygon at `tolerance` degrees
 * (Douglas-Peucker, see lib/simplify.ts). Holes are simplified independently
 * from their outer ring; each ring falls back to its original points if
 * simplification would degenerate it (see simplifyRing).
 */
export function simplifyPolygonalGeometry(geometry: PolygonalGeometry, tolerance: number): PolygonalGeometry {
  if (geometry.type === "Polygon") {
    return { type: "Polygon", coordinates: geometry.coordinates.map((ring) => simplifyRing(ring, tolerance)) };
  }
  return {
    type: "MultiPolygon",
    coordinates: geometry.coordinates.map((polygon) => polygon.map((ring) => simplifyRing(ring, tolerance))),
  };
}
