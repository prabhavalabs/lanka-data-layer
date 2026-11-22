// Synthetic geography for the map workspace. NOTHING in here is real data.
//
// Real: the coastline, district polygons, river and rail lines and town points in /public/geo.
// Invented, from those: a terrain model, a river-flood depth surface, a population grid, DS and GN
// division shapes for Western Province (Voronoi cells around approximately placed seeds: plausible
// outlines, not the gazetted ones), a sample of land listings with asking prices, hospitals, and a
// drive-time surface. The generators are deterministic, so numbers agree between the map, the
// inspector and the analysis: the analysis really is computed from these surfaces and points.
import type { Feature, FeatureCollection, MultiPolygon, Point, Polygon } from 'geojson'
import { geo, type AdminFeature, type Place, type Polyline } from '@/lib/geo'
import { hash, rng } from './db'

/* ---------------- frames: a grid whose rows are uniform in Web-Mercator y, so it registers with the map ---------------- */
export type Frame = { key: string; w: number; h: number; west: number; east: number; south: number; north: number; yN: number; yS: number; cellKm: number }
export type Grid = Frame & { data: Float32Array }
const RAD = Math.PI / 180
const mercY = (lat: number) => Math.log(Math.tan(Math.PI / 4 + (lat * RAD) / 2))
const invMercY = (y: number) => (2 * Math.atan(Math.exp(y)) - Math.PI / 2) / RAD
function frame(key: string, west: number, south: number, east: number, north: number, cell: number): Frame {
  const yN = mercY(north), yS = mercY(south)
  return { key, west, east, south, north, yN, yS, w: Math.round((east - west) / cell), h: Math.round((yN - yS) / (cell * RAD)), cellKm: cell * 111.2 * Math.cos(7.6 * RAD) }
}
/** The whole island at ≈ 550 m: what raster layers draw. */
export const ISLAND = frame('lk', 79.5, 5.85, 81.95, 9.9, 0.005)
/** Western Province at ≈ 220 m: what the analysis aggregates over. */
export const WP = frame('wp', 79.78, 6.33, 80.32, 7.33, 0.002)
export const lonOf = (f: Frame, i: number) => f.west + ((i + 0.5) / f.w) * (f.east - f.west)
export const latOf = (f: Frame, j: number) => invMercY(f.yN - ((j + 0.5) / f.h) * (f.yN - f.yS))
const px = (f: Frame, lon: number) => ((lon - f.west) / (f.east - f.west)) * f.w
const py = (f: Frame, lat: number) => ((f.yN - mercY(lat)) / (f.yN - f.yS)) * f.h
/** Bilinear sample; NaN outside the frame or where the surface has no value. */
export function sample(g: Grid, lon: number, lat: number): number {
  const x = px(g, lon) - 0.5, y = py(g, lat) - 0.5
  if (x < -0.5 || y < -0.5 || x > g.w - 0.5 || y > g.h - 0.5) return NaN
  const x0 = Math.max(0, Math.min(g.w - 1, Math.floor(x))), y0 = Math.max(0, Math.min(g.h - 1, Math.floor(y)))
  const x1 = Math.min(g.w - 1, x0 + 1), y1 = Math.min(g.h - 1, y0 + 1)
  const fx = Math.min(1, Math.max(0, x - x0)), fy = Math.min(1, Math.max(0, y - y0))
  const a = g.data[y0 * g.w + x0], b = g.data[y0 * g.w + x1], c = g.data[y1 * g.w + x0], d = g.data[y1 * g.w + x1]
  if (Number.isNaN(a) || Number.isNaN(b) || Number.isNaN(c) || Number.isNaN(d)) return g.data[Math.round(y) * g.w + Math.round(x)]
  return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy
}
export const cellAt = (f: Frame, lon: number, lat: number) => {
  const i = Math.floor(px(f, lon)), j = Math.floor(py(f, lat))
  return i < 0 || j < 0 || i >= f.w || j >= f.h ? -1 : j * f.w + i
}

/* ---------------- plumbing ---------------- */
const memo = <T,>(fn: () => Promise<T>) => {
  let p: Promise<T> | undefined
  return () => (p ??= fn())
}
const byKey = <T,>(fn: (key: string) => Promise<T>) => {
  const m = new Map<string, Promise<T>>()
  return (key: string) => {
    let p = m.get(key)
    if (!p) m.set(key, (p = fn(key)))
    return p
  }
}
/** Long loops give the frame back every ~10 ms, so generating a surface never freezes a drag. */
function pacer() {
  let t = performance.now()
  return async () => {
    if (performance.now() - t < 10) return
    await new Promise((r) => setTimeout(r, 0))
    t = performance.now()
  }
}
const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)))
  return t * t * (3 - 2 * t)
}
const gauss = (lon: number, lat: number, cx: number, cy: number, sx: number, sy: number) => Math.exp(-(((lon - cx) / sx) ** 2 + ((lat - cy) / sy) ** 2))
function h2(x: number, y: number) {
  const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453
  return s - Math.floor(s)
}
function vnoise(x: number, y: number) {
  const xi = Math.floor(x), yi = Math.floor(y)
  const xf = x - xi, yf = y - yi
  const u = xf * xf * xf * (xf * (xf * 6 - 15) + 10), v = yf * yf * yf * (yf * (yf * 6 - 15) + 10)
  const a = h2(xi, yi), b = h2(xi + 1, yi), c = h2(xi, yi + 1), d = h2(xi + 1, yi + 1)
  return a + (b - a) * u + (c - a + (a - b + d - c) * u) * v
}
export function fbm(x: number, y: number, oct = 3) {
  let a = 0.5, f = 1, s = 0, n = 0
  for (let i = 0; i < oct; i++) {
    s += a * vnoise(x * f, y * f)
    n += a
    a *= 0.5
    f *= 2.03
  }
  return s / n
}
const KX = 111.2 * Math.cos(7.6 * RAD), KY = 110.6
export const km = (lon1: number, lat1: number, lon2: number, lat2: number) => Math.hypot((lon1 - lon2) * KX, (lat1 - lat2) * KY)

