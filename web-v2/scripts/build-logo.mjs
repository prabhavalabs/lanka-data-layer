// Derives the Lanka Data Layer logo mark from the real coastline.
// Run: pnpm --filter @lanka-data-layer/web-v2 logo
//
// Input : public/geo/country.json (main-island ring, produced by scripts/build-geo.mjs)
// Output: src/components/common/logo-path.ts   path data for the <Logo> component
//         public/favicon.svg                   16 px-first favicon (tile + mark)
//         public/logo.svg                      standalone mark
//
// Pipeline: equirectangular projection with cos(lat) correction → morphological
// closing of lagoons (so Jaffna reads as one solid peninsula) → Douglas-Peucker →
// rounded-corner facets → fit to a 24 × 24 box, 0.01 px precision.
// Small optical tweaks are declared in TWEAKS so they stay visible and reviewable.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')
const { rings } = JSON.parse(readFileSync(resolve(root, 'public/geo/country.json'), 'utf8'))

const LAT0 = 7.85
const K = Math.cos((LAT0 * Math.PI) / 180)
const BRAND = '#d8446b'
const TILE = '#0b0e12'

const TWEAKS = {
  /** Douglas-Peucker tolerance in degrees. Higher = fewer, bolder facets. */
  tolerance: 0.02,
  /** Corner rounding: share of each facet given to the curve (0 = polygon, 0.5 = fully round B-spline). */
  round: 0.5,
  /** Anchors closer than this (px in the 24 box) are merged, which removes kinks. */
  minGap: 0.42,
  /** Opening radius (cells): shaves slivers thinner than the mark can hold at 16 px. */
  open: 1,
  /** Mark height inside the 24 px box (the island is ~0.57 as wide as it is tall). */
  height: 21.6,
  /** Raster cell (degrees) and closing radius (cells) used to seal the lagoons. */
  cell: 0.012,
  close: 3,
}

/* ---------- 1. project ---------- */
const project = ([lon, lat]) => [lon * K, -lat]
// main island, plus Mannar island so the north-west keeps its characteristic spur
const src = [rings[0], rings.find((r) => r.some(([x, y]) => x < 79.75 && y > 8.9 && y < 9.15))].filter(Boolean)

/* ---------- 2. rasterise + close ---------- */
let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity
for (const r of src) for (const p of r) { const [x, y] = project(p); x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y) }
const PAD = TWEAKS.close + 3
const C = TWEAKS.cell
const W = Math.ceil((x1 - x0) / C) + PAD * 2, H = Math.ceil((y1 - y0) / C) + PAD * 2
const inside = (x, y) => {
  let c = false
  for (const r of src)
    for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
      const [xi, yi] = project(r[i]), [xj, yj] = project(r[j])
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c
    }
  return c
}
let grid = new Uint8Array(W * H)
for (let py = 0; py < H; py++) for (let px = 0; px < W; px++) grid[py * W + px] = inside(x0 + (px - PAD + 0.5) * C, y0 + (py - PAD + 0.5) * C) ? 1 : 0
const morph = (g, r, dilate) => {
  const out = new Uint8Array(W * H)
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++) {
      let v = dilate ? 0 : 1
      for (let dy = -r; dy <= r && v !== (dilate ? 1 : 0); dy++)
        for (let dx = -r; dx <= r; dx++) {
          if (dx * dx + dy * dy > r * r) continue
          const yy = y + dy, xx = x + dx
          const s = yy < 0 || xx < 0 || yy >= H || xx >= W ? 0 : g[yy * W + xx]
          if (dilate ? s : !s) { v = dilate ? 1 : 0; break }
        }
      out[y * W + x] = v
    }
  return out
}
grid = morph(morph(grid, TWEAKS.close, true), TWEAKS.close, false)
grid = morph(morph(grid, TWEAKS.open, false), TWEAKS.open, true)

