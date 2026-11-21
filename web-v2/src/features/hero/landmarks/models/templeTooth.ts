// Sri Dalada Maligawa (Temple of the Sacred Tooth Relic), Kandy.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - the Pattirippuwa: a white octagonal tower standing in the moat at the
//     front-left corner, an open pillared upper storey under a pointed
//     red-tile Kandyan roof (two pitches, a brass finial)
//   - the moat and the "cloud wall" (walakulu bemma): a low white wall crested
//     with wave-shaped merlons, pierced for oil lamps, running along the moat
//     and on along the shore of Kandy Lake
//   - Mahawahalkada: the entrance over the moat, flanked by stone elephants
//   - white two-storey ranges under dark Kandyan hipped roofs around a court;
//     inside it the timber shrine (Vedahitina Maligawa) under the golden canopy
//     added in 1987
//   - the royal palace and the open-pillared audience hall (Magul Maduwa) to the
//     north; Udawattakele forest rising behind; Kandy Lake in front, with its
//     little island
//
// Model frame: +z = the lake front.
import { Kit, apron, blob, fbm, kandyanRoof, palm, rng, siteWeight, smooth, tree, type Paint } from '../kit'
import type { Built } from '../types'

const wallPaint = (y0: number, h: number, glow = 0.14): Paint => ({ c: 'white', glow, k: (p) => 0.84 + 0.16 * smooth(y0, y0 + h, p.y) })

/** a white range with a lit window band and a dark Kandyan roof */
function range(k: Kit, x: number, z: number, w: number, d: number, h: number, ry = 0, roof: 'tile' | 'roof' = 'tile') {
  k.at({ p: [x, 0.004, z], r: [0, ry, 0] }, () => {
    k.box(w, h, d, wallPaint(0.004, h))
    k.box(w * 1.004, h * 0.16, d * 1.01, { c: 'timber', c2: 'light', t: 0.25, glow: 0.7 }, { p: [0, h * 0.56, 0] })
    k.box(w * 1.004, h * 0.14, d * 1.01, { c: 'timber' }, { p: [0, h * 0.16, 0] })
    kandyanRoof(k, w + 0.01, d + 0.01, h * 0.62, { c: roof, k: (p) => 0.86 + 0.14 * smooth(h, h * 1.6, p.y) }, { p: [0, h, 0] })
  })
}

/** a run of cloud wall from (x0, z0) to (x1, z1) */
function cloudWall(k: Kit, x0: number, z0: number, x1: number, z1: number, y: number, lamps = true) {
  const len = Math.hypot(x1 - x0, z1 - z0)
  const ang = -Math.atan2(z1 - z0, x1 - x0)
  k.at({ p: [(x0 + x1) / 2, y, (z0 + z1) / 2], r: [0, ang, 0] }, () => {
    k.box(len, 0.0052, 0.0034, { c: 'white', k: (p) => 0.86 + 0.14 * smooth(y, y + 0.005, p.y) })
    const n = Math.max(2, Math.round(len / 0.0076))
    for (let i = 0; i < n; i++) {
      const x = -len / 2 + ((i + 0.5) / n) * len
      // wave-shaped merlon: a soft triangle
      k.cyl(0.0006, 0.0036, 0.0036, 4, 'white', { p: [x, 0.0052, 0], r: [0, Math.PI / 4, 0], s: [1, 1, 0.55] })
      if (lamps && i % 2 === 0) k.box(0.0018, 0.0016, 0.0038, { c: 'light', glow: 1.7, phase: 0.05 + ((i * 0.137) % 0.9) }, { p: [x, 0.0022, 0] })
    }
  })
}

