// Builds the island for the hero from real data:
//   - coastline rings (country.json) → the plate's outline, resampled and lightly smoothed
//   - signed distance to coast (coast-sdf.png) → ocean shader, and distances far from the shore
//   - a stylised heightfield: a flat coastal plain, the central highlands, the named summits
//
// The island is modelled the way a relief model is milled: a plate standing a little
// proud of the sea (PLINTH) with a rounded rim (BEVEL), and the hill country rising
// smoothly out of it. The surface is a Delaunay triangulation of the coast, three rows
// of rim vertices at exact distances from it, and a jittered lattice inland.
import Delaunator from 'delaunator'
import * as THREE from 'three'
import { geo, type Polyline } from '@/lib/geo'

export const ORIGIN = { lon: 80.72, lat: 7.86 }
export const K = Math.cos((7.85 * Math.PI) / 180)
/** lon/lat (degrees) → scene x/z. One degree of latitude is one scene unit; north is −z. */
export const toXZ = (lon: number, lat: number): [number, number] => [(lon - ORIGIN.lon) * K, -(lat - ORIGIN.lat)]

/** Height of the coastal plain above the sea, in scene units. */
export const PLINTH = 0.024
/** Width of the rounded rim between the waterline and the plain, in degrees. */
export const BEVEL = 0.021
/** The elevation the colour ramp treats as a summit. Sri Pada reaches it; nothing else does. */
export const SUMMIT = 0.275

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)))
  return t * t * (3 - 2 * t)
}
const lerp = (a: number, b: number, t: number) => a + (b - a) * t

/* ---------- value noise ---------- */
function h2(x: number, y: number) {
  const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453
  return s - Math.floor(s)
}
function vnoise(x: number, y: number) {
  const xi = Math.floor(x), yi = Math.floor(y)
  const xf = x - xi, yf = y - yi
  // quintic fade: the second derivative is continuous, so shading never shows the lattice
  const u = xf * xf * xf * (xf * (xf * 6 - 15) + 10), v = yf * yf * yf * (yf * (yf * 6 - 15) + 10)
  return lerp(lerp(h2(xi, yi), h2(xi + 1, yi), u), lerp(h2(xi, yi + 1), h2(xi + 1, yi + 1), u), v)
}
function fbm(x: number, y: number, oct: number) {
  let a = 0.5, f = 1, s = 0, n = 0
  for (let i = 0; i < oct; i++) {
    s += a * vnoise(x * f, y * f)
    n += a
    a *= 0.5
    f *= 2.03
  }
  return s / n
}

export type SdfMeta = { bounds: { w: number; e: number; s: number; n: number }; width: number; height: number; range: number }

export type Terrain = {
  geometry: THREE.BufferGeometry
  sdfTexture: THREE.DataTexture
  meta: SdfMeta
  /** signed distance to the coast in degrees; positive inland */
  coastDistance: (lon: number, lat: number) => number
  /**
   * Where to stand something: never below the plain on land, so a model on the
   * shore sits on the plate rather than sliding down its rim. 0 out at sea.
   */
  heightAt: (lon: number, lat: number) => number
  /** The surface itself, rim included. Use it to drape lines on the land. */
  surfaceAt: (lon: number, lat: number) => number
  rings: Polyline[]
}

const gauss = (lon: number, lat: number, cx: number, cy: number, sx: number, sy: number) => Math.exp(-(((lon - cx) / sx) ** 2 + ((lat - cy) / sy) ** 2))

/**
 * The hill country, in scene units above the plain. Smooth by construction: a few
 * broad masses for the ranges, one soft ridge field so they read as drained slopes
 * rather than domes, and the two summits people know by name.
 */
function relief(lon: number, lat: number) {
  const mass =
    0.15 * gauss(lon, lat, 80.78, 6.9, 0.29, 0.18) + // central plateau: Nuwara Eliya, Horton Plains
    0.075 * gauss(lon, lat, 80.66, 7.23, 0.2, 0.15) + // Kandy plateau
    0.105 * gauss(lon, lat, 80.82, 7.47, 0.1, 0.125) + // Knuckles
    0.085 * gauss(lon, lat, 81.03, 6.93, 0.12, 0.16) + // Uva: Namunukula, the Badulla basin rim
    0.075 * gauss(lon, lat, 80.6, 6.45, 0.24, 0.08) + // Rakwana and Sinharaja
    0.05 * gauss(lon, lat, 80.5, 6.8, 0.17, 0.12) // Peak Wilderness, the shoulder Sri Pada stands on
  if (mass < 0.0005) return 0
  // valleys cut into the mass rather than bumps added to it: the crests stay smooth and the ranges read as drained
  const r = 1 - Math.abs(fbm(lon * 3.3 + 3.1, lat * 3.3, 2) * 2 - 1)
  let h = mass * (0.52 + 0.62 * r * r)
  // Sri Pada: the sharpest point on the island, and the highest thing in this model
  h += 0.152 * gauss(lon, lat, 80.4994, 6.8096, 0.046, 0.046)
  // Pidurutalagala: the top of the plateau, broad rather than sharp
  h += 0.034 * gauss(lon, lat, 80.773, 7.0, 0.07, 0.06)
  return h
}