/* ---------------- rasterising vector shapes into a frame ---------------- */
function canvas2d(w: number, h: number) {
  const c = document.createElement('canvas')
  c.width = w
  c.height = h
  return c.getContext('2d', { willReadFrequently: true })!
}
function trace(ctx: CanvasRenderingContext2D, f: Frame, line: Polyline, ox = 0, oy = 0) {
  for (let k = 0; k < line.length; k++) {
    const x = px(f, line[k][0]) - ox, y = py(f, line[k][1]) - oy
    if (k === 0) ctx.moveTo(x, y)
    else ctx.lineTo(x, y)
  }
}
function alphaMask(ctx: CanvasRenderingContext2D, w: number, h: number): Uint8Array {
  const d = ctx.getImageData(0, 0, w, h).data
  const out = new Uint8Array(w * h)
  for (let i = 0; i < out.length; i++) out[i] = d[i * 4 + 3] > 127 ? 1 : 0
  return out
}
const ringsOf = (g: Polygon | MultiPolygon): Polyline[][] => (g.type === 'Polygon' ? [g.coordinates as Polyline[]] : (g.coordinates as Polyline[][]))
let scratch: CanvasRenderingContext2D | null = null
/** Every cell of the frame whose centre is inside the polygon. */
function cellsIn(f: Frame, polys: Polyline[][], visit: (cell: number) => void) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
  for (const poly of polys)
    for (const [lon, lat] of poly[0]) {
      const x = px(f, lon), y = py(f, lat)
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  const ox = Math.max(0, Math.floor(x0)), oy = Math.max(0, Math.floor(y0))
  const w = Math.min(f.w, Math.ceil(x1)) - ox, h = Math.min(f.h, Math.ceil(y1)) - oy
  if (w <= 0 || h <= 0) return
  if (!scratch || scratch.canvas.width < w || scratch.canvas.height < h) scratch = canvas2d(Math.max(w, scratch?.canvas.width ?? 0, 64), Math.max(h, scratch?.canvas.height ?? 0, 64))
  scratch.clearRect(0, 0, w, h)
  scratch.beginPath()
  for (const poly of polys) for (const ring of poly) trace(scratch, f, ring, ox, oy), scratch.closePath()
  scratch.fill('evenodd')
  const d = scratch.getImageData(0, 0, w, h).data
  for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) if (d[(j * w + i) * 4 + 3] > 127) visit((oy + j) * f.w + ox + i)
}
/** Chamfer distance, in cells, from every cell to the nearest cell where `source` is set. */
function distance(source: Uint8Array, w: number, h: number): Float32Array {
  const d = new Float32Array(w * h)
  const D = Math.SQRT2
  for (let i = 0; i < d.length; i++) d[i] = source[i] ? 0 : 1e9
  for (let j = 0; j < h; j++)
    for (let i = 0; i < w; i++) {
      const k = j * w + i
      let v = d[k]
      if (i > 0) v = Math.min(v, d[k - 1] + 1)
      if (j > 0) {
        v = Math.min(v, d[k - w] + 1)
        if (i > 0) v = Math.min(v, d[k - w - 1] + D)
        if (i < w - 1) v = Math.min(v, d[k - w + 1] + D)
      }
      d[k] = v
    }
  for (let j = h - 1; j >= 0; j--)
    for (let i = w - 1; i >= 0; i--) {
      const k = j * w + i
      let v = d[k]
      if (i < w - 1) v = Math.min(v, d[k + 1] + 1)
      if (j < h - 1) {
        v = Math.min(v, d[k + w] + 1)
        if (i < w - 1) v = Math.min(v, d[k + w + 1] + D)
        if (i > 0) v = Math.min(v, d[k + w - 1] + D)
      }
      d[k] = v
    }
  return d
}

/* ---------------- the physical model ---------------- */
/** Metres above sea level: a coastal plain, the hill country, nothing more. */
function elevation(lon: number, lat: number, coastKm: number): number {
  const massif =
    1900 * gauss(lon, lat, 80.78, 6.9, 0.29, 0.18) +
    900 * gauss(lon, lat, 80.66, 7.23, 0.2, 0.15) +
    1300 * gauss(lon, lat, 80.82, 7.47, 0.1, 0.125) +
    1100 * gauss(lon, lat, 81.03, 6.93, 0.12, 0.16) +
    950 * gauss(lon, lat, 80.6, 6.45, 0.24, 0.08) +
    700 * gauss(lon, lat, 80.5, 6.8, 0.17, 0.12)
  const ridge = 1 - Math.abs(fbm(lon * 3.3 + 3.1, lat * 3.3, 2) * 2 - 1)
  const plain = 4 + 70 * smooth(0, 55, coastKm) * (0.45 + fbm(lon * 6 + 9, lat * 6, 3))
  return plain + massif * (0.5 + 0.62 * ridge * ridge) + 260 * fbm(lon * 11, lat * 11 + 4, 3) * smooth(150, 900, massif)
}
/** Where rivers leave their banks most readily: the lower reaches of the wet-zone rivers and the eastern deltas. */
const FLOOD_PLAINS: [number, number, number, number, number][] = [
  [79.96, 6.95, 0.12, 0.045, 1.0], // Kelani lower valley: Kolonnawa, Kaduwela, Hanwella
  [80.06, 6.62, 0.15, 0.06, 1.25], // Kalu Ganga: Kalutara, Horana, Bulathsinhala
  [79.95, 7.1, 0.08, 0.07, 0.95], // Attanagalu Oya: Gampaha, Ja-Ela
  [79.93, 6.75, 0.05, 0.05, 0.7], // Bolgoda
  [79.9, 7.28, 0.1, 0.05, 0.75], // Maha Oya
  [80.2, 6.1, 0.14, 0.06, 1.0], // Gin Ganga
  [80.55, 6.02, 0.1, 0.06, 0.95], // Nilwala
  [81.2, 8.45, 0.14, 0.12, 0.9], // Mahaweli delta
  [81.65, 7.6, 0.12, 0.2, 0.8], // Batticaloa lagoons
  [80.0, 8.75, 0.16, 0.12, 0.6], // Malwathu Oya
]
function floodDepth(lon: number, lat: number, riverKm: number, elev: number): number {
  if (riverKm > 9) return 0
  let plain = 0.3
  for (const [cx, cy, sx, sy, k] of FLOOD_PLAINS) plain += 1.35 * k * gauss(lon, lat, cx, cy, sx, sy)
  const core = 2.3 * Math.exp(-riverKm / 0.85) + 1.05 * Math.exp(-riverKm / 2.6)
  const low = 1 - smooth(30, 170, elev)
  const tex = 0.5 + 1.0 * fbm(lon * 14 + 2.3, lat * 14, 3)
  return Math.max(0, core * plain * low * tex - 0.24)
}
const CBD: [number, number] = [79.8487, 6.9344]
/** Second cities: a land market of their own, far smaller than Colombo's. */
const CENTRES: [number, number, number, number][] = [
  [80.635, 7.2906, 1_050_000, 4.5], // Kandy
  [80.217, 6.0535, 700_000, 4], // Galle
  [79.838, 7.2086, 620_000, 4], // Negombo
  [80.0, 7.0917, 380_000, 3.5], // Gampaha
  [79.9607, 6.5854, 330_000, 3.5], // Kalutara
  [80.0098, 9.6615, 380_000, 4.5], // Jaffna
  [80.3647, 7.4863, 300_000, 3.5], // Kurunegala
  [80.3992, 6.6828, 220_000, 3], // Ratnapura
  [80.4037, 8.3114, 240_000, 4], // Anuradhapura
  [81.2335, 8.5874, 260_000, 3.5], // Trincomalee
  [81.7, 7.717, 240_000, 3.5], // Batticaloa
  [80.55, 5.9485, 330_000, 3.5], // Matara
]
/** LKR per perch for bare residential land, before the flood discount and before noise. */
function landValue(lon: number, lat: number, coastKm: number): number {
  const d = km(lon, lat, CBD[0], CBD[1])
  let v = 190_000 + 6_800_000 * Math.exp(-d / 6.2) + 1_250_000 * Math.exp(-d / 20)
  for (const [cx, cy, peak, s] of CENTRES) v += peak * Math.exp(-km(lon, lat, cx, cy) / s)
  v += 280_000 * Math.exp(-coastKm / 1.3) * Math.exp(-d / 110)
  return v
}
/**
 * What flooding does to an asking price, in two parts: the plot itself (nothing below ankle depth, up to
 * a quarter off at a metre and more), and the neighbourhood it stands in, which carries the reputation
 * whether or not this plot floods.
 */
