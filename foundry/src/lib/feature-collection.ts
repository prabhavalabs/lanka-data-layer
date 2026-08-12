/**
 * GeoJSON coordinate-rounding + light typing for the `layers`/`pois-extend`
 * steps. Deliberately dependency-free (no turf), matching the convention in
 * `lib/geo.ts`.
 *
 * Note: coordinate rounding for admin-geometry lives separately in
 * `lib/geojson.ts` (a different part of the pipeline, built concurrently) —
 * this file exists so the map-layer steps don't take a dependency on that
 * one's polygon-simplification machinery, which they don't need.
 */

export type Position = number[];
export type Geometry =
  | { type: "Point"; coordinates: Position }
  | { type: "MultiPoint" | "LineString"; coordinates: Position[] }
  | { type: "MultiLineString" | "Polygon"; coordinates: Position[][] }
  | { type: "MultiPolygon"; coordinates: Position[][][] };

export interface Feature<P = Record<string, unknown>> {
  type: "Feature";
  id?: string | number;
  properties: P;
  geometry: Geometry;
}
export interface FeatureCollection<P = Record<string, unknown>> {
  type: "FeatureCollection";
  features: Feature<P>[];
}

/** Rounds a single coordinate value to `dp` decimal places (default 6, per docs/architecture.md §3). */
export function roundCoord(n: number, dp = 6): number {
  const f = 10 ** dp;
  return Math.round(n * f) / f;
}

/** Recursively rounds every [lon, lat, ...] position in a coordinates array/tree to `dp` decimal places. */
export function roundCoordinates<T>(coords: T, dp = 6): T {
  if (!Array.isArray(coords)) return coords;
  if (typeof coords[0] === "number") {
    return (coords as unknown as number[]).map((v, i) => (i < 2 ? roundCoord(v, dp) : v)) as unknown as T;
  }
  return (coords as unknown as unknown[]).map((c) => roundCoordinates(c, dp)) as unknown as T;
}

/** Rounds every coordinate in a geometry (any type) to `dp` decimal places, per docs/architecture.md §3. */
export function roundGeometry<G extends Geometry>(geometry: G, dp = 6): G {
  return { ...geometry, coordinates: roundCoordinates(geometry.coordinates as never, dp) } as G;
}