/** Rounded rim: a quarter-circle from the waterline up onto the plain. */
const rim = (d: number) => {
  const t = Math.min(1, Math.max(0, d / BEVEL))
  return PLINTH * Math.sqrt(1 - (1 - t) * (1 - t))
}
/** Surface height for a point `d` degrees inland. */
const surface = (d: number, lon: number, lat: number) => (d <= 0 ? 0 : rim(d) + 0.007 * smooth(BEVEL, 0.9, d) + smooth(BEVEL * 1.2, 0.36, d) * relief(lon, lat))

function loadImageData(src: string): Promise<ImageData> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const c = document.createElement('canvas')
      c.width = img.naturalWidth
      c.height = img.naturalHeight
      const ctx = c.getContext('2d', { willReadFrequently: true })
      if (!ctx) return reject(new Error('2d context unavailable'))
      ctx.drawImage(img, 0, 0)
      resolve(ctx.getImageData(0, 0, c.width, c.height))
    }
    img.onerror = () => reject(new Error('sdf failed to load'))
    img.src = src
  })
}

/* ---------- coastline: resample, smooth, index ---------- */
/** A closed ring in projected coordinates (x = lon·K, y = lat), counter-clockwise. */
type Ring = { x: Float64Array; y: Float64Array }

/**
 * Resample a ring at an even spacing and take the sub-pixel jitter out of it. The
 * source coastline is faithful to a few hundred metres; at hero scale that reads as
 * a ragged edge, so the plate is cut to a slightly calmer curve.
 */
function refineRing(ring: Polyline, ds: number, passes: number): Ring | null {
  const closed = ring[0][0] === ring[ring.length - 1][0] && ring[0][1] === ring[ring.length - 1][1]
  const m = ring.length - (closed ? 1 : 0)
  if (m < 3) return null
  const px = new Float64Array(m + 1), py = new Float64Array(m + 1), cum = new Float64Array(m + 1)
  for (let i = 0; i <= m; i++) {
    px[i] = ring[i % m][0] * K
    py[i] = ring[i % m][1]
    if (i) cum[i] = cum[i - 1] + Math.hypot(px[i] - px[i - 1], py[i] - py[i - 1])
  }
  const total = cum[m]
  const n = Math.max(8, Math.round(total / ds))
  let x = new Float64Array(n), y = new Float64Array(n)
  for (let i = 0, k = 0; i < n; i++) {
    const s = (i / n) * total
    while (k < m - 1 && cum[k + 1] < s) k++
    const u = (s - cum[k]) / (cum[k + 1] - cum[k] || 1)
    x[i] = px[k] + (px[k + 1] - px[k]) * u
    y[i] = py[k] + (py[k + 1] - py[k]) * u
  }
  for (let p = 0; p < passes; p++) {
    const nx = new Float64Array(n), ny = new Float64Array(n)
    for (let i = 0; i < n; i++) {
      const a = (i + n - 1) % n, b = (i + 1) % n
      nx[i] = (x[a] + 2 * x[i] + x[b]) / 4
      ny[i] = (y[a] + 2 * y[i] + y[b]) / 4
    }
    x = nx
    y = ny
  }
  let area = 0
  for (let i = 0, j = n - 1; i < n; j = i++) area += x[j] * y[i] - x[i] * y[j]
  if (area < 0) {
    x.reverse()
    y.reverse()
  }
  return { x, y }
}