const floodDiscount = (depth: number, near: number) => (1 - 0.24 * smooth(0.08, 1.3, depth)) * (1 - 0.52 * smooth(0.01, 0.34, near))
export const FLOOD_ZONE_M = 0.15

export type Fields = { frame: Frame; land: Uint8Array; coast: Float32Array; river: Float32Array; elev: Float32Array; flood: Float32Array; /** mean depth within a couple of kilometres */ near: Float32Array; pop: Float32Array; urban: Float32Array }
/** Box blur, twice, in each axis: a cheap stand-in for a Gaussian. Empty cells count as zero. */
function blur(src: Float32Array, w: number, h: number, r: number): Float32Array {
  let a = Float32Array.from(src, (v) => (v === v ? v : 0))
  let b = new Float32Array(a.length)
  for (let pass = 0; pass < 2; pass++) {
    for (let j = 0; j < h; j++) {
      let sum = 0
      for (let i = -r; i <= r; i++) sum += a[j * w + Math.max(0, Math.min(w - 1, i))]
      for (let i = 0; i < w; i++) {
        b[j * w + i] = sum / (2 * r + 1)
        sum += a[j * w + Math.min(w - 1, i + r + 1)] - a[j * w + Math.max(0, i - r)]
      }
    }
    for (let i = 0; i < w; i++) {
      let sum = 0
      for (let j = -r; j <= r; j++) sum += b[Math.max(0, Math.min(h - 1, j)) * w + i]
      for (let j = 0; j < h; j++) {
        a[j * w + i] = sum / (2 * r + 1)
        sum += b[Math.min(h - 1, j + r + 1) * w + i] - b[Math.max(0, j - r) * w + i]
      }
    }
    ;[a, b] = [a, b]
  }
  return a
}
/** Chaikin corner-cutting: the river file is coarse, and a flood plain should not have elbows. */
function soften(line: Polyline): Polyline {
  let l = line
  for (let pass = 0; pass < 2 && l.length > 2; pass++) {
    const out: Polyline = [l[0]]
    for (let i = 0; i < l.length - 1; i++) {
      const [a, b] = [l[i], l[i + 1]]
      out.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25], [a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75])
    }
    out.push(l[l.length - 1])
    l = out
  }
  return l
}
export const fields = byKey<Fields>(async (key) => {
  const f = key === 'wp' ? WP : ISLAND
  const [country, rivers, places] = await Promise.all([geo.country(), geo.rivers(), geo.places()])
  const pace = pacer()
  const n = f.w * f.h

  let ctx = canvas2d(f.w, f.h)
  ctx.beginPath()
  for (const ring of country.rings) trace(ctx, f, ring), ctx.closePath()
  ctx.fill()
  const land = alphaMask(ctx, f.w, f.h)
  const coast = distance(Uint8Array.from(land, (v) => 1 - v), f.w, f.h)
  for (let i = 0; i < n; i++) coast[i] *= f.cellKm

  ctx = canvas2d(f.w, f.h)
  ctx.lineWidth = 1.25
  ctx.lineJoin = 'round'
  ctx.beginPath()
  for (const line of rivers) trace(ctx, f, soften(line))
  ctx.stroke()
  const riverMask = alphaMask(ctx, f.w, f.h)
  const river = distance(riverMask, f.w, f.h)
  for (let i = 0; i < n; i++) river[i] *= f.cellKm
  await pace()

  const elev = new Float32Array(n), flood = new Float32Array(n), pop = new Float32Array(n), urban = new Float32Array(n)
  for (let j = 0; j < f.h; j++) {
    const lat = latOf(f, j)
    for (let i = 0; i < f.w; i++) {
      const k = j * f.w + i
      if (!land[k]) {
        flood[k] = pop[k] = NaN
        continue
      }
      const lon = lonOf(f, i)
      elev[k] = elevation(lon, lat, coast[k])
      flood[k] = floodDepth(lon, lat, river[k], elev[k])
      // countryside: denser in the wet south-west, thin in the dry zone and on the high slopes
      const sw = Math.exp(-km(lon, lat, 80.05, 6.75) / 70)
      pop[k] = (70 + 520 * sw) * (0.45 + 1.1 * fbm(lon * 7 + 1.7, lat * 7, 3)) * (1 - 0.8 * smooth(700, 1800, elev[k]))
    }
    if (j % 16 === 0) await pace()
  }
  // towns: each one a mound of people around its centre
  for (const p of places) {
    const sKm = p.kind === 'city' ? 2.6 + Math.sqrt(p.pop / 1000) * 0.2 : 1.7
    const peak = (p.pop * 0.62) / (2 * Math.PI * sKm * sKm)
    const r = Math.ceil((sKm * 3.6) / f.cellKm)
    const ci = Math.floor(px(f, p.lon)), cj = Math.floor(py(f, p.lat))
    for (let j = Math.max(0, cj - r); j <= Math.min(f.h - 1, cj + r); j++)
      for (let i = Math.max(0, ci - r); i <= Math.min(f.w - 1, ci + r); i++) {
        const k = j * f.w + i
        if (!land[k]) continue
        const g = Math.exp(-(((i - ci) * f.cellKm) ** 2 + ((j - cj) * f.cellKm) ** 2) / (2 * sKm * sKm))
        pop[k] += peak * g
        urban[k] = Math.min(1, urban[k] + g)
      }
  }
  return { frame: f, land, coast, river, elev, flood, near: blur(flood, f.w, f.h, Math.max(1, Math.round(1.7 / f.cellKm))), pop, urban }
})

