/**
 * Minimal bbox computation for highlight auto-fit (mapStore's `setHighlight`,
 * consumed by MapView). No turf dependency: this walks every coordinate in
 * whatever geometry the API hands back for a highlight (admin boundaries are
 * Polygon/MultiPolygon per docs/architecture.md §4's `/v1/admin/:pcode/geometry`,
 * but this stays generic over any GeoJSON geometry).
 */

/** Recursively walks a nested coordinate array (Position | Position[] | ...) and folds each [lon, lat] into the running bbox. */
function visitCoordinates(coords: unknown, bbox: [number, number, number, number]): void {
  if (Array.isArray(coords) && typeof coords[0] === "number") {
    const [lon, lat] = coords as GeoJSON.Position;
    if (lon < bbox[0]) bbox[0] = lon;
    if (lat < bbox[1]) bbox[1] = lat;
    if (lon > bbox[2]) bbox[2] = lon;
    if (lat > bbox[3]) bbox[3] = lat;
    return;
  }
  if (Array.isArray(coords)) {
    for (const c of coords) visitCoordinates(c, bbox);
  }
}

/** [west, south, east, north] bounds of a geometry, or null for an empty/degenerate one. */
export function geometryBounds(geometry: GeoJSON.Geometry): [number, number, number, number] | null {
  const bbox: [number, number, number, number] = [Infinity, Infinity, -Infinity, -Infinity];

  if (geometry.type === "GeometryCollection") {
    for (const g of geometry.geometries) {
      const sub = geometryBounds(g);
      if (!sub) continue;
      if (sub[0] < bbox[0]) bbox[0] = sub[0];
      if (sub[1] < bbox[1]) bbox[1] = sub[1];
      if (sub[2] > bbox[2]) bbox[2] = sub[2];
      if (sub[3] > bbox[3]) bbox[3] = sub[3];
    }
  } else {
    visitCoordinates(geometry.coordinates, bbox);
  }

  const [west, south, east, north] = bbox;
  if (!Number.isFinite(west) || !Number.isFinite(south) || !Number.isFinite(east) || !Number.isFinite(north)) {
    return null;
  }
  return bbox;
}

/** [west, south, east, north] bounds of a Feature's geometry, or null if it has none / is degenerate. */
export function featureBounds(feature: GeoJSON.Feature): [number, number, number, number] | null {
  if (!feature.geometry) return null;
  return geometryBounds(feature.geometry);
}
