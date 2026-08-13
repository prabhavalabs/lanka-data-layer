/** Standard slippy-map tile → lon/lat bounds (Web Mercator), for the "tiles" MiniMap view's real tile-bbox rectangle. */
export function tileBounds(z: number, x: number, y: number): [number, number, number, number] | null {
  if (!Number.isFinite(z) || !Number.isFinite(x) || !Number.isFinite(y) || z < 0) return null;
  const n = 2 ** z;
  const lonAt = (col: number) => (col / n) * 360 - 180;
  const latAt = (row: number) => {
    const yRad = Math.PI - (2 * Math.PI * row) / n;
    return (180 / Math.PI) * Math.atan(0.5 * (Math.exp(yRad) - Math.exp(-yRad)));
  };
  const west = lonAt(x);
  const east = lonAt(x + 1);
  const north = latAt(y);
  const south = latAt(y + 1);
  if (![west, east, north, south].every(Number.isFinite)) return null;
  return [west, south, east, north];
}

/** The tile's bounds as a GeoJSON Polygon Feature, for drawing its rectangle on the MiniMap. */
export function tileBoundsFeature(z: number, x: number, y: number): GeoJSON.Feature | null {
  const bounds = tileBounds(z, x, y);
  if (!bounds) return null;
  const [west, south, east, north] = bounds;
  return {
    type: "Feature",
    properties: { z, x, y },
    geometry: {
      type: "Polygon",
      coordinates: [
        [
          [west, south],
          [east, south],
          [east, north],
          [west, north],
          [west, south],
        ],
      ],
    },
  };
}
