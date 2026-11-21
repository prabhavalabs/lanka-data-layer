// Builds the island for the hero from real data:
//   - coastline rings (country.json) → mesh boundary
//   - signed distance to coast (coast-sdf.png) → coastal plain, beach, ocean shader
//   - a stylised heightfield: distance-to-coast + central highlands + named peaks
// The surface is a Delaunay triangulation of coast vertices + jittered interior
// points, so the shoreline is the true (simplified) coastline, not a grid edge.
import Delaunator from 'delaunator'
import * as THREE from 'three'
import { geo, type Polyline } from '@/lib/geo'

export const ORIGIN = { lon: 80.72, lat: 7.86 }
export const K = Math.cos((7.85 * Math.PI) / 180)
/** lon/lat (degrees) → scene x/z. One degree of latitude is one scene unit; north is −z. */
export const toXZ = (lon: number, lat: number): [number, number] => [(lon - ORIGIN.lon) * K, -(lat - ORIGIN.lat)]

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
  const u = xf * xf * (3 - 2 * xf), v = yf * yf * (3 - 2 * yf)
  return lerp(lerp(h2(xi, yi), h2(xi + 1, yi), u), lerp(h2(xi, yi + 1), h2(xi + 1, yi + 1), u), v)
}
function fbm(x: number, y: number, oct = 4) {
  let a = 0.5, f = 1, s = 0, n = 0
  for (let i = 0; i < oct; i++) {
    s += a * vnoise(x * f, y * f)
    n += a
    a *= 0.5
    f *= 2.03
  }
  return s / n
}
/** ridged noise: sharp crests, good for mountain spines */
const ridged = (x: number, y: number) => 1 - Math.abs(fbm(x, y, 4) * 2 - 1)

export type SdfMeta = { bounds: { w: number; e: number; s: number; n: number }; width: number; height: number; range: number }

export type Terrain = {
  geometry: THREE.BufferGeometry
  sdfTexture: THREE.DataTexture
  meta: SdfMeta
  /** signed distance to the coast in degrees; positive inland */
  coastDistance: (lon: number, lat: number) => number
  /** terrain height in scene units; 0 at sea */
  heightAt: (lon: number, lat: number) => number
  rings: Polyline[]
}

const gauss = (lon: number, lat: number, cx: number, cy: number, sx: number, sy: number) => Math.exp(-(((lon - cx) / sx) ** 2 + ((lat - cy) / sy) ** 2))

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

