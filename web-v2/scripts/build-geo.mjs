// Derives small, quantized geometry for the web-v2 mockups from the foundry
// layer artifacts (gitignored source data). Run: pnpm --filter @lanka-data-layer/web-v2 geo
//
// Outputs (web-v2/public/geo):
//   country.json        coastline rings [[lon,lat],...] (main island + larger islets)
//   adm1.geojson        provinces, topology-aware simplification
//   adm2.geojson        districts, topology-aware simplification
//   places.json         top places with trilingual names + population
//   rail.json           railway polylines
//   rivers.json         major river polylines
//   protected.geojson   protected areas
//   coast-sdf.png       signed distance to coast (8-bit grey, sqrt-encoded) for terrain + ocean shader
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync, statSync, rmSync } from 'node:fs'
import { deflateSync } from 'node:zlib'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const here = dirname(fileURLToPath(import.meta.url))
const SRC = resolve(here, '../../foundry/data/artifacts/layers')
const OUT = resolve(here, '../public/geo')
const TMP = resolve(here, '../.geo-tmp')
mkdirSync(OUT, { recursive: true })
mkdirSync(TMP, { recursive: true })

const require = createRequire(import.meta.url)
const mapshaperBin = join(dirname(require.resolve('mapshaper/package.json')), 'bin/mapshaper')
const ms = (...args) => execFileSync(process.execPath, [mapshaperBin, ...args], { stdio: ['ignore', 'ignore', 'inherit'] })
const read = (p) => JSON.parse(readFileSync(p, 'utf8'))
const round = (n, d = 4) => Math.round(n * 10 ** d) / 10 ** d
const kb = (p) => (statSync(p).size / 1024).toFixed(1) + ' KB'

// ---------- admin boundaries (shared borders stay shared) ----------
for (const [name, pct] of [['adm1', '22%'], ['adm2', '26%']]) {
  const out = join(OUT, `${name}.geojson`)
  ms(join(SRC, `admin-${name}.geojson`), '-simplify', pct, 'keep-shapes', '-filter-islands', 'min-area=4km2', '-o', 'precision=0.0001', 'force', out)
  console.log(name, kb(out))
}

// ---------- country coastline ----------
{
  const tmp = join(TMP, 'country.geojson')
  ms(join(SRC, 'country.geojson'), '-simplify', '75%', 'keep-shapes', '-filter-islands', 'min-area=3km2', '-o', 'precision=0.0001', 'force', tmp)
  const g = read(tmp)
  const rings = []
  for (const f of g.features) {
    const polys = f.geometry.type === 'Polygon' ? [f.geometry.coordinates] : f.geometry.coordinates
    for (const poly of polys) rings.push(poly[0].map(([x, y]) => [round(x), round(y)]))
  }
  rings.sort((a, b) => b.length - a.length)
  writeFileSync(join(OUT, 'country.json'), JSON.stringify({ rings }))
  console.log('country', kb(join(OUT, 'country.json')), rings.length, 'rings', rings.reduce((s, r) => s + r.length, 0), 'pts')
}

// ---------- places ----------
{
  const g = read(join(SRC, 'places.geojson'))
  const keep = g.features
    .filter((f) => f.properties.kind !== 'suburb' && f.properties.population)
    .sort((a, b) => b.properties.population - a.properties.population)
    .slice(0, 90)
    .map((f) => ({
      name: f.properties.name_en,
      si: f.properties.name_si,
      ta: f.properties.name_ta,
      kind: f.properties.kind,
      pop: f.properties.population,
      lon: round(f.geometry.coordinates[0]),
      lat: round(f.geometry.coordinates[1]),
    }))
  writeFileSync(join(OUT, 'places.json'), JSON.stringify(keep))
  console.log('places', kb(join(OUT, 'places.json')))
}