/* ---------- 3. trace the largest outline (marching squares, boundary walk) ---------- */
const at = (x, y) => (x < 0 || y < 0 || x >= W || y >= H ? 0 : grid[y * W + x])
function trace() {
  // start at the top-most, left-most filled cell and walk the boundary clockwise along cell corners
  let sx = -1, sy = -1
  outer: for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) if (at(x, y)) { sx = x; sy = y; break outer }
  const pts = []
  let x = sx, y = sy, dx = 1, dy = 0 // corner coordinates; heading east along the top edge
  do {
    pts.push([x, y])
    // cells around corner (x,y): a=NW b=NE c=SW d=SE relative to heading
    const left = dx === 1 ? at(x, y - 1) : dx === -1 ? at(x - 1, y) : dy === 1 ? at(x, y) : at(x - 1, y - 1)
    const right = dx === 1 ? at(x, y) : dx === -1 ? at(x - 1, y - 1) : dy === 1 ? at(x - 1, y) : at(x, y - 1)
    if (left) [dx, dy] = [dy, -dx] // turn left
    else if (!right) [dx, dy] = [-dy, dx] // turn right
    x += dx; y += dy
  } while ((x !== sx || y !== sy) && pts.length < 200000)
  return pts.map(([cx, cy]) => [x0 + (cx - PAD) * C, y0 + (cy - PAD) * C])
}
let outline = trace()

/* ---------- 4. simplify ---------- */
function dp(pts, tol) {
  const keep = new Uint8Array(pts.length)
  keep[0] = keep[pts.length - 1] = 1
  const stack = [[0, pts.length - 1]]
  while (stack.length) {
    const [a, b] = stack.pop()
    let best = 0, idx = -1
    const [ax, ay] = pts[a], [bx, by] = pts[b]
    const l = Math.hypot(bx - ax, by - ay) || 1e-9
    for (let i = a + 1; i < b; i++) {
      const d = Math.abs((bx - ax) * (ay - pts[i][1]) - (ax - pts[i][0]) * (by - ay)) / l
      if (d > best) { best = d; idx = i }
    }
    if (best > tol) { keep[idx] = 1; stack.push([a, idx], [idx, b]) }
  }
  return pts.filter((_, i) => keep[i])
}
// split the closed ring at its two extreme points so DP has stable anchors
const south = outline.reduce((m, p, i) => (p[1] > outline[m][1] ? i : m), 0)
outline = [...outline.slice(south), ...outline.slice(0, south)]
const north = outline.reduce((m, p, i) => (p[1] < outline[m][1] ? i : m), 0)
const simplified = [...dp(outline.slice(0, north + 1), TWEAKS.tolerance).slice(0, -1), ...dp([...outline.slice(north), outline[0]], TWEAKS.tolerance).slice(0, -1)]

/* ---------- 5. fit to the 24 px box ---------- */
let bx0 = Infinity, bx1 = -Infinity, by0 = Infinity, by1 = -Infinity
for (const [x, y] of simplified) { bx0 = Math.min(bx0, x); bx1 = Math.max(bx1, x); by0 = Math.min(by0, y); by1 = Math.max(by1, y) }
const S = TWEAKS.height / (by1 - by0)
const ox = (24 - (bx1 - bx0) * S) / 2, oy = (24 - TWEAKS.height) / 2
const fit = ([x, y]) => [(x - bx0) * S + ox, (y - by0) * S + oy]
let P = simplified.map(fit)
// merge near-coincident anchors (keeps the first of each cluster)
P = P.filter((p, i) => i === 0 || Math.hypot(p[0] - P[i - 1][0], p[1] - P[i - 1][1]) >= TWEAKS.minGap)
if (Math.hypot(P[0][0] - P[P.length - 1][0], P[0][1] - P[P.length - 1][1]) < TWEAKS.minGap) P.pop()
const f = (n) => String(Math.round(n * 100) / 100)

/* ---------- 6. round the corners: straight facets joined by quadratic curves (never overshoots) ---------- */
function roundedPath(pts, r) {
  const n = pts.length
  const mix = (a, b, t) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
  let d = ''
  for (let i = 0; i < n; i++) {
    const prev = pts[(i - 1 + n) % n], cur = pts[i], next = pts[(i + 1) % n]
    const a = mix(cur, prev, r), b = mix(cur, next, r)
    d += `${i ? 'L' : 'M'}${f(a[0])} ${f(a[1])}Q${f(cur[0])} ${f(cur[1])} ${f(b[0])} ${f(b[1])}`
  }
  return d + 'Z'
}
const d = roundedPath(P, TWEAKS.round)