/* ---------------- surfaces the catalogue serves as grids ---------------- */
const grid = (f: Frame, data: Float32Array): Grid => ({ ...f, data })
export const POP_YEARS = [2012, 2014, 2016, 2018, 2020, 2022, 2024, 2026]
export const floodGrid = memo(async () => {
  const F = await fields('lk')
  return grid(ISLAND, F.flood)
})
export const popGrid = byKey<Grid>(async (year) => {
  const F = await fields('lk')
  const dy = Number(year) - 2024
  // towns grow faster than the countryside around them
  return grid(ISLAND, Float32Array.from(F.pop, (v, i) => v * (1 + 0.008 * dy) * (1 + 0.013 * dy * F.urban[i])))
})
export const elevationGrid = memo(async () => {
  const F = await fields('lk')
  return grid(ISLAND, Float32Array.from(F.elev, (v, i) => (F.land[i] ? v : NaN)))
})
/** Shaded relief, 0 (in shadow) … 1 (lit), from the north-west. 0.5 is flat ground. */
export const reliefGrid = byKey<Grid>(async (exaggeration) => {
  const F = await fields('lk')
  const f = ISLAND
  const z = Number(exaggeration) || 1
  const out = new Float32Array(f.w * f.h)
  const cell = f.cellKm * 1000
  for (let j = 0; j < f.h; j++)
    for (let i = 0; i < f.w; i++) {
      const k = j * f.w + i
      if (!F.land[k]) {
        out[k] = NaN
        continue
      }
      const e = (ii: number, jj: number) => F.elev[Math.max(0, Math.min(f.h - 1, jj)) * f.w + Math.max(0, Math.min(f.w - 1, ii))]
      const dx = ((e(i + 1, j) - e(i - 1, j)) / (2 * cell)) * z * 3.2, dy = ((e(i, j + 1) - e(i, j - 1)) / (2 * cell)) * z * 3.2
      // light from the north-west, 45° up
      const shade = (1 - 0.7071 * dx - 0.7071 * dy) / Math.sqrt(1 + dx * dx + dy * dy) / Math.SQRT2
      out[k] = Math.min(1, Math.max(0, shade * 0.5 + 0.146))
    }
  return grid(f, out)
})

/* ---------------- towns → hospitals → a drive-time surface ---------------- */
export type Hospital = { name: string; tier: 'Teaching' | 'General' | 'Base' | 'Divisional'; beds: number; ownership: 'Government' | 'Private'; lon: number; lat: number }
export const hospitals = memo(async (): Promise<Hospital[]> => {
  const [places, F] = await Promise.all([geo.places(), fields('lk')])
  const out: Hospital[] = []
  const at = (p: Place, r: () => number, spread: number): [number, number] => {
    for (let t = 0; t < 12; t++) {
      const a = r() * Math.PI * 2, d = (0.3 + r()) * spread
      const lon = p.lon + (Math.cos(a) * d) / KX, lat = p.lat + (Math.sin(a) * d) / KY
      const c = cellAt(ISLAND, lon, lat)
      if (c >= 0 && F.land[c]) return [lon, lat]
    }
    return [p.lon, p.lat]
  }
  for (const p of places) {
    const r = rng(`hosp:${p.name}`)
    const big = p.pop > 120_000
    const first: Hospital['tier'] = p.pop > 400_000 ? 'Teaching' : big ? 'General' : p.pop > 30_000 ? 'Base' : 'Divisional'
    const count = p.pop > 400_000 ? 4 : big ? 2 : 1
    for (let i = 0; i < count; i++) {
      const tier: Hospital['tier'] = i === 0 ? first : i === 1 ? 'Base' : 'Divisional'
      const priv = i > 0 && r() < 0.45
      const [lon, lat] = at(p, r, i === 0 ? 0.6 : 2.4)
      const beds = Math.round(({ Teaching: 1200, General: 620, Base: 260, Divisional: 70 }[tier] * (0.6 + r() * 0.9) * (priv ? 0.45 : 1)) / 5) * 5
      out.push({ name: `${p.name} ${priv ? 'Private Hospital' : `${tier} Hospital`}`, tier, beds, ownership: priv ? 'Private' : 'Government', lon: +lon.toFixed(5), lat: +lat.toFixed(5) })
    }
  }
  return out
})
/** Minutes by road to the nearest base hospital or above. */
export const driveTimeGrid = memo(async () => {
  const [F, hs] = await Promise.all([fields('lk'), hospitals()])
  const f = ISLAND
  const src = new Uint8Array(f.w * f.h)
  for (const h of hs) {
    if (h.tier === 'Divisional') continue
    const c = cellAt(f, h.lon, h.lat)
    if (c >= 0) src[c] = 1
  }
  const d = distance(src, f.w, f.h)
  const out = new Float32Array(f.w * f.h)
  for (let j = 0; j < f.h; j++)
    for (let i = 0; i < f.w; i++) {
      const k = j * f.w + i
      if (!F.land[k]) {
        out[k] = NaN
        continue
      }
      // slower in the hills and wherever the road network thins out
      const kmh = (46 - 18 * smooth(200, 1400, F.elev[k])) * (0.72 + 0.5 * fbm(lonOf(f, i) * 9 + 5, latOf(f, j) * 9, 2))
      out[k] = 3 + ((d[k] * f.cellKm * 1.28) / kmh) * 60
    }
  return grid(f, out)
})