// ---------- lines helper ----------
function lines(file, keep, pct, outName, extra = []) {
  const src = read(join(SRC, file))
  const pre = join(TMP, outName + '-in.geojson')
  writeFileSync(pre, JSON.stringify({ type: 'FeatureCollection', features: src.features.filter(keep).map((f) => ({ type: 'Feature', properties: { name: f.properties.name ?? '' }, geometry: f.geometry })) }))
  const tmp = join(TMP, outName + '.geojson')
  ms(pre, ...extra, '-simplify', pct, '-o', 'precision=0.001', 'force', tmp)
  const g = read(tmp)
  const out = []
  for (const f of g.features ?? g.geometries.map((geometry) => ({ geometry }))) {
    if (!f.geometry) continue
    const parts = f.geometry.type === 'LineString' ? [f.geometry.coordinates] : f.geometry.coordinates
    for (const p of parts) if (p.length >= 2) out.push(p.map(([x, y]) => [round(x, 3), round(y, 3)]))
  }
  writeFileSync(join(OUT, outName + '.json'), JSON.stringify(out))
  console.log(outName, kb(join(OUT, outName + '.json')), out.length, 'lines')
}
lines('railways.geojson', (f) => f.geometry.type === 'LineString', '14%', 'rail', ['-dissolve'])
{
  // keep only rivers whose named segments add up to a long course
  const src = read(join(SRC, 'waterways.geojson')).features.filter((f) => f.geometry.type === 'LineString' && f.properties.name)
  const total = new Map()
  for (const f of src) {
    let l = 0
    const c = f.geometry.coordinates
    for (let i = 1; i < c.length; i++) l += Math.hypot((c[i][0] - c[i - 1][0]) * 0.99, c[i][1] - c[i - 1][1])
    total.set(f.properties.name, (total.get(f.properties.name) ?? 0) + l)
  }
  lines('waterways.geojson', (f) => f.geometry.type === 'LineString' && (total.get(f.properties.name) ?? 0) > 0.22, '14%', 'rivers')
}

// ---------- protected areas ----------
{
  const out = join(OUT, 'protected.geojson')
  ms(join(SRC, 'protected-areas.geojson'), '-filter-fields', 'name,kind', '-simplify', '14%', 'keep-shapes', '-o', 'precision=0.001', 'force', out)
  console.log('protected', kb(out))
}

// ---------- signed distance field of the coast ----------
{
  const { rings } = read(join(OUT, 'country.json'))
  const B = { w: 79.3, e: 82.2, s: 5.55, n: 10.15 }
  const W = 348, H = 552 // ~0.0083 deg / px
  const K = Math.cos((7.85 * Math.PI) / 180)
  const RANGE = 0.7 // degrees, encoded span is [-RANGE, +RANGE]
  // segments in equal-area-ish space
  const segs = []
  for (const r of rings) for (let i = 0; i < r.length - 1; i++) segs.push([r[i][0] * K, r[i][1], r[i + 1][0] * K, r[i + 1][1]])
  const inside = (x, y) => {
    let c = false
    for (const r of rings) {
      for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
        const xi = r[i][0], yi = r[i][1], xj = r[j][0], yj = r[j][1]
        if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c
      }
    }
    return c
  }
  const raw = Buffer.alloc((W + 1) * H)
  for (let py = 0; py < H; py++) {
    const row = py * (W + 1)
    raw[row] = 0
    const lat = B.n - ((py + 0.5) / H) * (B.n - B.s)
    for (let px = 0; px < W; px++) {
      const lon = B.w + ((px + 0.5) / W) * (B.e - B.w)
      const x = lon * K
      let best = Infinity
      for (let s = 0; s < segs.length; s++) {
        const [ax, ay, bx, by] = segs[s]
        const dx = bx - ax, dy = by - ay
        const l2 = dx * dx + dy * dy
        let t = l2 ? ((x - ax) * dx + (lat - ay) * dy) / l2 : 0
        t = t < 0 ? 0 : t > 1 ? 1 : t
        const ex = ax + t * dx - x, ey = ay + t * dy - lat
        const d = ex * ex + ey * ey
        if (d < best) best = d
      }
      let d = Math.sqrt(best)
      if (!inside(lon, lat)) d = -d
      // sqrt-encoded so 8 bits keep sub-pixel precision near the shoreline
      const m = Math.sqrt(Math.min(1, Math.abs(d) / RANGE))
      raw[row + 1 + px] = Math.round((0.5 + 0.5 * Math.sign(d) * m) * 255)
    }
  }
  const crcT = new Int32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c })
  const crc = (b) => { let c = -1; for (const x of b) c = crcT[(c ^ x) & 255] ^ (c >>> 8); return (c ^ -1) >>> 0 }
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length)
    const td = Buffer.concat([Buffer.from(type), data])
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td))
    return Buffer.concat([len, td, c])
  }
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4); ihdr[8] = 8; ihdr[9] = 0
  const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))])
  writeFileSync(join(OUT, 'coast-sdf.png'), png)
  writeFileSync(join(OUT, 'coast-sdf.json'), JSON.stringify({ bounds: B, width: W, height: H, range: RANGE, k: round(K, 5) }))
  console.log('sdf', kb(join(OUT, 'coast-sdf.png')))
}

rmSync(TMP, { recursive: true, force: true })