export async function buildTerrain(detail: 'high' | 'low', onAssets?: () => void): Promise<Terrain> {
  const [{ rings }, meta, img] = await Promise.all([geo.country(), geo.sdfMeta(), loadImageData('/geo/coast-sdf.png')])
  if (onAssets) {
    // the build below is synchronous: report the milestone and let it paint first
    onAssets()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  }
  const { width: W, height: H, bounds: B, range: RANGE } = meta

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

  const heightAt = (lon: number, lat: number) => {
    const d = coastDistance(lon, lat)
    if (d <= 0) return 0
    const coast = smooth(0.0, 0.2, d)
    const inland = smooth(0.0, 0.5, d)
    // central highlands massif, the Knuckles range to its north, the Rakwana ridge to the south-west
    const massif = 0.27 * gauss(lon, lat, 80.79, 6.93, 0.36, 0.29) + 0.11 * gauss(lon, lat, 80.84, 7.43, 0.15, 0.2) + 0.07 * gauss(lon, lat, 80.56, 6.5, 0.26, 0.13)
    const crest = ridged(lon * 3.4 + 3.1, lat * 3.4)
    const detail = fbm(lon * 17, lat * 17, 3)
    let h = 0.004 + coast * 0.01 + inland * 0.02 * fbm(lon * 2.6, lat * 2.6, 3)
    h += massif * coast * (0.5 + 0.8 * crest * crest + 0.1 * detail)
    // low inselberg country of the dry zone
    h += inland * 0.016 * Math.max(0, fbm(lon * 9 + 9, lat * 9, 3) - 0.56) * 8
    // named summits: Sri Pada (Adam's Peak) stands apart as the sharpest; Pidurutalagala is the highest mass
    h += 0.2 * gauss(lon, lat, 80.4994, 6.8096, 0.05, 0.05)
    h += 0.09 * gauss(lon, lat, 80.4994, 6.8096, 0.14, 0.1)
    h += 0.11 * gauss(lon, lat, 80.773, 7.0, 0.075, 0.07)
    return h
  }

  /* ---------- points ---------- */
  const pts: number[] = [] // lon, lat pairs
  const isCoast: boolean[] = []
  const MAXSEG = detail === 'high' ? 0.02 : 0.034
  for (const ring of rings) {
    for (let i = 0; i < ring.length - 1; i++) {
      const [ax, ay] = ring[i], [bx, by] = ring[i + 1]
      const n = Math.max(1, Math.ceil(Math.hypot((bx - ax) * K, by - ay) / MAXSEG))
      for (let s = 0; s < n; s++) {
        pts.push(lerp(ax, bx, s / n), lerp(ay, by, s / n))
        isCoast.push(true)
      }
    }
  }
  const STEP = detail === 'high' ? 0.021 : 0.036
  let row = 0
  for (let lat = B.s; lat < B.n; lat += STEP * 0.866, row++) {
    for (let lon = B.w + (row % 2 ? STEP / (2 * K) : 0); lon < B.e; lon += STEP / K) {
      const jx = (h2(lon * 91, lat * 57) - 0.5) * STEP * 0.5
      const jy = (h2(lon * 33, lat * 77) - 0.5) * STEP * 0.5
      const x = lon + jx / K, y = lat + jy
      if (coastDistance(x, y) > STEP * 0.42) {
        pts.push(x, y)
        isCoast.push(false)
      }
    }
  }
  // second, offset lattice over the hill country so ridges stay smooth
  if (detail === 'high') {
    row = 0
    for (let lat = 6.25; lat < 7.75; lat += STEP * 0.866, row++) {
      for (let lon = 80.2 + (row % 2 ? 0 : STEP / (2 * K)); lon < 81.25; lon += STEP / K) {
        const x = lon + (h2(lon * 13, lat * 29) - 0.5) * STEP * 0.3
        const y = lat + STEP * 0.433 + (h2(lon * 47, lat * 11) - 0.5) * STEP * 0.3
        if (coastDistance(x, y) > 0.12 && heightAt(x, y) > 0.05) {
          pts.push(x, y)
          isCoast.push(false)
        }
      }
    }
  }

  const n = pts.length / 2
  const proj = new Float64Array(n * 2)
  for (let i = 0; i < n; i++) {
    proj[i * 2] = pts[i * 2] * K
    proj[i * 2 + 1] = pts[i * 2 + 1]
  }
  const del = new Delaunator(proj)

  const inside = (x: number, y: number) => {
    let c = false
    for (const r of rings)
      for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
        const xi = r[i][0], yi = r[i][1], xj = r[j][0], yj = r[j][1]
        if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c
      }
    return c
  }

  const index: number[] = []
  const t = del.triangles
  for (let i = 0; i < t.length; i += 3) {
    const a = t[i], b = t[i + 1], c = t[i + 2]
    const coastCount = +isCoast[a] + +isCoast[b] + +isCoast[c]
    if (coastCount >= 2) {
      const cx = (pts[a * 2] + pts[b * 2] + pts[c * 2]) / 3
      const cy = (pts[a * 2 + 1] + pts[b * 2 + 1] + pts[c * 2 + 1]) / 3
      if (!inside(cx, cy)) continue
    }
    // orient counter-clockwise in (lon, lat) so normals point up (+y)
    const area = (pts[b * 2] - pts[a * 2]) * (pts[c * 2 + 1] - pts[a * 2 + 1]) - (pts[c * 2] - pts[a * 2]) * (pts[b * 2 + 1] - pts[a * 2 + 1])
    if (area > 0) index.push(a, b, c)
    else index.push(a, c, b)
  }

  /* ---------- attributes ---------- */
  const position = new Float32Array(n * 3)
  const color = new Float32Array(n * 3)
  const C = (hex: string) => new THREE.Color(hex)
  const sand = C('#e6d3a3'), lowWet = C('#6fb04f'), lowDry = C('#a3ad5c'), paddy = C('#93c35a'), forest = C('#2f7a45'), tea = C('#7fbf5c'), rock = C('#9a9a84'), scrub = C('#b9a968')
  const tmp = new THREE.Color()
  for (let i = 0; i < n; i++) {
    const lon = pts[i * 2], lat = pts[i * 2 + 1]
    const d = Math.max(0, coastDistance(lon, lat))
    const h = isCoast[i] ? 0.003 : heightAt(lon, lat)
    const [x, z] = toXZ(lon, lat)
    position[i * 3] = x
    position[i * 3 + 1] = h
    position[i * 3 + 2] = z

    // wet zone = south-west; dry zone = north and east
    const wet = Math.min(1, gauss(lon, lat, 80.25, 6.75, 0.75, 0.85) * 1.25)
    const nz = fbm(lon * 11, lat * 11, 3)
    tmp.copy(lowDry).lerp(scrub, smooth(0.45, 0.8, nz) * (1 - wet) * 0.8)
    tmp.lerp(lowWet, wet)
    tmp.lerp(paddy, smooth(0.5, 0.75, fbm(lon * 6 + 4, lat * 6, 2)) * 0.45)
    tmp.lerp(forest, smooth(0.035, 0.11, h) * 0.9)
    tmp.lerp(tea, smooth(0.16, 0.26, h) * smooth(0.4, 0.7, nz) * 0.75)
    tmp.lerp(rock, smooth(0.3, 0.46, h) * 0.7)
    tmp.lerp(sand, 1 - smooth(0.004, 0.028, d))
    const shade = 0.9 + nz * 0.2
    color[i * 3] = tmp.r * shade
    color[i * 3 + 1] = tmp.g * shade
    color[i * 3 + 2] = tmp.b * shade
  }

  const geometry = new THREE.BufferGeometry()
  geometry.setAttribute('position', new THREE.BufferAttribute(position, 3))
  geometry.setAttribute('color', new THREE.BufferAttribute(color, 3))
  geometry.setIndex(index)
  geometry.computeVertexNormals()
  geometry.computeBoundingSphere()

  const sdfTexture = new THREE.DataTexture(bytes, W, H, THREE.RedFormat, THREE.UnsignedByteType)
  sdfTexture.minFilter = THREE.LinearFilter
  sdfTexture.magFilter = THREE.LinearFilter
  sdfTexture.wrapS = sdfTexture.wrapT = THREE.ClampToEdgeWrapping
  sdfTexture.needsUpdate = true

  return { geometry, sdfTexture, meta, coastDistance, heightAt, rings }
}