/* ---------------- Western Province: DS and GN divisions ---------------- */
type Pt = [number, number]
/** Approximate seats of the 40 DS divisions; the cells around them are this mockup's stand-in for the real outlines. */
const DSD_SEEDS: Record<string, [string, number, number][]> = {
  LK11: [
    ['Colombo', 79.862, 6.938], ['Kolonnawa', 79.897, 6.928], ['Kaduwela', 79.975, 6.918], ['Homagama', 80.01, 6.84], ['Seethawaka', 80.16, 6.93], ['Padukka', 80.09, 6.845],
    ['Maharagama', 79.93, 6.848], ['Sri Jayawardenepura Kotte', 79.905, 6.893], ['Thimbirigasyaya', 79.868, 6.893], ['Dehiwala', 79.872, 6.852], ['Ratmalana', 79.882, 6.82], ['Moratuwa', 79.885, 6.777], ['Kesbewa', 79.945, 6.795],
  ],
  LK12: [
    ['Negombo', 79.845, 7.2], ['Katana', 79.89, 7.135], ['Divulapitiya', 80.01, 7.22], ['Mirigama', 80.13, 7.24], ['Minuwangoda', 79.955, 7.168], ['Wattala', 79.885, 6.99], ['Ja-Ela', 79.9, 7.075],
    ['Gampaha', 80.0, 7.09], ['Attanagalla', 80.12, 7.11], ['Dompe', 80.06, 6.975], ['Mahara', 79.955, 7.02], ['Kelaniya', 79.915, 6.96], ['Biyagama', 79.985, 6.955],
  ],
  LK13: [
    ['Panadura', 79.915, 6.712], ['Bandaragama', 79.99, 6.715], ['Horana', 80.065, 6.72], ['Ingiriya', 80.175, 6.745], ['Bulathsinhala', 80.17, 6.665], ['Madurawala', 80.075, 6.655], ['Millaniya', 80.02, 6.668],
    ['Kalutara', 79.965, 6.585], ['Beruwala', 79.99, 6.475], ['Dodangoda', 80.02, 6.555], ['Mathugama', 80.105, 6.52], ['Agalawatta', 80.165, 6.545], ['Palindanuwara', 80.26, 6.53], ['Walallavita', 80.13, 6.44],
  ],
}
const K0 = Math.cos(6.9 * RAD)
/** Keep the part of `poly` nearer to a than to b (one half-plane of the Voronoi cell of a). */
function nearer(poly: Pt[], a: Pt, b: Pt): Pt[] {
  const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2
  const nx = (b[0] - a[0]) * K0 * K0, ny = b[1] - a[1]
  const side = (p: Pt) => (p[0] - mx) * nx + (p[1] - my) * ny
  const out: Pt[] = []
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i], q = poly[(i + 1) % poly.length]
    const sp = side(p), sq = side(q)
    if (sp <= 0) out.push(p)
    if (sp <= 0 !== sq <= 0) {
      const t = sp / (sp - sq)
      out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t])
    }
  }
  return out
}
const inside = (p: Pt, poly: Pt[]) => {
  let c = false
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i], [xj, yj] = poly[j]
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) c = !c
  }
  return c
}
const areaKm2 = (poly: Pt[]) => {
  let a = 0
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) a += (poly[j][0] - poly[i][0]) * KX * (poly[j][1] + poly[i][1]) * KY
  return Math.abs(a / 2)
}
const centroid = (poly: Pt[]): Pt => {
  let x = 0, y = 0, a = 0
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const f = poly[j][0] * poly[i][1] - poly[i][0] * poly[j][1]
    x += (poly[j][0] + poly[i][0]) * f
    y += (poly[j][1] + poly[i][1]) * f
    a += f
  }
  return a ? [x / (3 * a), y / (3 * a)] : poly[0]
}
const ring = (poly: Pt[]): Pt[] => {
  const r = poly.map(([x, y]): Pt => [+x.toFixed(5), +y.toFixed(5)])
  r.push(r[0])
  return r
}
const open = (r: Polyline): Pt[] => (r.length > 1 && r[0][0] === r[r.length - 1][0] && r[0][1] === r[r.length - 1][1] ? r.slice(0, -1) : r.slice()) as Pt[]

