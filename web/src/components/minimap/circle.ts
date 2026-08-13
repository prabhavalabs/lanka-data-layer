const EARTH_RADIUS_KM = 6371;

/** A `steps`-sided polygon approximating a `radiusKm` circle around (lat, lon) — no turf dependency, just the destination-point formula on a sphere. Used for the postal "catchment" dashed circle and the population radius ring. */
export function circleFeature(lat: number, lon: number, radiusKm: number, steps = 64): GeoJSON.Feature {
  const coords: [number, number][] = [];
  const latRad = (lat * Math.PI) / 180;
  const angularDistance = radiusKm / EARTH_RADIUS_KM;

  for (let i = 0; i <= steps; i++) {
    const bearing = (i / steps) * 2 * Math.PI;
    const destLat = Math.asin(
      Math.sin(latRad) * Math.cos(angularDistance) + Math.cos(latRad) * Math.sin(angularDistance) * Math.cos(bearing)
    );
    const destLon =
      (lon * Math.PI) / 180 +
      Math.atan2(
        Math.sin(bearing) * Math.sin(angularDistance) * Math.cos(latRad),
        Math.cos(angularDistance) - Math.sin(latRad) * Math.sin(destLat)
      );
    coords.push([(destLon * 180) / Math.PI, (destLat * 180) / Math.PI]);
  }

  return {
    type: "Feature",
    properties: {},
    geometry: { type: "Polygon", coordinates: [coords] },
  };
}

/** Deterministic jittered points around (lat, lon), for the population view's decorative ramp-colored scatter — not real sub-cell data (the API only returns an aggregate point/radius total), just a texture matching the design's vocabulary. Seeded so it doesn't reshuffle on every render. */
export function scatterPoints(lat: number, lon: number, radiusKm: number, count: number): { lat: number; lon: number; t: number }[] {
  const points: { lat: number; lon: number; t: number }[] = [];
  const latRad = (lat * Math.PI) / 180;
  for (let i = 0; i < count; i++) {
    // A simple deterministic pseudo-random sequence (no Math.random — stable across re-renders).
    const seed = Math.sin(i * 12.9898) * 43758.5453;
    const frac = seed - Math.floor(seed);
    const bearing = ((i * 47) % 360) * (Math.PI / 180);
    const dist = (0.25 + frac * 1.1) * radiusKm;
    const angularDistance = dist / EARTH_RADIUS_KM;
    const destLat = Math.asin(
      Math.sin(latRad) * Math.cos(angularDistance) + Math.cos(latRad) * Math.sin(angularDistance) * Math.cos(bearing)
    );
    const destLon =
      (lon * Math.PI) / 180 +
      Math.atan2(
        Math.sin(bearing) * Math.sin(angularDistance) * Math.cos(latRad),
        Math.cos(angularDistance) - Math.sin(latRad) * Math.sin(destLat)
      );
    points.push({ lat: (destLat * 180) / Math.PI, lon: (destLon * 180) / Math.PI, t: 1 - dist / (radiusKm * 1.35) });
  }
  return points;
}
