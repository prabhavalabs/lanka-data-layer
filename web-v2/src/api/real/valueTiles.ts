// `ldl-v16` value tiles (10 §3.2, ui-contract §5.2), coloured in the browser.
//
// The server ships numbers: q = R·256 + G per pixel, value = q·scale + offset, q = 65535 is no data.
// MapLibre asks for `ldlv://<paint>/{z}/{x}/{y}` through a custom protocol; the handler fetches the
// value tile once (decoded with colour management and premultiplication off, so the bytes are the
// numbers), keeps the 16-bit array, and colours it through a 65,536-entry lookup table built from the
// layer's ramp, classes and range mask. A style change registers a new paint and swaps the tile URL:
// the tiles are recoloured from memory, never fetched again. The same arrays answer the value under the
// cursor, so hovering costs no request either.
import { addProtocol } from 'maplibre-gl'
import { absoluteApiUrl, apiUrl } from '@/api/http'

export type ValueCodec = { scale: number; offset: number; nodata: number }
/** A colour for one value, or null to leave the pixel empty. */
export type Colorer = (v: number) => [number, number, number] | null

const SIZE = 256
const MAX_TILES = 320
type Decoded = { q: Uint16Array }
const tiles = new Map<string, Promise<Decoded | null>>()
const ready = new Map<string, Decoded | null>()

function remember(url: string, p: Promise<Decoded | null>) {
  tiles.set(url, p)
  if (tiles.size > MAX_TILES) {
    const oldest = tiles.keys().next().value
    if (oldest !== undefined) {
      tiles.delete(oldest)
      ready.delete(oldest)
    }
  }
}
let scratch: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D | null = null
const ctx2d = () => {
  if (scratch) return scratch
  const c = typeof OffscreenCanvas !== 'undefined' ? new OffscreenCanvas(SIZE, SIZE) : Object.assign(document.createElement('canvas'), { width: SIZE, height: SIZE })
  scratch = c.getContext('2d', { willReadFrequently: true }) as OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D
  return scratch
}

/** One value tile as its 16-bit codes; null for a tile outside the grid (204). */
export function loadValueTile(url: string, signal?: AbortSignal): Promise<Decoded | null> {
  const hit = tiles.get(url)
  if (hit) return hit
  const p = (async () => {
    const res = await fetch(apiUrl(url), { credentials: 'omit', signal })
    if (res.status === 204 || res.status === 404) return null
    if (!res.ok) throw new Error(`${res.status} ${url}`)
    const bitmap = await createImageBitmap(await res.blob(), { premultiplyAlpha: 'none', colorSpaceConversion: 'none' })
    const c = ctx2d()
    c.clearRect(0, 0, SIZE, SIZE)
    c.drawImage(bitmap, 0, 0)
    bitmap.close()
    const px = c.getImageData(0, 0, SIZE, SIZE).data
    const q = new Uint16Array(SIZE * SIZE)
    for (let i = 0, j = 0; i < q.length; i++, j += 4) q[i] = (px[j] << 8) | px[j + 1]
    const out = { q }
    ready.set(url, out)
    return out
  })()
  p.catch(() => tiles.delete(url))
  remember(url, p)
  return p
}
export const tileUrl = (template: string, z: number, x: number, y: number) => template.replace('{z}', String(z)).replace('{x}', String(x)).replace('{y}', String(y))

/* ---------------- paints: a lookup table per style ---------------- */
type Paint = { template: string; lut: Uint32Array }
const paints = new Map<string, Paint>()
let serial = 0
const LUT_KEYS = new Map<string, string>()

/**
 * Registers how a grid is drawn and returns the tile URL template MapLibre should use. Same template and
 * same paint key → same id, so an unchanged style keeps its tiles.
 */
export function registerPaint(template: string, codec: ValueCodec, paintKey: string, color: Colorer): string {
  const key = `${template}|${paintKey}`
  let id = LUT_KEYS.get(key)
  if (!id) {
    id = `p${++serial}`
    const lut = new Uint32Array(65536)
    for (let q = 0; q < 65535; q++) {
      if (q === codec.nodata) continue
      const c = color(q * codec.scale + codec.offset)
      if (c) lut[q] = (255 << 24) | (c[2] << 16) | (c[1] << 8) | c[0]
    }
    paints.set(id, { template, lut })
    LUT_KEYS.set(key, id)
    if (paints.size > 64) {
      const first = paints.keys().next().value
      if (first !== undefined && first !== id) paints.delete(first)
    }
  }
  return `ldlv://${id}/{z}/{x}/{y}`
}