export type UnitProps = { pcode: string; name: string; level: number; parent: string }
export type UnitFeature = Feature<Polygon | MultiPolygon, UnitProps>
export type Units = { level: number; features: UnitFeature[]; codes: string[]; names: string[]; parents: string[]; centroids: Pt[]; areas: number[] }
const collect = (level: number, features: UnitFeature[], centroids: Pt[], areas: number[]): Units => ({
  level,
  features,
  codes: features.map((f) => f.properties.pcode),
  names: features.map((f) => f.properties.name),
  parents: features.map((f) => f.properties.parent),
  centroids,
  areas,
})
const wpUnits = memo(async () => {
  const adm2 = await geo.adm2()
  const dsd: UnitFeature[] = [], dsdC: Pt[] = [], dsdA: number[] = []
  const gn: UnitFeature[] = [], gnC: Pt[] = [], gnA: number[] = []
  for (const [district, seeds] of Object.entries(DSD_SEEDS)) {
    const feature = adm2.features.find((f) => f.properties.pcode === district)
    if (!feature) continue
    const outline = open(ringsOf(feature.geometry)[0][0])
    seeds.forEach(([name, lon, lat], si) => {
      let cell = outline
      for (let k = 0; k < seeds.length && cell.length > 2; k++) if (k !== si) cell = nearer(cell, [lon, lat], [seeds[k][1], seeds[k][2]])
      if (cell.length < 3) return
      const pcode = `${district}${String((si + 1) * 3).padStart(2, '0')}`
      const a = areaKm2(cell)
      dsd.push({ type: 'Feature', properties: { pcode, name, level: 3, parent: district }, geometry: { type: 'Polygon', coordinates: [ring(cell)] } })
      dsdC.push(centroid(cell))
      dsdA.push(a)

      // GN divisions: small in the city, larger as the land empties out
      const dC = km(lon, lat, CBD[0], CBD[1])
      const target = 0.55 + 3.6 * smooth(4, 42, dC)
      const step = Math.sqrt(target) / KY
      const r = rng(`gn:${pcode}`)
      const pts: Pt[] = []
      let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity
      for (const [x, y] of cell) {
        x0 = Math.min(x0, x)
        x1 = Math.max(x1, x)
        y0 = Math.min(y0, y)
        y1 = Math.max(y1, y)
      }
      for (let y = y0 + step * 0.3, row = 0; y < y1; y += step, row++)
        for (let x = x0 + (row % 2 ? step * 0.55 : step * 0.05); x < x1; x += step / K0) {
          const p: Pt = [x + ((r() - 0.5) * step * 0.7) / K0, y + (r() - 0.5) * step * 0.7]
          if (inside(p, cell)) pts.push(p)
        }
      if (pts.length === 0) pts.push(centroid(cell))
      pts.forEach((p, gi) => {
        let g = cell
        const reach = (step * 2.6) ** 2
        for (let k = 0; k < pts.length && g.length > 2; k++) {
          if (k === gi) continue
          const q = pts[k]
          if (((q[0] - p[0]) * K0) ** 2 + (q[1] - p[1]) ** 2 > reach) continue
          g = nearer(g, p, q)
        }
        if (g.length < 3) return
        const code = `${pcode}${String((gi + 1) * 5).padStart(3, '0')}`
        gn.push({ type: 'Feature', properties: { pcode: code, name: `${name} ${String((gi + 1) * 5).padStart(3, '0')}`, level: 4, parent: pcode }, geometry: { type: 'Polygon', coordinates: [ring(g)] } })
        gnC.push(centroid(g))
        gnA.push(areaKm2(g))
      })
    })
  }
  return { 3: collect(3, dsd, dsdC, dsdA), 4: collect(4, gn, gnC, gnA) }
})
const clean = (s: string) => s.replace(/\s+(District|Province)$/i, '')
/** Admin units at a level. Levels 3 and 4 exist for Western Province only (`parent: "LK1"`). */
export async function units(level: number, parent?: string): Promise<Units> {
  if (level >= 3) return (await wpUnits())[level === 3 ? 3 : 4]
  const fc = await (level === 1 ? geo.adm1() : geo.adm2())
  const picked = (fc.features as AdminFeature[]).filter((f) => !parent || f.properties.pcode.startsWith(parent))
  const features = picked.map((f): UnitFeature => ({ type: 'Feature', geometry: f.geometry, properties: { pcode: f.properties.pcode, name: clean(f.properties.name_en), level, parent: f.properties.parent_pcode } }))
  const main = features.map((f) => open(ringsOf(f.geometry).reduce((a, b) => (b[0].length > a[0].length ? b : a))[0]))
  return collect(level, features, main.map(centroid), features.map((f) => ringsOf(f.geometry).reduce((s, p) => s + areaKm2(open(p[0])), 0)))
}