/** Exact distance to the refined coastline, bucketed so a query only looks at nearby segments. */
function coastIndex(rings: Ring[]) {
  const CELL = 0.04
  const seg: number[] = []
  const cells = new Map<number, number[]>()
  const key = (cx: number, cy: number) => cx * 16384 + cy
  for (const r of rings) {
    const n = r.x.length
    for (let i = 0; i < n; i++) {
      const j = (i + 1) % n
      const id = seg.length
      seg.push(r.x[i], r.y[i], r.x[j], r.y[j])
      const cx0 = Math.floor(Math.min(r.x[i], r.x[j]) / CELL), cx1 = Math.floor(Math.max(r.x[i], r.x[j]) / CELL)
      const cy0 = Math.floor(Math.min(r.y[i], r.y[j]) / CELL), cy1 = Math.floor(Math.max(r.y[i], r.y[j]) / CELL)
      for (let cx = cx0; cx <= cx1; cx++)
        for (let cy = cy0; cy <= cy1; cy++) {
          const k = key(cx, cy)
          const list = cells.get(k)
          if (list) list.push(id)
          else cells.set(k, [id])
        }
    }
  }
  /** unsigned distance to the coast, capped at `maxR` */
  const distance = (x: number, y: number, maxR: number) => {
    let best = maxR * maxR
    const cx0 = Math.floor((x - maxR) / CELL), cx1 = Math.floor((x + maxR) / CELL)
    const cy0 = Math.floor((y - maxR) / CELL), cy1 = Math.floor((y + maxR) / CELL)
    for (let cx = cx0; cx <= cx1; cx++)
      for (let cy = cy0; cy <= cy1; cy++) {
        const list = cells.get(key(cx, cy))
        if (!list) continue
        for (let q = 0; q < list.length; q++) {
          const id = list[q]
          const ax = seg[id], ay = seg[id + 1], dx = seg[id + 2] - ax, dy = seg[id + 3] - ay
          const t = Math.min(1, Math.max(0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy || 1)))
          const ex = x - ax - dx * t, ey = y - ay - dy * t
          const d2 = ex * ex + ey * ey
          if (d2 < best) best = d2
        }
      }
    return Math.sqrt(best)
  }
  const inside = (x: number, y: number) => {
    let c = false
    for (let i = 0; i < seg.length; i += 4) {
      const xi = seg[i], yi = seg[i + 1], xj = seg[i + 2], yj = seg[i + 3]
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c
    }
    return c
  }
  return { distance, inside }
}

