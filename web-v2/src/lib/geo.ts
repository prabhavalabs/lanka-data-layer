import type { Feature, FeatureCollection, MultiPolygon, Polygon } from 'geojson'

export type AdminProps = { pcode: string; name_en: string; name_si: string; name_ta: string; level: number; parent_pcode: string }
export type AdminFeature = Feature<Polygon | MultiPolygon, AdminProps>
export type AdminCollection = FeatureCollection<Polygon | MultiPolygon, AdminProps>
export type Place = { name: string; si: string; ta: string; kind: string; pop: number; lon: number; lat: number }
export type Polyline = [number, number][]

const cache = new Map<string, Promise<unknown>>()
function load<T>(path: string): Promise<T> {
  let p = cache.get(path)
  if (!p) {
    p = fetch(path).then((r) => {
      if (!r.ok) throw new Error(`Failed to load ${path}`)
      return r.json()
    })
    cache.set(path, p)
  }
  return p as Promise<T>
}

/** d3-geo expects clockwise exterior rings (the opposite of RFC 7946). Rewind once at load. */
function rewind<T extends FeatureCollection>(fc: T): T {
  const area = (r: number[][]) => {
    let a = 0
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) a += (r[j][0] - r[i][0]) * (r[j][1] + r[i][1])
    return a
  }
  const fix = (poly: number[][][]) => poly.forEach((ring, i) => (area(ring) < 0) === (i === 0) || ring.reverse())
  for (const f of fc.features) {
    const g = f.geometry
    if (g.type === 'Polygon') fix(g.coordinates)
    else if (g.type === 'MultiPolygon') g.coordinates.forEach(fix)
  }
  return fc
}
const rewound = new Map<string, Promise<unknown>>()
function loadPolys<T extends FeatureCollection>(path: string): Promise<T> {
  let p = rewound.get(path)
  if (!p) {
    p = load<T>(path).then(rewind)
    rewound.set(path, p)
  }
  return p as Promise<T>
}

export const geo = {
  country: () => load<{ rings: Polyline[] }>('/geo/country.json'),
  adm1: () => loadPolys<AdminCollection>('/geo/adm1.geojson'),
  adm2: () => loadPolys<AdminCollection>('/geo/adm2.geojson'),
  places: () => load<Place[]>('/geo/places.json'),
  rail: () => load<Polyline[]>('/geo/rail.json'),
  rivers: () => load<Polyline[]>('/geo/rivers.json'),
  protectedAreas: () => loadPolys<FeatureCollection>('/geo/protected.geojson'),
  sdfMeta: () => load<{ bounds: { w: number; e: number; s: number; n: number }; width: number; height: number; range: number; k: number }>('/geo/coast-sdf.json'),
}

/** Sri Lanka frame used by every map in the product. */
export const LK_BOUNDS: [[number, number], [number, number]] = [
  [79.35, 5.75],
  [82.15, 10.0],
]
export const LK_CENTER: [number, number] = [80.75, 7.85]