/* ---------------- listings: a sample of land for sale ---------------- */
export type Listings = { n: number; lon: Float32Array; lat: Float32Array; price: Float32Array; perches: Float32Array; year: Uint16Array; use: Uint8Array; gn: Int32Array; district: Int16Array }
export const LAND_USE = ['Residential', 'Commercial', 'Agricultural'] as const
export const LISTING_YEARS = [2022, 2023, 2024, 2025, 2026]
/** Unit index of every WP grid cell at GN level, −1 outside the province. */
const gnCells = memo(async () => {
  const u = await units(4)
  const id = new Int32Array(WP.w * WP.h).fill(-1)
  u.features.forEach((f, i) => cellsIn(WP, ringsOf(f.geometry), (c) => void (id[c] = i)))
  return id
})
const districtCells = memo(async () => {
  const u = await units(2)
  const id = new Int16Array(ISLAND.w * ISLAND.h).fill(-1)
  u.features.forEach((f, i) => cellsIn(ISLAND, ringsOf(f.geometry), (c) => void (id[c] = i)))
  return id
})
export const listings = memo(async (): Promise<Listings> => {
  const [F, W, gnId, dId] = await Promise.all([fields('lk'), fields('wp'), gnCells(), districtCells()])
  const f = ISLAND
  // a fixed number inside the province this study is about, and a thinner sample of the rest of the island
  const N_WP = 15500, N_REST = 6500
  const N = N_WP + N_REST
  const inWp = (lon: number, lat: number) => lon > 79.8 && lon < 80.3 && lat > 6.36 && lat < 7.32
  const weight = new Float32Array(f.w * f.h)
  let sumWp = 0, sumRest = 0
  for (let j = 0; j < f.h; j++) {
    const lat = latOf(f, j)
    for (let i = 0; i < f.w; i++) {
      const k = j * f.w + i
      if (!F.land[k]) continue
      // where listings appear: where people are, flattened so the countryside is not left empty
      const wp = inWp(lonOf(f, i), lat)
      weight[k] = Math.pow(F.pop[k], wp ? 0.3 : 0.62) * (wp ? 1 : -1)
      if (wp) sumWp += weight[k]
      else sumRest -= weight[k]
    }
  }
  const cum = new Float64Array(f.w * f.h)
  let acc = 0
  for (let k = 0; k < cum.length; k++) {
    acc += weight[k] > 0 ? (weight[k] / sumWp) * N_WP : (-weight[k] / sumRest) * N_REST
    cum[k] = acc
  }
  const r = rng('listings:v1')
  const out: Listings = { n: N, lon: new Float32Array(N), lat: new Float32Array(N), price: new Float32Array(N), perches: new Float32Array(N), year: new Uint16Array(N), use: new Uint8Array(N), gn: new Int32Array(N), district: new Int16Array(N) }
  const normal = () => Math.sqrt(-2 * Math.log(1 - r())) * Math.cos(2 * Math.PI * r())
  for (let s = 0; s < N; s++) {
    const t = r() * acc
    let lo = 0, hi = cum.length - 1
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (cum[mid] < t) lo = mid + 1
      else hi = mid
    }
    const lon = lonOf(f, lo % f.w) + ((r() - 0.5) * (f.east - f.west)) / f.w, lat = latOf(f, Math.floor(lo / f.w)) + (r() - 0.5) * 0.0045
    const wc = cellAt(WP, lon, lat)
    // inside the province the finer surfaces decide; outside, the island ones
    const depth = wc >= 0 && W.land[wc] ? W.flood[wc] : F.flood[lo] || 0
    const near = wc >= 0 && W.land[wc] ? W.near[wc] : F.near[lo]
    const coast = wc >= 0 && W.land[wc] ? W.coast[wc] : F.coast[lo]
    const year = LISTING_YEARS[Math.floor(r() * LISTING_YEARS.length)]
    const use = r() < 0.78 ? 0 : r() < 0.6 ? 1 : 2
    out.lon[s] = lon
    out.lat[s] = lat
    out.year[s] = year
    out.use[s] = use
    out.perches[s] = Math.round(6 + r() * r() * 54)
    out.price[s] = Math.round((landValue(lon, lat, coast) * floodDiscount(depth, near) * Math.exp(0.2 * normal()) * [1, 1.55, 0.42][use] * Math.pow(1.07, year - 2026)) / 5000) * 5000
    out.gn[s] = wc >= 0 ? gnId[wc] : -1
    out.district[s] = dId[lo]
  }
  return out
})
/** An interpolated price surface (inverse-distance weighting), blank where no listing is within reach. */
export const priceGrid = memo(async () => {
  const [F, L] = await Promise.all([fields('lk'), listings()])
  const f = ISLAND
  const B = 0.06
  const bw = Math.ceil((f.east - f.west) / B), bh = Math.ceil((f.north - f.south) / B)
  const buckets: number[][] = Array.from({ length: bw * bh }, () => [])
  for (let s = 0; s < L.n; s++) if (L.use[s] === 0) buckets[Math.floor((L.lat[s] - f.south) / B) * bw + Math.floor((L.lon[s] - f.west) / B)]?.push(s)
  const out = new Float32Array(f.w * f.h)
  const pace = pacer()
  for (let j = 0; j < f.h; j++) {
    const lat = latOf(f, j)
    const bj = Math.floor((lat - f.south) / B)
    for (let i = 0; i < f.w; i++) {
      const k = j * f.w + i
      if (!F.land[k]) {
        out[k] = NaN
        continue
      }
      const lon = lonOf(f, i)
      const bi = Math.floor((lon - f.west) / B)
      let sw = 0, sv = 0, near = Infinity
      for (let y = Math.max(0, bj - 1); y <= Math.min(bh - 1, bj + 1); y++)
        for (let x = Math.max(0, bi - 1); x <= Math.min(bw - 1, bi + 1); x++)
          for (const s of buckets[y * bw + x]) {
            const d = km(lon, lat, L.lon[s], L.lat[s])
            if (d < near) near = d
            const w = 1 / (0.35 + d * d)
            sw += w
            // prices are brought to 2026 terms so the surface is one moment, not five
            sv += w * Math.log(L.price[s] * Math.pow(1.07, 2026 - L.year[s]))
          }
      out[k] = near < 6.5 ? Math.exp(sv / sw) : NaN
    }
    if (j % 12 === 0) await pace()
  }
  return grid(f, out)
})

