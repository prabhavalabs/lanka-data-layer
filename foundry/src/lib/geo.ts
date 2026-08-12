/**
 * Minimal geometry helpers. Deliberately dependency-free (no turf): the
 * foundry only ever needs a cheap bounding-box centroid for admin units,
 * never real spatial analysis.
 */

type Position = [number, number, ...number[]];
type Geometry =
  | { type: "Point"; coordinates: Position }
  | { type: "MultiPoint" | "LineString"; coordinates: Position[] }
  | { type: "MultiLineString" | "Polygon"; coordinates: Position[][] }
  | { type: "MultiPolygon"; coordinates: Position[][][] };

/** Recursively walks a GeoJSON coordinates array, yielding every [lon, lat] pair. */
function* walkPositions(coords: unknown): Generator<Position> {
  if (!Array.isArray(coords)) return;
  if (typeof coords[0] === "number" && typeof coords[1] === "number") {
    yield coords as Position;
    return;
  }
  for (const child of coords) yield* walkPositions(child);
}

export interface BBox {
  west: number;
  south: number;
  east: number;
  north: number;
}

/** Bounding box across every coordinate in a geometry, any type/nesting depth. */
export function bboxOf(geometry: Geometry): BBox {
  let west = Infinity;
  let south = Infinity;
  let east = -Infinity;
  let north = -Infinity;
  for (const [lon, lat] of walkPositions(geometry.coordinates)) {
    if (lon < west) west = lon;
    if (lon > east) east = lon;
    if (lat < south) south = lat;
    if (lat > north) north = lat;
  }
  return { west, south, east, north };
}

/** Centroid approximated as the bbox center — cheap and adequate for admin-unit metadata. */
export function bboxCenter(geometry: Geometry): { lat: number; lon: number } {
  const { west, south, east, north } = bboxOf(geometry);
  return { lat: (south + north) / 2, lon: (west + east) / 2 };
}
