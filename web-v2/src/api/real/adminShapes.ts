// DS-division shapes for the island-wide view.
//
// The engine's admin tileset holds `adm3` only from z8 (and `adm4` from z10), while an island-wide DS
// choropleth is read at z6–7. Vector sources in MapLibre cannot be asked for deeper tiles than the view
// (their tile size is fixed at 512), so the z8 tiles that cover the island (~16 requests, ~0.4 MB, cached
// by the browser) are decoded once into GeoJSON and fill the units below z8; the tiles themselves take
// over from z8, where they carry the full detail.
import { VectorTile } from '@mapbox/vector-tile'
import type { Feature, FeatureCollection } from 'geojson'
import Pbf from 'pbf'
import { apiUrl } from '@/api/http'

export const SHAPE_GATE: Record<number, number> = { 3: 8 }
const lon2x = (lon: number, z: number) => Math.floor(((lon + 180) / 360) * 2 ** z)
const lat2y = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180
  return Math.floor(((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z)
}
const round = (c: unknown): unknown => (typeof c === 'number' ? Math.round(c * 1e5) / 1e5 : Array.isArray(c) ? c.map(round) : c)
const cache = new Map<number, Promise<FeatureCollection>>()

/** Every unit of an admin level as (tile-clipped) polygons; empty for levels the tiles carry at every zoom. */
export function adminShapes(level: number): Promise<FeatureCollection> {
  const z = SHAPE_GATE[level]
  if (!z) return Promise.resolve({ type: 'FeatureCollection', features: [] })
  let p = cache.get(level)
  if (!p) {
    p = (async () => {
      const jobs: Promise<Feature[]>[] = []
      for (let x = lon2x(79.5, z); x <= lon2x(81.95, z); x++)
        for (let y = lat2y(9.9, z); y <= lat2y(5.85, z); y++)
          jobs.push(
            fetch(apiUrl(`/v2/tiles/admin/${z}/${x}/${y}.mvt`), { credentials: 'omit' }).then(async (res) => {
              if (!res.ok || res.status === 204) return []
              const tile = new VectorTile(new Pbf(new Uint8Array(await res.arrayBuffer())))
              const layer = tile.layers[`adm${level}`]
              const out: Feature[] = []
              for (let i = 0; i < (layer?.length ?? 0); i++) {
                const f = layer.feature(i).toGeoJSON(x, y, z)
                out.push({ type: 'Feature', properties: { pcode: f.properties?.pcode, name_en: f.properties?.name_en, level }, geometry: { ...f.geometry, coordinates: round((f.geometry as { coordinates: unknown }).coordinates) } as Feature['geometry'] })
              }
              return out
            }),
          )
      return { type: 'FeatureCollection', features: (await Promise.all(jobs)).flat() } as FeatureCollection
    })()
    p.catch(() => cache.delete(level))
    cache.set(level, p)
  }
  return p
}