export async function buildTerrain(detail: 'high' | 'low', onAssets?: () => void): Promise<Terrain> {
  const [{ rings }, meta, img] = await Promise.all([geo.country(), geo.sdfMeta(), loadImageData('/geo/coast-sdf.png')])
  if (onAssets) {
    // the build below is synchronous: report the milestone and let it paint first
    onAssets()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  }
  const { width: W, height: H, bounds: B, range: RANGE } = meta
  const high = detail === 'high'

  // decode sqrt-encoded signed distance into a float field
  const field = new Float32Array(W * H)
  const bytes = new Uint8Array(W * H)
  for (let i = 0; i < W * H; i++) {
    const v = img.data[i * 4]
    bytes[i] = v
    const s = (v / 255 - 0.5) * 2
    field[i] = Math.sign(s) * s * s * RANGE
  }
  const coastDistance = (lon: number, lat: number) => {
    const fx = Math.min(W - 1.001, Math.max(0, ((lon - B.w) / (B.e - B.w)) * W - 0.5))
    const fy = Math.min(H - 1.001, Math.max(0, ((B.n - lat) / (B.n - B.s)) * H - 0.5))
    const x0 = Math.floor(fx), y0 = Math.floor(fy)
    const tx = fx - x0, ty = fy - y0
    const i = y0 * W + x0
    return lerp(lerp(field[i], field[i + 1], tx), lerp(field[i + W], field[i + W + 1], tx), ty)
  }

  const surfaceAt = (lon: number, lat: number) => surface(coastDistance(lon, lat), lon, lat)
  const heightAt = (lon: number, lat: number) => {
    const d = coastDistance(lon, lat)
    // a hair of tolerance at the shore: a harbour landmark geocoded onto the quay still stands on the plate
    return d > -0.02 ? Math.max(PLINTH, surface(d, lon, lat)) : 0
  }

  /* ---------- coastline ---------- */
  const DS = high ? 0.0115 : 0.021
  const coastRings: Ring[] = []
  for (const r of rings) {
    const ring = refineRing(r, DS, high ? 7 : 3)
    if (ring) coastRings.push(ring)
  }
  const coast = coastIndex(coastRings)
  /** the distance field is good to a couple of hundred metres: settle anything closer than that against the outline itself */
  const onLand = (x: number, y: number, sdf: number) => (sdf > 0.006 ? true : sdf < -0.006 ? false : coast.inside(x, y))

  /* ---------- points: projected x, latitude, distance inland ---------- */
  const px: number[] = [], py: number[] = [], pd: number[] = []
  /** 0 the waterline · 1 the rim · 2 inland */
  const kind: number[] = []
  /** for the waterline and the rim: the unit direction that leads inland */
  const ix: number[] = [], iy: number[] = []
  const add = (x: number, y: number, d: number, k: number, nx = 0, ny = 0) => {
    px.push(x)
    py.push(y)
    pd.push(d)
    kind.push(k)
    ix.push(nx)
    iy.push(ny)
  }
  // keep rim vertices from piling up where the coast folds back on itself
  const SPACING = DS * 0.5
  const taken = new Map<number, number[]>()
  const claim = (x: number, y: number) => {
    const cx = Math.floor(x / SPACING), cy = Math.floor(y / SPACING)
    for (let ix = cx - 1; ix <= cx + 1; ix++)
      for (let iy = cy - 1; iy <= cy + 1; iy++) {
        const list = taken.get(ix * 65536 + iy)
        if (!list) continue
        for (let q = 0; q < list.length; q += 2) if (Math.hypot(list[q] - x, list[q + 1] - y) < SPACING) return false
      }
    const k = cx * 65536 + cy
    const list = taken.get(k)
    if (list) list.push(x, y)
    else taken.set(k, [x, y])
    return true
  }

  for (const r of coastRings) {
    const n = r.x.length
    for (let i = 0; i < n; i++) {
      if (!claim(r.x[i], r.y[i])) continue
      const a = (i + n - 1) % n, b = (i + 1) % n
      const tx = r.x[b] - r.x[a], ty = r.y[b] - r.y[a], l = Math.hypot(tx, ty) || 1
      // counter-clockwise ring: the land is to the left of the direction of travel
      add(r.x[i], r.y[i], 0, 0, -ty / l, tx / l)
    }
  }
  // the rim: rows at fixed distances inside the outline, so the rounded edge is cut evenly all the way round
  const ROWS = high ? [0.26, 0.6, 1, 1.55] : [0.4, 1]
  for (const row of ROWS) {
    const o = row * BEVEL
    for (const r of coastRings) {
      const n = r.x.length
      for (let i = 0; i < n; i++) {
        const a = (i + n - 1) % n, b = (i + 1) % n
        const tx = r.x[b] - r.x[a], ty = r.y[b] - r.y[a], l = Math.hypot(tx, ty) || 1
        const x = r.x[i] - (ty / l) * o, y = r.y[i] + (tx / l) * o
        if (!onLand(x, y, coastDistance(x / K, y))) continue
        const d = coast.distance(x, y, o * 1.2)
        // closer to some other stretch of coast than to its own: a spit or an inlet too narrow for this row
        if (d < o * 0.82) continue
        if (claim(x, y)) add(x, y, d, 1, -ty / l, tx / l)
      }
    }
  }
  const RIM_END = ROWS[ROWS.length - 1] * BEVEL
  const NEAR = 0.075
  const STEP = high ? 0.021 : 0.036
  let row = 0
  for (let lat = B.s; lat < B.n; lat += STEP * 0.866, row++) {
    for (let lon = B.w + (row % 2 ? STEP / (2 * K) : 0); lon < B.e; lon += STEP / K) {
      const jx = (h2(lon * 91, lat * 57) - 0.5) * STEP * 0.36
      const jy = (h2(lon * 33, lat * 77) - 0.5) * STEP * 0.36
      const x = (lon + jx / K) * K, y = lat + jy
      const sdf = coastDistance(x / K, y)
      if (sdf < RIM_END) continue
      const d = sdf > NEAR ? sdf : coast.distance(x, y, NEAR)
      if (d > RIM_END + STEP * 0.42) add(x, y, d, 2)
    }
  }
  // second, offset lattice over the hill country so ridges stay smooth
  if (high) {
    row = 0
    for (let lat = 6.2; lat < 7.8; lat += STEP * 0.866, row++) {
      for (let lon = 80.15 + (row % 2 ? 0 : STEP / (2 * K)); lon < 81.3; lon += STEP / K) {
        const x = lon + (h2(lon * 13, lat * 29) - 0.5) * STEP * 0.24
        const y = lat + STEP * 0.433 + (h2(lon * 47, lat * 11) - 0.5) * STEP * 0.24
        const d = coastDistance(x, y)
        if (d > 0.12 && relief(x, y) > 0.022) add(x * K, y, d, 2)
      }
    }
  }

  // a fan of vertices on Sri Pada, so the summit is a point and not whichever lattice vertex came nearest
  {
    const lon = 80.4994, lat = 6.8096
    add(lon * K, lat, coastDistance(lon, lat), 2)
    for (const [r, m] of [[0.011, 6], [0.024, 10], [0.04, 14]] as const)
      for (let j = 0; j < m; j++) {
        const a = (j / m) * Math.PI * 2 + r * 40
        const x = lon + (Math.cos(a) * r) / K, y = lat + Math.sin(a) * r
        add(x * K, y, coastDistance(x, y), 2)
      }
  }

  const n = px.length
  const proj = new Float64Array(n * 2)
  for (let i = 0; i < n; i++) {
    proj[i * 2] = px[i]
    proj[i * 2 + 1] = py[i]
  }
  const del = new Delaunator(proj)

  const index: number[] = []
  const t = del.triangles
  for (let i = 0; i < t.length; i += 3) {
    const a = t[i], b = t[i + 1], c = t[i + 2]
    // anything touching the rim could be bridging open water: keep it only if its middle is on land
    if (kind[a] + kind[b] + kind[c] < 6) {
      const cx = (px[a] + px[b] + px[c]) / 3, cy = (py[a] + py[b] + py[c]) / 3
      if (!onLand(cx, cy, coastDistance(cx / K, cy))) continue
    }
    // orient counter-clockwise in (lon, lat) so normals point up (+y)
    const area = (px[b] - px[a]) * (py[c] - py[a]) - (px[c] - px[a]) * (py[b] - py[a])
    if (area > 0) index.push(a, b, c)
    else index.push(a, c, b)
  }

  /* ---------- attributes ---------- */
  const position = new Float32Array(n * 3)
  /** x: baked occlusion (valleys a touch darker, crests a touch lighter) · y: wet zone −1 … dry zone +1 · z: distance inland in rim widths */
  const shade = new Float32Array(n * 3)
  for (let i = 0; i < n; i++) {
    const lon = px[i] / K, lat = py[i]
    position[i * 3] = px[i] - ORIGIN.lon * K
    position[i * 3 + 1] = kind[i] === 0 ? 0 : surface(pd[i], lon, lat)
    position[i * 3 + 2] = -(lat - ORIGIN.lat)
    shade[i * 3] = 1
    // wet zone = south-west; dry zone = north and east
    shade[i * 3 + 1] = 1 - 2 * Math.min(1, gauss(lon, lat, 80.3, 6.8, 0.8, 0.9) * 1.2)
    shade[i * 3 + 2] = Math.min(4, pd[i] / BEVEL)
  }

  const geometry = new THREE.BufferGeometry()
  geometry.setAttribute('position', new THREE.BufferAttribute(position, 3))
  geometry.setAttribute('aShade', new THREE.BufferAttribute(shade, 3))
  geometry.setIndex(index)
  geometry.computeVertexNormals()

  // Take every normal from the surface itself rather than from whichever triangles happen
  // to meet at the vertex: the shading then owes nothing to the mesh. On the rim that is the
  // slope of the quarter-circle, leaning out to sea; inland it is the heightfield's gradient.
  const normal = geometry.getAttribute('normal') as THREE.BufferAttribute
  const E = 0.018
  for (let i = 0; i < n; i++) {
    if (kind[i] !== 2) {
      // the profile is vertical at the waterline: start a little way up it so the edge still catches light from above
      const t = Math.min(1, Math.max(0.1, pd[i] / BEVEL))
      const slope = t >= 1 ? 0 : ((PLINTH / BEVEL) * (1 - t)) / Math.sqrt(1 - (1 - t) * (1 - t))
      const nx = -ix[i] * slope, nz = iy[i] * slope
      const l = Math.hypot(nx, 1, nz)
      normal.setXYZ(i, nx / l, 1 / l, nz / l)
      continue
    }
    const lon = px[i] / K, lat = py[i]
    const hE = surfaceAt(lon + E / K, lat), hW = surfaceAt(lon - E / K, lat)
    const hN = surfaceAt(lon, lat + E), hS = surfaceAt(lon, lat - E)
    const nx = -(hE - hW) / (2 * E), nz = (hN - hS) / (2 * E)
    const l = Math.hypot(nx, 1, nz)
    normal.setXYZ(i, nx / l, 1 / l, nz / l)
    const hollow = (hE + hW + hN + hS) / 4 - position[i * 3 + 1]
    shade[i * 3] = Math.min(1.08, Math.max(0.7, 1 - hollow * 95))
  }
  normal.needsUpdate = true
  geometry.computeBoundingSphere()

  const sdfTexture = new THREE.DataTexture(bytes, W, H, THREE.RedFormat, THREE.UnsignedByteType)
  sdfTexture.minFilter = THREE.LinearFilter
  sdfTexture.magFilter = THREE.LinearFilter
  sdfTexture.wrapS = sdfTexture.wrapT = THREE.ClampToEdgeWrapping
  sdfTexture.needsUpdate = true

  return { geometry, sdfTexture, meta, coastDistance, heightAt, surfaceAt, rings }
}