export function buildTempleTooth(): Built {
  const k = new Kit()
  const rand = rng(79)

  /* ---- site ---- */
  // the temple stands on a levelled terrace above the lake: rigid in the middle, eased onto the hillside at the edge
  const outline = blob(0.275, 0.25, 0.08, 4)
  const site = siteWeight(outline, 0.78, 0, 0.015)
  apron(k, outline, (p) => ({ c: 'ground', c2: 'lawn', t: smooth(0.24, 0.1, Math.hypot(p.x, p.z)) * 0.7 }), { cz: 0.015, seg: 36, rings: 9, level: 0.78 })

  /* ---- Udawattakele: the wooded hill behind ---- */
  const hill = (x: number, z: number) => 0.062 * Math.exp(-(((x + 0.02) / 0.15) ** 2 + ((z + 0.165) / 0.062) ** 2)) + 0.03 * Math.exp(-(((x - 0.14) / 0.07) ** 2 + ((z + 0.11) / 0.06) ** 2))
  const fade = (x: number, z: number) => smooth(0.26, 0.21, Math.hypot(x * 0.95, z - 0.015))
  k.drape = site
  k.surface(
    26, 8,
    (u, v, out) => {
      const x = -0.23 + u * 0.46, z = -0.075 - v * 0.14
      out.set(x, 0.002 + hill(x, z) * fade(x, z), z)
    },
    (p) => ({ c: 'foliageDeep', c2: 'foliage', t: fbm(p.x * 40, p.z * 40, 2), m: [['ground', smooth(0.006, 0.002, p.y)]], face: true }),
  )
  for (let i = 0, placed = 0; i < 400 && placed < 70; i++) {
    const x = -0.21 + rand() * 0.42, z = -0.2 + rand() * 0.12
    const y = hill(x, z) * fade(x, z)
    if (y < 0.006) continue
    const s = 0.008 + rand() * 0.007
    tree(k, x, z, s * 1.15, s * 2, placed % 3 ? (placed % 2 ? 'foliage' : 'foliageDeep') : 'foliageLite', i, y - s * 0.3)
    placed++
  }

  /* ---- Kandy Lake and its island ---- */
  const lake: [number, number][] = []
  for (let i = 0; i < 26; i++) {
    const a = (i / 26) * Math.PI * 2
    const r = 1 + 0.12 * Math.sin(a * 2 + 0.6) + 0.06 * Math.sin(a * 5)
    // kept inside the levelled part of the site, so the water is flat and nothing rises through it
    lake.push([0.03 + Math.cos(a) * 0.125 * r, 0.142 + Math.sin(a) * 0.047 * r])
  }
  k.prism(lake, 0.0016, 0.0034, { c: 'water', gloss: 0.9 })
  k.cyl(0.011, 0.0125, 0.004, 9, (_p, n) => (n.y > 0.5 ? { c: 'lawn' } : { c: 'white' }), { p: [0.06, 0.0026, 0.146] })
  k.box(0.007, 0.004, 0.007, wallPaint(0.006, 0.004), { p: [0.06, 0.0066, 0.146] })
  kandyanRoof(k, 0.0105, 0.0105, 0.004, 'roof', { p: [0.06, 0.0106, 0.146] }, 0)
  palm(k, 0.0645, 0.15, 0.022, 0.25, 0.6, 'palmTrunk', 'palmLeaf', 0.0066)
  palm(k, 0.0555, 0.143, 0.018, 0.2, 3.4, 'palmTrunk', 'palmLeaf', 0.0066)
  k.drape = 0

  /* ---- forecourt, moat, cloud wall ---- */
  k.plate(-0.15, -0.085, 0.13, 0.066, 0.004, (p) => ({ c: 'lawn', c2: 'path', t: smooth(0.04, 0.02, Math.abs(p.z - 0.02)) }))
  const MZ0 = 0.05, MZ1 = 0.066
  // the moat: a white-kerbed channel of water between the two runs of cloud wall
  k.plate(-0.15, MZ0, 0.115, MZ1, 0.0043, { c: 'whiteShade' })
  k.plate(-0.1485, MZ0 + 0.0015, 0.1135, MZ1 - 0.0015, 0.0048, { c: 'water', gloss: 0.9 })
  cloudWall(k, -0.15, MZ1 + 0.0026, 0.115, MZ1 + 0.0026, 0.004)
  cloudWall(k, -0.15, MZ0 - 0.0022, -0.107, MZ0 - 0.0022, 0.004, false)
  cloudWall(k, -0.058, MZ0 - 0.0022, -0.028, MZ0 - 0.0022, 0.004, false)
  cloudWall(k, 0.0, MZ0 - 0.0022, 0.115, MZ0 - 0.0022, 0.004, false)
  // and on along the lake shore
  cloudWall(k, -0.13, 0.1, -0.07, 0.088, 0.0036)
  cloudWall(k, -0.07, 0.088, 0.03, 0.084, 0.0036)
  cloudWall(k, 0.03, 0.084, 0.14, 0.092, 0.0036)

  /* ---- Pattirippuwa: the octagon standing in the moat ---- */
  k.at({ p: [-0.084, 0.0026, 0.054], s: 1.14 }, () => {
    const o = Math.PI / 8
    k.cyl(0.0225, 0.026, 0.024, 8, wallPaint(0.0026, 0.026, 0.1), { r: [0, o, 0] })
    k.cyl(0.0246, 0.0246, 0.0022, 8, 'whiteShade', { p: [0, 0.024, 0], r: [0, o, 0] })
    // arched openings of the lower storey
    for (let i = 0; i < 8; i++) k.at({ r: [0, (i * Math.PI) / 4, 0] }, () => k.box(0.0062, 0.0095, 0.001, { c: 'timber', c2: 'light', t: 0.3, glow: 0.8 }, { p: [0, 0.009, 0.0231] }))
    // open upper storey: pillars and a balustrade round a lit core
    k.cyl(0.0236, 0.0236, 0.0042, 8, 'white', { p: [0, 0.0262, 0], r: [0, o, 0] })
    k.cyl(0.016, 0.016, 0.019, 8, { c: 'cream', c2: 'light', t: 0.35, glow: 0.75 }, { p: [0, 0.0304, 0], r: [0, o, 0] })
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4
      k.box(0.0028, 0.019, 0.0028, 'white', { p: [Math.sin(a) * 0.0236, 0.0304, Math.cos(a) * 0.0236], r: [0, a, 0] })
    }
    // the roof: broad skirt, steep cap, brass finial
    const tile: Paint = { c: 'roof', c2: 'red', t: 0.25, k: (p) => 0.86 + 0.14 * smooth(0.055, 0.09, p.y) }
    k.cyl(0.0176, 0.0296, 0.0068, 8, tile, { p: [0, 0.0494, 0], r: [0, o, 0] })
    k.cyl(0.0012, 0.018, 0.0245, 8, tile, { p: [0, 0.0562, 0], r: [0, o, 0] })
    k.lathe([[0.0022, 0.0805], [0.003, 0.0827], [0.0014, 0.086], [0.0005, 0.0917]], 8, { c: 'gold', smooth: true, glow: 0.6, gloss: 0.6 })
  })

  /* ---- Mahawahalkada: bridge, stair, gatehouse, elephants ---- */
  k.at({ p: [-0.014, 0.004, 0.058] }, () => {
    k.box(0.02, 0.0034, 0.024, (_p, n) => (n.y > 0.5 ? { c: 'path' } : { c: 'whiteShade' }), { p: [0, -0.0012, 0] })
    k.at({ p: [0, 0, -0.022] }, () => {
      k.box(0.034, 0.023, 0.017, wallPaint(0.004, 0.023))
      k.box(0.0105, 0.0135, 0.0176, 'timber')
      kandyanRoof(k, 0.042, 0.025, 0.012, 'tile', { p: [0, 0.023, 0] })
    })
    for (const e of [-1, 1]) {
      // stone elephants at the foot of the stair
      k.box(0.0062, 0.0056, 0.0092, 'stoneDark', { p: [e * 0.0145, 0, 0.016] })
      k.box(0.0044, 0.0044, 0.0044, 'stoneDark', { p: [e * 0.0145, 0.0016, 0.0222] })
    }
  })

  /* ---- the temple: ranges round the court ---- */
  range(k, 0.008, 0.024, 0.132, 0.022, 0.027) // front range, facing the moat
  range(k, -0.052, -0.02, 0.022, 0.07, 0.025) // north wing
  range(k, 0.068, -0.02, 0.022, 0.07, 0.025) // south wing
  range(k, 0.008, -0.062, 0.132, 0.022, 0.034) // Alut Maligawa behind
  // the shrine itself: red-brown timber on a white base, under the golden canopy
  k.at({ p: [0.008, 0.004, -0.019] }, () => {
    k.box(0.05, 0.012, 0.036, wallPaint(0.004, 0.012))
    k.box(0.044, 0.016, 0.03, { c: 'brick', c2: 'timber', t: 0.4, glow: 0.2 }, { p: [0, 0.012, 0] })
    k.box(0.0445, 0.004, 0.0304, { c: 'gold', glow: 0.7, gloss: 0.5 }, { p: [0, 0.019, 0] })
    kandyanRoof(k, 0.056, 0.042, 0.011, 'tile', { p: [0, 0.028, 0] })
    // the canopy stands clear above the shrine roof on its own posts
    const gold: Paint = { c: 'gold', c2: 'goldDeep', t: (p) => smooth(0.065, 0.045, p.y) * 0.6, glow: 0.55, gloss: 0.75 }
    for (const sx of [-1, 1]) for (const sz of [-1, 1]) k.box(0.0018, 0.042, 0.0018, 'goldDeep', { p: [sx * 0.031, 0, sz * 0.024] })
    kandyanRoof(k, 0.074, 0.058, 0.02, gold, { p: [0, 0.042, 0] })
    k.lathe([[0.002, 0.062], [0.003, 0.0645], [0.0013, 0.068], [0.0004, 0.0745]], 8, { c: 'gold', smooth: true, glow: 0.9, gloss: 0.8 })
  })

  /* ---- palace, audience hall, queens' bath ---- */
  range(k, -0.118, -0.045, 0.024, 0.062, 0.016, 0, 'roof') // royal palace
  // Magul Maduwa: nothing but carved timber posts under a great roof
  k.at({ p: [-0.112, 0.004, 0.012] }, () => {
    k.box(0.05, 0.003, 0.026, 'stone')
    for (let i = 0; i < 6; i++) for (const sz of [-1, 1]) k.box(0.0018, 0.012, 0.0018, 'timber', { p: [-0.02 + i * 0.008, 0.003, sz * 0.0085] })
    kandyanRoof(k, 0.058, 0.034, 0.014, 'tile', { p: [0, 0.015, 0] })
  })
  range(k, 0.118, 0.07, 0.03, 0.02, 0.014, 0.12, 'roof') // Ulpange, on the lake edge

  /* ---- trees and lamps ---- */
  k.drape = site
  for (const [x, z] of [[-0.15, 0.02], [-0.16, -0.01], [0.105, 0.02], [0.115, -0.03], [0.1, -0.07], [-0.15, -0.075], [-0.155, 0.085], [0.15, 0.04], [0.03, -0.1], [-0.08, -0.095]] as const) {
    const s = 0.009 + rand() * 0.006
    tree(k, x, z, s * 1.15, s * 2, rand() > 0.5 ? 'foliage' : 'foliageDeep', 3, 0.004)
  }
  for (const [x, z] of [[0.004, 0.078], [0.095, 0.078], [-0.14, 0.074]] as const) palm(k, x, z, 0.036 + rand() * 0.012, 0.18, rand() * 6, 'palmTrunk', 'palmLeaf', 0.004)
  k.drape = 0

  return { body: k.build() }
}
