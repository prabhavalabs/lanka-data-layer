// Sigiriya (Lion Rock), Matale District.
//
// Reference (Wikipedia + Commons aerials, checked 2026-11):
//   - gneiss monolith ~180 m above the plain, summit 349 m a.s.l.
//   - plan is an elongated oval on a N–S axis, ~350 × 190 m; the top-down aerial
//     shows it broader in the southern half and pointed at the north end
//   - walls are near-vertical with a mushroom overhang near the rim; the west
//     face overhangs most (frescoes + Mirror Wall gallery sit under it, halfway up)
//   - colour: ochre / rust gneiss with dark vertical water streaks from the rim
//   - summit (~1.6 ha) is covered edge to edge by stepped red-brick palace
//     terraces, highest at the north-west, with a rock-cut pool south of centre
//   - Lion Terrace: a plateau halfway up the NORTH end; two colossal paws flank a
//     brick stair that climbs into the rock, iron stairs zig-zag above
//   - Water Gardens: a walled, moated precinct on the WEST, strictly symmetric on
//     an E–W axis: four L-pools round a square island, two long fountain pools,
//     moated summer-palace islands either side, an octagonal pool by the rock
//
// Model frame: +z = west (water gardens, hero side), −x = north (Lion Terrace).
// Proportion 1 : 0.54 : 0.60 (length : width : height) against the true
// 1 : 0.54 : 0.51, i.e. ≈1.18× vertical.
import * as THREE from 'three'
import { Kit, apron, blob, clamp, curve, fbm, hash2, rng, smooth, tree, vnoise, type Paint } from '../kit'
import type { Built } from '../types'

const A_N = 0.16 // half-length toward the north tip
const A_S = 0.142 // half-length toward the south
const B = 0.082 // half-width
const H = 0.182

/** plan outline at angle th (0 = south end, π/2 = west face) for unit height v */
function plan(th: number, v: number, out: THREE.Vector3) {
  const c = Math.cos(th), s = Math.sin(th)
  const e = 2 / 2.7 // superellipse: boxier than an ellipse
  let x = Math.sign(c) * Math.pow(Math.abs(c), e) * (c > 0 ? A_S : A_N)
  let z = Math.sign(s) * Math.pow(Math.abs(s), e) * B
  z *= 1 + 0.16 * (x / A_S) // broader toward the south
  // talus flare → waist → overhang → rounded rim
  const prof = curve([[0, 1.06], [0.1, 0.985], [0.34, 0.93], [0.6, 0.95], [0.82, 1.035], [0.93, 1.025], [1, 0.94]], v)
  const lump = (fbm(th * 2.2 + 3.1, v * 2.0, 3) - 0.5) * 0.13
  const flute = (vnoise(th * 19, v * 0.6) - 0.5) * 0.1 * smooth(0.08, 0.4, v)
  const k = prof * (1 + lump + flute)
  x *= k
  z *= k
  // the west face leans out over the gallery
  z += 0.013 * smooth(0.48, 0.86, v) * Math.max(0, s)
  const y = H * v + 0.012 * clamp(-x / A_N) * smooth(0.75, 1, v)
  out.set(x, y, z)
}