/* ---------------- per-unit statistics: what the indicator frames and the analysis read ---------------- */
export type UnitStats = { floodShare: Float32Array; popDensity: Float32Array; price: Float32Array; nListings: Uint16Array; travelMin: Float32Array; meanDepth: Float32Array }
const median = (a: number[]) => {
  if (!a.length) return NaN
  const s = [...a].sort((p, q) => p - q)
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2
}
export const unitStats = byKey<UnitStats>(async (levelKey) => {
  const level = Number(levelKey)
  const u = await units(level)
  const L = await listings()
  const n = u.codes.length
  const S: UnitStats = { floodShare: new Float32Array(n), popDensity: new Float32Array(n), price: new Float32Array(n), nListings: new Uint16Array(n), travelMin: new Float32Array(n), meanDepth: new Float32Array(n) }
  if (level === 1) {
    // provinces: weighted from their districts
    const d = await unitStats('2'), du = await units(2)
    u.codes.forEach((code, i) => {
      const kids = du.codes.map((c, k) => (c.startsWith(code) ? k : -1)).filter((k) => k >= 0)
      const w = kids.map((k) => d.popDensity[k] * du.areas[k])
      const tot = w.reduce((s, x) => s + x, 0) || 1
      const avg = (col: Float32Array) => kids.reduce((s, k, m) => s + col[k] * w[m], 0) / tot
      S.floodShare[i] = avg(d.floodShare)
      S.price[i] = avg(d.price)
      S.travelMin[i] = avg(d.travelMin)
      S.meanDepth[i] = avg(d.meanDepth)
      S.popDensity[i] = tot / kids.reduce((s, k) => s + du.areas[k], 0)
      S.nListings[i] = kids.reduce((s, k) => s + d.nListings[k], 0)
    })
    return S
  }
  const f = level === 2 ? ISLAND : WP
  const F = await fields(f.key)
  const prices: number[][] = Array.from({ length: n }, () => [])
  if (level === 2) for (let s = 0; s < L.n; s++) if (L.district[s] >= 0 && L.use[s] === 0) prices[L.district[s]].push(L.price[s] * Math.pow(1.07, 2026 - L.year[s]))
  if (level === 4) for (let s = 0; s < L.n; s++) if (L.gn[s] >= 0 && L.use[s] === 0) prices[L.gn[s]].push(L.price[s] * Math.pow(1.07, 2026 - L.year[s]))
  if (level === 3) {
    const g = await units(4)
    const parent = new Map(u.codes.map((c, i) => [c, i]))
    for (let s = 0; s < L.n; s++) if (L.gn[s] >= 0 && L.use[s] === 0) prices[parent.get(g.parents[L.gn[s]])!].push(L.price[s] * Math.pow(1.07, 2026 - L.year[s]))
  }
  u.features.forEach((feat, i) => {
    let pop = 0, wet = 0, cells = 0, depth = 0, near = 0
    cellsIn(f, ringsOf(feat.geometry), (c) => {
      if (!F.land[c]) return
      cells++
      pop += F.pop[c]
      depth += F.flood[c]
      near += F.near[c]
      if (F.flood[c] >= FLOOD_ZONE_M) wet += F.pop[c]
    })
    const [lon, lat] = u.centroids[i]
    const c = cellAt(f, lon, lat)
    const r = rng(`unit:${u.codes[i]}`)
    S.floodShare[i] = pop ? (wet / pop) * 100 : 0
    S.popDensity[i] = cells ? pop / cells : 0
    S.meanDepth[i] = cells ? depth / cells : 0
    S.nListings[i] = prices[i].length
    // a median needs a few listings; below that the value is modelled from the surface, and says so
    S.price[i] = prices[i].length >= 3 ? median(prices[i]) : landValue(lon, lat, c >= 0 ? F.coast[c] : 5) * floodDiscount(S.meanDepth[i], cells ? near / cells : 0) * (0.9 + 0.2 * r())
    S.travelMin[i] = 7 + km(lon, lat, CBD[0], CBD[1]) * (1.75 + 0.7 * fbm(lon * 8, lat * 8, 2))
  })
  return S
})

/* ---------------- the flood zone and its rings, for the inside / near / outside comparison ---------------- */
/** Kilometres from each WP cell to the modelled flood zone (0 inside it). */
export const zoneDistance = memo(async () => {
  const W = await fields('wp')
  const d = distance(Uint8Array.from(W.flood, (v) => (v >= FLOOD_ZONE_M ? 1 : 0)), WP.w, WP.h)
  for (let i = 0; i < d.length; i++) d[i] *= WP.cellKm
  return grid(WP, d)
})

/* ---------------- features for the catalogue ---------------- */
const point = (lon: number, lat: number, properties: Record<string, string | number>): Feature<Point> => ({ type: 'Feature', properties, geometry: { type: 'Point', coordinates: [+lon.toFixed(5), +lat.toFixed(5)] } })
export const listingFeatures = memo(async (): Promise<FeatureCollection<Point>> => {
  const L = await listings()
  return {
    type: 'FeatureCollection',
    features: Array.from({ length: L.n }, (_, s) => point(L.lon[s], L.lat[s], { id: s, price_per_perch: L.price[s], perches: L.perches[s], land_use: LAND_USE[L.use[s]], listed_year: L.year[s] })),
  }
})
export const hospitalFeatures = memo(async (): Promise<FeatureCollection<Point>> => {
  const hs = await hospitals()
  return { type: 'FeatureCollection', features: hs.map((h, i) => point(h.lon, h.lat, { id: i, name: h.name, tier: h.tier, beds: h.beds, ownership: h.ownership })) }
})
export const placeFeatures = memo(async (): Promise<FeatureCollection<Point>> => {
  const places = await geo.places()
  return {
    type: 'FeatureCollection',
    features: [...places].sort((a, b) => b.pop - a.pop).map((p, i) => point(p.lon, p.lat, { id: i, name: p.name, kind: p.kind === 'city' ? 'City' : 'Town', population: p.pop, rank: i })),
  }
})
const lineFeatures = (lines: Polyline[], props: (line: Polyline, i: number) => Record<string, string | number>): FeatureCollection => ({
  type: 'FeatureCollection',
  features: lines.map((line, i) => ({ type: 'Feature', properties: props(line, i), geometry: { type: 'LineString', coordinates: line } })),
})
const lengthKm = (line: Polyline) => line.reduce((s, p, i) => (i ? s + km(p[0], p[1], line[i - 1][0], line[i - 1][1]) : 0), 0)
export const riverFeatures = memo(async () => {
  const rivers = await geo.rivers()
  return lineFeatures(rivers.map(soften), (line, i) => {
    const len = lengthKm(line)
    return { id: i, class: len > 22 ? 'Main river' : len > 9 ? 'River' : 'Stream', length_km: +len.toFixed(1) }
  })
})
export const railFeatures = memo(async () => {
  const rail = await geo.rail()
  return lineFeatures(rail, (line, i) => {
    const mid = line[Math.floor(line.length / 2)]
    // the lines that fan out of Colombo carry most of the traffic
    const d = km(mid[0], mid[1], CBD[0], CBD[1])
    const trains = Math.round(8 + 150 * Math.exp(-d / 28) * (0.6 + (hash(`rail:${i}`) % 100) / 125))
    return { id: i, class: d < 45 ? 'Suburban' : 'Main line', trains_per_day: trains }
  })
})
export const protectedFeatures = memo(async (): Promise<FeatureCollection> => {
  const fc = await geo.protectedAreas()
  const label: Record<string, string> = { national_park: 'National park', nature_reserve: 'Nature reserve', protected_area: 'Protected area' }
  return {
    type: 'FeatureCollection',
    features: fc.features
      .filter((f) => f.geometry)
      .map((f, i) => ({ ...f, properties: { id: i, name: String(f.properties?.name ?? 'Protected area'), designation: label[String(f.properties?.kind)] ?? 'Protected area', area_km2: +ringsOf(f.geometry as Polygon).reduce((s, p) => s + areaKm2(open(p[0])), 0).toFixed(1) } })),
  }
})