// tight horizontal bounds, so the lockup can set the mark without dead space either side
const minX = Math.min(...P.map((p) => p[0])), maxX = Math.max(...P.map((p) => p[0]))
const tight = [Math.floor((minX - 0.4) * 10) / 10, Math.ceil((maxX - minX + 0.8) * 10) / 10]

/* ---------- 7. emit ---------- */
writeFileSync(
  resolve(root, 'src/components/common/logo-path.ts'),
  `// GENERATED by scripts/build-logo.mjs from the real coastline — do not edit by hand.
/** Square box, mark centred: favicons, app icons. */
export const LOGO_VIEWBOX = '0 0 24 24'
/** Tight box for lockups next to the wordmark. Aspect = LOGO_ASPECT (width / height). */
export const LOGO_VIEWBOX_TIGHT = '${f(tight[0])} 0 ${f(tight[1])} 24'
export const LOGO_ASPECT = ${f(tight[1] / 24)}
/** Sri Lanka silhouette: ${P.length} anchors, lagoons closed, fitted to a 24 px box. */
export const LOGO_PATH =
  '${d}'
`,
)
// favicon: tile + mark. The mark is drawn slightly larger than in the 24 px lockup so it holds at 16 px.
const fav = (tile) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">${tile ? `<rect width="32" height="32" rx="7.5" fill="${TILE}"/>` : ''}<path transform="translate(${tile ? '2.2 2.2' : '-1.6 -1.6'}) scale(${tile ? 1.15 : 1.4667})" fill="${BRAND}" d="${d}"/></svg>\n`
writeFileSync(resolve(root, 'public/favicon.svg'), fav(true))
writeFileSync(resolve(root, 'public/logo.svg'), `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="${BRAND}" d="${d}"/></svg>\n`)

if (process.argv.includes('--preview')) {
  const out = resolve(root, '.screens/work')
  mkdirSync(out, { recursive: true })
  const mark = (size, extra = '') => `<svg viewBox="0 0 24 24" width="${size}" height="${size}"><path fill="${BRAND}" d="${d}"/>${extra}</svg>`
  const dot = ''
  const overlay = `<svg viewBox="0 0 24 24" width="330" height="330"><path fill="none" stroke="#999" stroke-width=".05" d="${src.map((r) => 'M' + r.map((p) => fit(project(p)).map(f).join(' ')).join('L') + 'Z').join('')}"/><path fill="${BRAND}" fill-opacity=".45" d="${d}"/>${P.map(([x, y]) => `<circle cx="${f(x)}" cy="${f(y)}" r=".12"/>`).join('')}</svg>`
  writeFileSync(
    resolve(out, 'logo-preview.html'),
    `<!doctype html><body style="margin:0;background:#0b0e12;color:#9aa7b4;font:12px ui-monospace,monospace;padding:24px">
<div style="display:flex;gap:26px;align-items:flex-end">${[16, 22, 24, 32, 64, 160, 330].map((s) => `<div>${mark(s)}<div>${s}</div></div>`).join('')}<div style="background:#fbfbfa">${overlay}</div></div>
<div style="display:flex;gap:26px;align-items:flex-end;margin-top:24px">${[16, 24, 32, 64, 160].map((s) => `<div>${mark(s, dot)}<div>${s}</div></div>`).join('')}
${[16, 32, 64, 128].map((s) => `<div><img src="../../public/favicon.svg" width="${s}" height="${s}"><div>fav ${s}</div></div>`).join('')}
<div style="background:#fbfbfa;padding:16px;display:flex;gap:26px;align-items:flex-end">${[16, 24, 64, 160].map((s) => mark(s)).join('')}</div></div>
<div style="zoom:8;image-rendering:pixelated;margin-top:6px;display:flex;gap:6px">${mark(16)}${mark(24)}<img src="../../public/favicon.svg" width="16" height="16"></div>`,
  )
}
console.log(`logo: ${P.length} anchors, ${d.length} chars, tight viewBox ${tight[0]} 0 ${tight[1]} 24`)