export function buildSigiriya(): Built {
  const k = new Kit()
  const tmp = new THREE.Vector3()
  const rand = rng(7)

  /* ---- site pad ---- */
  apron(k, blob(0.275, 0.215, 0.1, 2), (p) => ({ c: 'ground', c2: 'foliage', t: 0.8 * smooth(0.27, 0.1, Math.hypot((p.x + 0.01) * 0.75, p.z - 0.035)) }), { cx: -0.01, cz: 0.035, seg: 36, rings: 6 })

  /* ---- vegetated talus the rock stands in ---- */
  k.surface(
    40, 3,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      plan(th, 0.06, out)
      const grow = 1 + (1 - v) * (0.55 + 0.3 * vnoise(u * 9, 2.3))
      // the foot is buried, so the rock never shows daylight under it on uneven ground
      out.set(out.x * lerpN(grow, 1.02, v), v === 0 ? -0.03 : 0.001 + v * H * (0.17 + 0.08 * vnoise(u * 7, 5.1)), out.z * lerpN(grow, 1.02, v))
    },
    (p) => ({ c: 'foliageDeep', c2: 'foliage', t: fbm(p.x * 40, p.z * 40, 2), face: true }),
    { closeU: true },
  )

  /* ---- the rock ---- */
  const rockPaint = (p: THREE.Vector3): Paint => {
    const th = Math.atan2(p.z / B, p.x / (p.x > 0 ? A_S : A_N))
    const v = clamp(p.y / H)
    const tone = fbm(th * 2.6 + 1.7, v * 2.6, 3)
    // water stains: narrow, start at the rim, fade toward the foot
    const streak = smooth(0.52, 0.66, vnoise(th * 38 + 9, v * 0.9)) * smooth(0.1, 0.7, v)
    const stain = smooth(0.5, 0.75, vnoise(th * 11 + 2, v * 0.5)) * smooth(0.45, 1, v)
    return {
      c: 'sigOchre',
      c2: 'sigPale',
      t: smooth(0.3, 0.75, tone) * 0.75,
      m: [
        ['sigRust', smooth(0.5, 0.2, tone) * 0.8 + 0.25 * smooth(0.5, 0.2, v)],
        ['sigGrey', stain * 0.6],
        ['sigStreak', streak * 0.85],
      ],
      // soft contact shadow where the wall meets the trees
      k: 0.78 + 0.22 * smooth(0.04, 0.3, v),
    }
  }
  const NU = 72, NV = 15
  k.surface(NU, NV, (u, v, out) => plan(-u * Math.PI * 2, v, out), rockPaint, { closeU: true })

  /* ---- summit: brick terraces stepping up to the palace at the north-west ---- */
  const rim = (scale: number, ox: number, oz: number, n = 30) => {
    const out: [number, number][] = []
    for (let i = 0; i < n; i++) {
      plan((i / n) * Math.PI * 2, 1, tmp)
      out.push([tmp.x * scale + ox, tmp.z * scale + oz])
    }
    return out
  }
  const terrace = (p: THREE.Vector3, n: THREE.Vector3): Paint =>
    n.y > 0.5 ? { c: 'brick', c2: 'lawn', t: smooth(0.4, 0.6, fbm(p.x * 60, p.z * 60, 2)) * 0.85, m: [['sigPale', 0.3]] } : { c: 'brickDark', c2: 'brick', t: 0.4 }
  const top = H + 0.004
  k.prism(rim(1.02, 0, 0), H - 0.012, top, terrace)
  // straight-edged brick platforms, the way the palace footings actually read from the air
  const step = 0.0065
  k.prism([[-0.128, -0.02], [-0.1, -0.05], [-0.01, -0.056], [0.022, -0.03], [0.022, 0.05], [-0.06, 0.062], [-0.118, 0.04]], top, top + step, terrace)
  k.prism([[-0.12, -0.014], [-0.092, -0.04], [-0.034, -0.042], [-0.034, 0.046], [-0.108, 0.036]], top + step, top + step * 2, terrace)
  k.prism([[-0.106, -0.016], [-0.056, -0.022], [-0.056, 0.03], [-0.1, 0.026]], top + step * 2, top + step * 3, terrace)
  k.box(0.03, 0.007, 0.026, { c: 'brick', c2: 'sigPale', t: 0.45 }, { p: [-0.08, top + step * 3, 0.004] })
  // lower southern terraces
  k.prism([[0.03, -0.05], [0.1, -0.046], [0.118, -0.01], [0.112, 0.045], [0.03, 0.058]], top, top + step * 0.55, terrace)
  for (const [x, z, w, d, h, lvl] of [[-0.012, 0.022, 0.018, 0.022, 0.005, 1], [0.0, -0.03, 0.022, 0.016, 0.005, 1], [0.082, -0.022, 0.022, 0.024, 0.005, 0.55], [0.098, 0.022, 0.018, 0.016, 0.004, 0.55], [0.05, 0.04, 0.02, 0.014, 0.004, 0.55]] as const)
    k.box(w, h, d, { c: 'brick', c2: 'sigPale', t: 0.35 }, { p: [x, top + step * lvl, z] })
  // the rock-cut pool
  k.plate(0.034, -0.02, 0.07, 0.012, top + step * 0.55 + 0.0006, { c: 'brickDark' })
  k.plate(0.037, -0.017, 0.067, 0.009, top + step * 0.55 + 0.0011, { c: 'waterDeep', gloss: 0.9 })
  // the few trees that survive on the plateau
  tree(k, 0.112, -0.03, 0.009, 0.016, 'foliage', 1, top)
  tree(k, 0.012, -0.058, 0.008, 0.014, 'foliageLite', 2, top)
  tree(k, 0.074, 0.052, 0.007, 0.012, 'foliage', 3, top)

  /* ---- Mirror Wall gallery: the pale band halfway up the west face ---- */
  k.surface(
    16, 1,
    (u, v, out) => {
      const th = (0.8 - u * 0.56) * Math.PI
      const vv = 0.45 + v * 0.05
      plan(th, vv, out)
      const n = Math.hypot(out.x, out.z)
      out.x += (out.x / n) * 0.0045
      out.z += (out.z / n) * 0.0045
    },
    { c: 'sigMirror' },
  )
  // its lip (reads as the shadowed gallery roof line)
  k.surface(
    16, 1,
    (u, v, out) => {
      const th = (0.8 - u * 0.56) * Math.PI
      plan(th, 0.5, out)
      const n = Math.hypot(out.x, out.z)
      const off = 0.0045 * (1 - v)
      out.x += (out.x / n) * off
      out.z += (out.z / n) * off
    },
    { c: 'sigStreak' },
  )

  /* ---- Lion Terrace on the north end ---- */
  const TY = H * 0.43 // terrace level
  const TX = -A_N + 0.004 // terrace centre
  k.surface(
    22, 4,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      const r = curve([[0, 1.75], [0.45, 1.35], [0.85, 1.06], [1, 1.0]], v) * (1 + (vnoise(u * 6 + 1, v * 2) - 0.5) * 0.18)
      out.set(TX + Math.cos(th) * 0.05 * r, v === 0 ? -0.03 : TY * v, Math.sin(th) * 0.062 * r)
    },
    (p) => ({ c: 'foliageDeep', c2: 'sigGrey', t: smooth(0.45, 0.95, p.y / TY), m: [['foliage', 0.4 * fbm(p.x * 60, p.z * 60, 2) * (1 - p.y / TY)]], face: true }),
    { closeU: true },
  )
  const court: [number, number, number][] = []
  for (let i = 0; i < 22; i++) {
    const th = (i / 22) * Math.PI * 2
    court.push([TX + Math.cos(th) * 0.05, TY, Math.sin(th) * 0.062])
  }
  k.fan([TX, TY + 0.0005, 0], court, (p) => ({ c: 'path', c2: 'lawn', t: smooth(0.028, 0.055, Math.hypot(p.x - TX, p.z)) }))

  // brick stair between the paws, then iron flights up the face
  const faceX = -A_N * 0.88
  k.at({ p: [faceX - 0.004, TY, 0] }, () => {
    const wedge = new THREE.Shape([new THREE.Vector2(-0.036, 0), new THREE.Vector2(0.012, 0), new THREE.Vector2(0.012, 0.056), new THREE.Vector2(0.004, 0.056)])
    k.slab(wedge, 0.02, (_p, n) => (n.y > 0.3 ? { c: 'path' } : { c: 'brick' }))
    // side walls of the stairway
    for (const z of [-0.012, 0.012]) k.slab(wedge, 0.004, { c: 'brickDark' }, { p: [0, 0.004, z] })
  })
  const flight = (x0: number, y0: number, z0: number, x1: number, y1: number, z1: number) => {
    const d = new THREE.Vector3(x1 - x0, y1 - y0, z1 - z0)
    const len = d.length()
    const q = new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 1, 0), d.normalize())
    const e = new THREE.Euler().setFromQuaternion(q)
    k.box(0.0055, len, 0.0035, { c: 'iron', c2: 'sigPale', t: 0.35 }, { p: [x0, y0, z0], r: [e.x, e.y, e.z] })
  }
  flight(faceX + 0.004, TY + 0.056, 0.002, faceX + 0.006, H * 0.82, 0.036)
  flight(faceX + 0.006, H * 0.82, 0.036, faceX + 0.026, H * 1.0, 0.012)

  // the paws: knuckled forefeet of plastered brick, three claws each
  for (const side of [-1, 1]) {
    k.at({ p: [faceX - 0.02, TY, side * 0.027], s: 1.7 }, () => {
      const paw: Paint = { c: 'stoneWarm', k: (p) => 0.8 + 0.2 * smooth(TY, TY + 0.02, p.y) }
      k.ball(1, 1, paw, { p: [0.006, 0.008, 0], s: [0.017, 0.012, 0.0115] })
      for (const tz of [-1, 0, 1]) {
        k.ball(1, 1, paw, { p: [-0.012, 0.0052, tz * 0.0068], s: [0.0085, 0.0058, 0.0036] })
        k.cone(0.0019, 0.006, 5, { c: 'sigStreak' }, { p: [-0.019, 0.0035, tz * 0.0068], r: [0, 0, 1.75] })
      }
    })
  }

  /* ---- boulder garden at the west foot ---- */
  for (let i = 0; i < 9; i++) {
    const x = -0.11 + rand() * 0.24, z = B + 0.02 + rand() * 0.022
    const s = 0.009 + rand() * 0.009
    k.draped(1, () => k.rigidAt(x, z, () => k.ball(1, 0, { c: 'sigGrey', c2: 'sigStreak', t: rand() * 0.4, k: 0.9 + rand() * 0.15 }, { p: [x, s * 0.45 + 0.006, z], r: [rand() * 3, rand() * 3, 0], s: [s * 1.25, s * 0.8, s] })))
  }

  /* ---- Water Gardens: the western precinct ---- */
  k.drape = 1
  // a long rectangle on the rock's east–west axis: ≈3 : 2 here, the real precinct is longer still
  const Z0 = B + 0.05, Z1 = Z0 + 0.216, GW = 0.08
  // the garden is a stack of thin plates. They are draped, and cut finely enough that every layer follows the
  // same ground: the steps between layers are several times the error that leaves.
  const y0 = 0.0026
  const water: Paint = { c: 'water', gloss: 0.9 }
  const pool = (x0: number, z0: number, x1: number, z1: number) => {
    k.plate(x0 - 0.0028, z0 - 0.0028, x1 + 0.0028, z1 + 0.0028, y0 + 0.0026, { c: 'brick' })
    k.plate(x0, z0, x1, z1, y0 + 0.0036, water)
  }
  // moat (three sides) and the rampart outside it
  k.plate(-GW - 0.026, Z0 - 0.01, GW + 0.026, Z1 + 0.026, y0 - 0.001, { c: 'foliageLite', c2: 'ground', t: 0.4 })
  k.plate(-GW - 0.016, Z0, GW + 0.016, Z1 + 0.016, y0, water)
  k.plate(-GW, Z0, GW, Z1, y0 + 0.0014, (p) => ({ c: 'lawn', c2: 'foliageLite', t: 0.5 * fbm(p.x * 70, p.z * 70, 2) }))
  // axial path from the west gate to the foot of the rock
  k.plate(-0.0065, Z0 - 0.03, 0.0065, Z1 + 0.03, y0 + 0.0046, { c: 'path' })
  k.plate(-GW, Z0 + 0.142, GW, Z0 + 0.15, y0 + 0.0026, { c: 'path' })
  // Garden 1: four L-pools round the square island (read as a char-bagh)
  const g1 = Z1 - 0.036
  for (const sx of [-1, 1])
    for (const sz of [-1, 1]) pool(sx * 0.011 + (sx < 0 ? -0.027 : 0), g1 + sz * 0.011 + (sz < 0 ? -0.027 : 0), sx * 0.011 + (sx < 0 ? 0 : 0.027), g1 + sz * 0.011 + (sz < 0 ? 0 : 0.027))
  // Garden 2: the long fountain pools either side of the path
  for (const sx of [-1, 1]) pool(sx * 0.0165 - 0.005, Z0 + 0.046, sx * 0.0165 + 0.005, Z0 + 0.13)
  // summer-palace islands, each in its own moat
  for (const sx of [-1, 1]) {
    pool(sx * 0.052 - 0.019, Z0 + 0.054, sx * 0.052 + 0.019, Z0 + 0.124)
    k.rigidAt(sx * 0.052, Z0 + 0.089, () => {
      k.box(0.024, 0.003, 0.05, (_p, n) => (n.y > 0.5 ? { c: 'lawn' } : { c: 'brick' }), { p: [sx * 0.052, y0 + 0.0036, Z0 + 0.089] })
      k.box(0.012, 0.004, 0.018, { c: 'brick', c2: 'sigPale', t: 0.3 }, { p: [sx * 0.052, y0 + 0.0066, Z0 + 0.089] })
    })
  }
  // Garden 3: the octagonal pool under the rock
  k.rigidAt(0.04, Z0 + 0.02, () => {
    k.cyl(0.0185, 0.0185, 0.0014, 8, { c: 'brick' }, { p: [0.04, y0 + 0.0022, Z0 + 0.02], r: [0, Math.PI / 8, 0] })
    k.cyl(0.0155, 0.0155, 0.0014, 8, water, { p: [0.04, y0 + 0.003, Z0 + 0.02], r: [0, Math.PI / 8, 0] })
  })
  k.plate(-0.056, Z0 + 0.008, -0.022, Z0 + 0.032, y0 + 0.0026, { c: 'brick', c2: 'lawn', t: 0.5 })
  // west gate
  for (const sx of [-1, 1]) k.rigidAt(sx * 0.0105, Z1 + 0.02, () => k.box(0.006, 0.009, 0.008, { c: 'brick' }, { p: [sx * 0.0105, y0, Z1 + 0.02] }))

  /* ---- trees on the Lion Terrace spur ---- */
  k.drape = 0
  for (let i = 0; i < 16; i++) {
    const a = Math.PI * (0.55 + (i / 16) * 0.9) + (rand() - 0.5) * 0.2
    const lv = 0.15 + rand() * 0.5
    const rr = curve([[0, 1.75], [0.45, 1.35], [0.85, 1.06], [1, 1.0]], lv) * 1.04
    const s = 0.008 + rand() * 0.006
    tree(k, TX + Math.cos(a) * 0.05 * rr, Math.sin(a) * 0.062 * rr, s, s * 1.7, i % 3 ? 'foliage' : 'foliageDeep', i, TY * lv - s * 0.5)
  }

  /* ---- jungle ---- */
  k.drape = 1
  const inGarden = (x: number, z: number) => Math.abs(x) < GW + 0.03 && z > Z0 - 0.012 && z < Z1 + 0.04
  let placed = 0
  for (let i = 0; i < 900 && placed < 120; i++) {
    const a = rand() * Math.PI * 2
    const rr = 0.4 + rand() * 0.62
    const x = Math.cos(a) * 0.265 * rr - 0.01, z = Math.sin(a) * 0.195 * rr + 0.035
    plan(Math.atan2(z / B, x / (x > 0 ? A_S : A_N)), 0.06, tmp)
    const inside = Math.hypot(x, z) < Math.hypot(tmp.x, tmp.z) * 1.22
    if (inside || inGarden(x, z) || Math.hypot((x - TX) / 0.06, z / 0.074) < 1) continue
    const s = 0.0085 + rand() * 0.0075
    const pick = hash2(x * 311, z * 173)
    tree(k, x, z, s, s * (1.5 + rand() * 0.6), pick > 0.66 ? 'foliageDeep' : pick > 0.3 ? 'foliage' : 'foliageLite', i)
    placed++
  }
  // an avenue of trees along the garden walls
  for (let i = 0; i < 10; i++)
    for (const sx of [-1, 1]) tree(k, sx * (GW - 0.006), Z0 + 0.012 + i * 0.0215, 0.006, 0.012, i % 2 ? 'foliage' : 'foliageDeep', i, y0)

  k.drape = 0

  return { body: k.build() }
}

function lerpN(a: number, b: number, t: number) {
  return a + (b - a) * t
}