/** One tile of a registered paint, coloured: what the protocol hands MapLibre, and what previews draw. */
export async function paintTile(url: string): Promise<ImageBitmap> {
  const m = /^ldlv:\/\/(p\d+)\/(\d+)\/(\d+)\/(\d+)/.exec(url)
  const paint = m && paints.get(m[1])
  if (!m || !paint) throw new Error(`unknown value tile ${url}`)
  // shared between paints and the probe, so a cancelled draw does not cancel the fetch
  const decoded = await loadValueTile(tileUrl(paint.template, +m[2], +m[3], +m[4]))
  const img = new ImageData(SIZE, SIZE)
  if (decoded) {
    const out = new Uint32Array(img.data.buffer)
    const { q } = decoded
    const lut = paint.lut
    for (let i = 0; i < q.length; i++) out[i] = lut[q[i]]
  }
  return createImageBitmap(img)
}
let installed = false
export function installValueProtocol() {
  if (installed) return
  installed = true
  addProtocol('ldlv', async (params) => ({ data: await paintTile(params.url) }))
}
/** The z7 tiles that cover the island, with their extents, for a preview drawn outside the map. */
export function islandTiles(z = 7): { z: number; x: number; y: number; west: number; east: number; north: number; south: number }[] {
  const n = 2 ** z
  const lonOf = (x: number) => (x / n) * 360 - 180
  const latOf = (y: number) => (Math.atan(Math.sinh(Math.PI * (1 - (2 * y) / n))) * 180) / Math.PI
  const out = []
  for (let x = Math.floor(lon2x(79.5, z)); x <= Math.floor(lon2x(81.95, z)); x++)
    for (let y = Math.floor(lat2y(9.9, z)); y <= Math.floor(lat2y(5.85, z)); y++) out.push({ z, x, y, west: lonOf(x), east: lonOf(x + 1), north: latOf(y), south: latOf(y + 1) })
  return out
}

/* ---------------- reading values back ---------------- */
const lon2x = (lon: number, z: number) => ((lon + 180) / 360) * 2 ** z
const lat2y = (lat: number, z: number) => {
  const r = (lat * Math.PI) / 180
  return ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * 2 ** z
}
/** The value under a point from tiles already in memory (the zoom drawn, or any coarser one): no request. */
export function valueAt(template: string, codec: ValueCodec, lon: number, lat: number, zoom: number, minzoom: number, maxzoom: number): number | null {
  for (let z = Math.max(minzoom, Math.min(maxzoom, Math.floor(zoom))); z >= minzoom; z--) {
    const fx = lon2x(lon, z), fy = lat2y(lat, z)
    const tx = Math.floor(fx), ty = Math.floor(fy)
    const d = ready.get(tileUrl(template, z, tx, ty))
    if (d === undefined) continue
    if (d === null) return null
    const q = d.q[Math.min(SIZE - 1, Math.floor((fy - ty) * SIZE)) * SIZE + Math.min(SIZE - 1, Math.floor((fx - tx) * SIZE))]
    return q === codec.nodata ? null : q * codec.scale + codec.offset
  }
  return null
}

/** The values of the island at an overview zoom: four z7 tiles, the sample class breaks and histograms are drawn from. */
export async function overviewSample(template: string, codec: ValueCodec): Promise<Float32Array> {
  const z = 7
  const [x0, x1] = [Math.floor(lon2x(79.5, z)), Math.floor(lon2x(81.95, z))]
  const [y0, y1] = [Math.floor(lat2y(9.9, z)), Math.floor(lat2y(5.85, z))]
  const jobs: Promise<Decoded | null>[] = []
  for (let x = x0; x <= x1; x++) for (let y = y0; y <= y1; y++) jobs.push(loadValueTile(tileUrl(template, z, x, y)))
  const decoded = await Promise.all(jobs)
  const out: number[] = []
  for (const d of decoded) {
    if (!d) continue
    // every other pixel: ~10 k values per tile is plenty for breaks and a histogram
    for (let i = 0; i < d.q.length; i += 3) if (d.q[i] !== codec.nodata) out.push(d.q[i] * codec.scale + codec.offset)
  }
  return Float32Array.from(out)
}
export { absoluteApiUrl }
