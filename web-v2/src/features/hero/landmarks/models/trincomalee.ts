// Trincomalee: Swami Rock (Konamalai), Fort Frederick, Koneswaram, the harbour.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - Swami Rock is a narrow promontory ≈1.1 km long standing out into the sea
//     between Back Bay and Dutch Bay; it climbs from the isthmus to a cliff that
//     drops ≈120 m sheer into the water (Lovers' Leap, Ravana's Cleft)
//   - layered tan-brown rock on the seaward cliff, scrub and trees on the back
//   - Fort Frederick's stone curtain wall and gate close the neck of the promontory
//   - Koneswaram temple crowns the tip: a multicoloured pastel gopuram, low
//     mandapam halls, and the large seated Shiva (peach skin, gilded ornaments)
//   - the natural deep-water harbour lies in the lee of the rock: navy base,
//     oil-tank farm at China Bay, a white lighthouse on Round Island
//
// Model frame: +z = out to sea, −x = the harbour side. `shore` (z = 0) is the
// waterline; the scene turns the model to face the real sea and slides it onto
// the real coast. Everything at sea is built at `ctx.sea`.
import * as THREE from 'three'
import { Kit, apron, blob, curve, fbm, hipRoof, palm, rng, smooth, tree, type Paint } from '../kit'
import type { BuildCtx, Built, Mover } from '../types'

const Z0 = -0.05, Z1 = 0.24
const halfW = (z: number) => curve([[Z0, 0.085], [-0.01, 0.062], [0.045, 0.036], [0.1, 0.044], [0.16, 0.058], [0.205, 0.05], [0.228, 0.03], [Z1, 0.008]], z)
const crest = (z: number) => curve([[Z0, 0], [-0.01, 0.004], [0.045, 0.026], [0.1, 0.058], [0.16, 0.098], [0.21, 0.112], [Z1, 0.104]], z)
/** spine wanders a little so the promontory is not a ruler */
const spine = (z: number) => 0.012 * Math.sin((z - Z0) * 11)

/** top of the rock at a point across it: ξ = −1 harbour edge … +1 cliff edge */
function topAt(z: number, xi: number) {
  const c = crest(z)
  // the back of the rock ramps up from the harbour; the head is a near-level plateau
  const ramp = smooth(-1.05, THREE.MathUtils.lerp(0.35, -0.5, smooth(0.1, 0.16, z)), xi)
  return c * (0.5 + 0.5 * ramp) + (fbm(z * 40 + 1, xi * 3, 2) - 0.5) * 0.007 * smooth(0.0, 0.06, c) * (1 - 0.7 * smooth(0.13, 0.17, z))
}

function shipGeometry() {
  const k = new Kit()
  const hull: [number, number][] = [[-0.044, -0.0058], [0.018, -0.0062], [0.036, -0.0034], [0.05, 0], [0.036, 0.0034], [0.018, 0.0062], [-0.044, 0.0058], [-0.047, 0]]
  // waterline boot, hull, deck
  // nothing below the waterline: the sea is drawn without depth and would not hide it
  k.prism(hull.map(([x, z]) => [x * 0.97, z * 0.92]), 0, 0.0012, 'navyDark')
  k.prism(hull, 0.0012, 0.0066, (_p, n) => (n.y > 0.5 ? { c: 'navy', c2: 'white', t: 0.25 } : { c: 'navy' }))
  // superstructure: bridge block, wheelhouse with lit windows, funnel, mast
  k.box(0.03, 0.0052, 0.0092, 'navy', { p: [-0.006, 0.0066, 0] })
  k.box(0.014, 0.0044, 0.008, { c: 'navy', c2: 'white', t: 0.3 }, { p: [0.002, 0.0118, 0] })
  k.box(0.0144, 0.0014, 0.0083, { c: 'light', glow: 1.3 }, { p: [0.002, 0.0138, 0] })
  k.box(0.008, 0.0048, 0.0052, 'navyDark', { p: [-0.013, 0.0118, 0], r: [0, 0, 0.12] })
  k.cyl(0.0005, 0.0008, 0.016, 5, 'navyDark', { p: [0.0, 0.0162, 0] })
  k.box(0.0008, 0.0008, 0.009, 'navyDark', { p: [0.0, 0.026, 0] })
  k.add(new THREE.OctahedronGeometry(0.0013, 0), { c: 'red', glow: -2.2, phase: 0.62 }, { p: [0, 0.0328, 0] })
  // forward gun, helicopter deck aft
  k.cyl(0.0034, 0.0038, 0.0034, 8, 'navy', { p: [0.026, 0.0066, 0] })
  k.cyl(0.0006, 0.0006, 0.011, 5, 'navyDark', { p: [0.028, 0.0086, 0], r: [0, 0, -1.4] })
  k.plate(-0.042, -0.0046, -0.024, 0.0046, 0.0068, { c: 'navyDark', c2: 'white', t: 0.15 })
  return k.build()
}

export function buildTrincomalee(ctx: BuildCtx): Built {
  const k = new Kit()
  const rand = rng(31)
  const SEA = ctx.sea

  /* ---- the town's ground behind the shore ---- */
  apron(k, blob(0.2, 0.105, 0.08, 5), (p) => ({ c: 'ground', c2: 'sand', t: smooth(-0.03, -0.004, p.z) * 0.8 }), { cz: -0.108, seg: 30, rings: 5 })

  /* ---- Swami Rock ---- */
  const NU = 34, NV = 16
  const rockPaint = (p: THREE.Vector3, n: THREE.Vector3): Paint => {
    const strata = fbm(p.y * 95 + 3, p.z * 9 + p.x * 9, 2)
    const steep = 1 - smooth(0.5, 0.82, n.y)
    const wet = smooth(SEA + 0.012, SEA, p.y)
    const green = (1 - steep) * smooth(0.002, 0.012, p.y - SEA)
    return {
      c: 'cliff',
      c2: 'cliffLite',
      t: smooth(0.4, 0.7, strata),
      m: [
        ['cliffDark', smooth(0.5, 0.2, strata) * 0.8 + wet * 0.7],
        ['foliage', green * (0.6 + 0.4 * fbm(p.x * 50, p.z * 50, 2))],
        ['lawn', green * smooth(0.55, 0.8, fbm(p.x * 24 + 9, p.z * 24, 2)) * 0.8],
      ],
      face: true,
    }
  }
  k.surface(
    NU, NV,
    (u, v, out) => {
      const z = Z0 + (Z1 - Z0) * u
      const hw = halfW(z), cx = spine(z)
      const c = crest(z)
      const rough = (fbm(z * 55, v * 9, 2) - 0.5) * 0.012
      if (v < 0.25) {
        // harbour side: a wooded slope down to the water
        const t = v / 0.25
        const run = 0.012 + 0.34 * c
        out.set(cx - hw - run * (1 - t) + rough * (1 - t), THREE.MathUtils.lerp(SEA, topAt(z, -1), Math.pow(t, 0.8)), z)
      } else if (v <= 0.75) {
        const xi = (v - 0.25) / 0.5 * 2 - 1
        out.set(cx + hw * xi, topAt(z, xi), z)
      } else {
        // seaward side: the cliff, undercut a little at the waterline
        const t = (v - 0.75) / 0.25
        const lean = 0.006 * Math.sin(t * Math.PI) + 0.004 * t
        out.set(cx + hw + lean + rough * t, THREE.MathUtils.lerp(topAt(z, 1), SEA, Math.pow(t, 1.25)), z)
      }
    },
    rockPaint,
  )
  // the nose of the promontory, and the cap that closes its top
  const nose = (u: number, v: number, out: THREE.Vector3) => {
    const a = -Math.PI * (1 - u) // harbour side → round the point → cliff side
    const hw = halfW(Z1) + 0.012
    const r = 1 + (1 - v) * 0.5 + (fbm(u * 7, v * 5, 2) - 0.5) * 0.25 * (1 - v)
    out.set(spine(Z1) + Math.cos(a) * hw * r, THREE.MathUtils.lerp(SEA, topAt(Z1, Math.cos(a)), Math.pow(v, 0.8)), Z1 - 0.005 + Math.sin(-a) * 0.022 * r)
  }
  k.surface(12, 6, nose, rockPaint)
  const noseRim: [number, number, number][] = []
  const np = new THREE.Vector3()
  for (let i = 0; i <= 12; i++) {
    nose(i / 12, 1, np)
    noseRim.push([np.x, np.y, np.z])
  }
  k.fan([spine(Z1), topAt(Z1, 0), Z1 - 0.006], noseRim, rockPaint, false)
  // surf at the foot of the cliffs
  const surf: Paint = { c: 'foam', c2: 'sea', t: 0.35, gloss: 0.4 }
  k.surface(
    30, 1,
    (u, v, out) => {
      const z = 0.012 + (Z1 + 0.012 - 0.012) * u
      const zz = Math.min(z, Z1)
      const w = 0.006 + 0.004 * Math.sin(u * 40)
      out.set(spine(zz) + halfW(zz) + 0.006 + v * w, SEA + 0.0009, z)
    },
    surf,
  )

  /* ---- Fort Frederick: curtain wall and gate across the neck ---- */
  const wallZ = 0.004
  const wall: Paint = { c: 'rampart', c2: 'rampartDark', t: 0.3, k: (p) => 0.85 + 0.15 * smooth(0, 0.012, p.y) }
  const span = halfW(wallZ) + 0.014
  for (const sx of [-1, 1]) {
    k.box(span - 0.012, 0.013, 0.007, wall, { p: [spine(wallZ) + sx * (0.006 + (span - 0.012) / 2), 0.004, wallZ] })
    // angled bastion where the wall meets the water
    k.cyl(0.011, 0.014, 0.016, 5, wall, { p: [spine(wallZ) + sx * span, 0.002, wallZ + 0.002], r: [0, sx * 0.4, 0] })
  }
  // gatehouse with a dark archway
  k.box(0.02, 0.019, 0.01, wall, { p: [spine(wallZ), 0.004, wallZ] })
  k.box(0.0085, 0.011, 0.0104, 'iron', { p: [spine(wallZ), 0.004, wallZ] })
  k.box(0.023, 0.0022, 0.012, { c: 'stone' }, { p: [spine(wallZ), 0.023, wallZ] })
  // the road up the spine to the temple
  for (let i = 0; i < 16; i++) {
    const z0 = -0.03 + i * 0.0125, z1 = z0 + 0.0125
    const x0 = spine(z0) - 0.004 * Math.sin(i), x1 = spine(z1) - 0.004 * Math.sin(i + 1)
    const y0 = Math.max(0.0034, topAt(z0, -0.05) + 0.0016), y1 = Math.max(0.0034, topAt(z1, -0.05) + 0.0016)
    k.quad([x0 + 0.0042, y0, z0], [x0 - 0.0042, y0, z0], [x1 - 0.0042, y1, z1], [x1 + 0.0042, y1, z1], 'path')
  }
  // barracks inside the fort
  for (const [x, z, ry] of [[-0.03, 0.034, 0.2], [0.012, 0.05, -0.1]] as const) {
    k.at({ p: [spine(z) + x, topAt(z, x / halfW(z)) - 0.001, z], r: [0, ry, 0] }, () => {
      k.box(0.02, 0.0065, 0.0095, { c: 'white', glow: 0.14 })
      hipRoof(k, 0.023, 0.0125, 0.0045, 0.012, 'roof', { p: [0, 0.0065, 0] })
    })
  }

  /* ---- Koneswaram on the summit ---- */
  const TZ = 0.186
  const TY = topAt(TZ, 0.3) + 0.001
  k.at({ p: [spine(TZ) + 0.004, TY, TZ] }, () => {
    // paved court
    k.box(0.07, 0.008, 0.054, (_p, n) => (n.y > 0.5 ? { c: 'sand', c2: 'stone', t: 0.4 } : { c: 'cliffLite' }), { p: [0, -0.004, 0] })
    const y = 0.004
    // mandapam halls: cream walls banded red and white at the plinth
    k.box(0.03, 0.009, 0.02, { c: 'cream', glow: 0.12 }, { p: [-0.014, y, -0.008] })
    k.box(0.0306, 0.0022, 0.0206, 'kovilRed', { p: [-0.014, y, -0.008] })
    k.box(0.032, 0.0016, 0.022, 'konePink', { p: [-0.014, y + 0.009, -0.008] })
    // vimana over the sanctum: stepped, gilded finial
    for (let i = 0; i < 3; i++) k.box(0.011 - i * 0.0026, 0.0036, 0.011 - i * 0.0026, i % 2 ? 'koneBlue' : 'cream', { p: [-0.02, y + 0.0106 + i * 0.0036, -0.01] })
    k.cone(0.0026, 0.005, 6, { c: 'gold', glow: 0.5, gloss: 0.6 }, { p: [-0.02, y + 0.0214, -0.01] })
    // the gopuram: five diminishing tiers in pastel, barrel roof, gilded kalasams
    k.at({ p: [-0.016, y, 0.014], s: 1.15 }, () => {
      const tiers = ['koneBlue', 'cream', 'konePink', 'koneGreen', 'cream', 'koneBlue'] as const
      let yy = 0
      k.box(0.02, 0.0085, 0.0125, 'cream')
      k.box(0.0066, 0.006, 0.0128, 'iron')
      yy = 0.0085
      for (let i = 0; i < 5; i++) {
        const f = 1 - i * 0.13
        k.box(0.0205 * f, 0.0052, 0.0125 * f, tiers[i], { p: [0, yy, 0] })
        k.box(0.0222 * f, 0.0011, 0.0142 * f, 'gold', { p: [0, yy + 0.0052, 0] })
        yy += 0.0063
      }
      k.add(new THREE.CylinderGeometry(0.0034, 0.0034, 0.0105, 8, 1, false, 0, Math.PI).rotateZ(Math.PI / 2), 'konePink', { p: [0, yy, 0] })
      for (const x of [-0.0034, 0, 0.0034]) k.cone(0.001, 0.0042, 5, { c: 'gold', glow: 0.7, gloss: 0.6 }, { p: [x, yy + 0.0032, 0] })
    })
    // the seated Shiva on his lotus plinth, facing the sea
    k.at({ p: [0.019, y, 0.004], r: [0, 0.25, 0], s: 1.28 }, () => {
      k.cyl(0.0105, 0.012, 0.004, 8, 'koneBlue')
      k.cyl(0.0098, 0.0082, 0.0038, 10, 'konePink', { p: [0, 0.004, 0] })
      const skin: Paint = { c: 'shiva', k: (p) => 0.86 + 0.14 * smooth(TY + 0.012, TY + 0.03, p.y) }
      // crossed legs, torso, shoulders, head
      k.ball(1, 1, skin, { p: [0, 0.0102, 0.001], s: [0.0098, 0.0036, 0.0074] })
      k.cyl(0.0048, 0.0036, 0.0105, 7, skin, { p: [0, 0.011, -0.001] })
      k.ball(1, 1, skin, { p: [0, 0.0208, -0.001], s: [0.0068, 0.0028, 0.0036] })
      k.ball(0.0034, 1, skin, { p: [0, 0.0262, -0.0008] })
      // four arms: two raised with trident and drum, two resting
      for (const sx of [-1, 1]) {
        k.cyl(0.0011, 0.0013, 0.0092, 5, skin, { p: [sx * 0.0058, 0.0196, -0.001], r: [0, 0, -sx * 0.75] })
        k.cyl(0.0011, 0.0013, 0.008, 5, skin, { p: [sx * 0.0054, 0.0186, 0.0004], r: [0.9, 0, -sx * 0.35] })
      }
      // jata crown, necklace, trident
      k.cone(0.0028, 0.0072, 6, { c: 'gold', gloss: 0.6, glow: 0.35 }, { p: [0, 0.0282, -0.0008] })
      k.cyl(0.0042, 0.0046, 0.0012, 8, { c: 'gold', gloss: 0.6 }, { p: [0, 0.0208, -0.001] })
      k.cyl(0.0004, 0.0004, 0.024, 4, { c: 'gold', gloss: 0.6 }, { p: [-0.0124, 0.012, -0.001] })
      k.box(0.0044, 0.0005, 0.0005, { c: 'gold', gloss: 0.6 }, { p: [-0.0124, 0.032, -0.001] })
      for (const dx of [-0.002, 0, 0.002]) k.cyl(0.0003, 0.0003, 0.0044, 4, { c: 'gold', gloss: 0.6 }, { p: [-0.0124 + dx, 0.0322, -0.001] })
    })
    // flagstaff and oil lamps
    k.cyl(0.0005, 0.0006, 0.022, 5, { c: 'gold', gloss: 0.6 }, { p: [0.002, y, 0.022] })
    for (const [x, z] of [[-0.03, 0.024], [0.03, 0.024], [0.03, -0.022], [0.008, 0.024]] as const)
      k.add(new THREE.OctahedronGeometry(0.0016, 0), { c: 'light', glow: 1.6, phase: 0.1 + Math.abs(x) * 9 + z * 3 }, { p: [x, y + 0.004, z] })
  })

  /* ---- trees on the rock ---- */
  for (let i = 0, placed = 0; i < 500 && placed < 60; i++) {
    const z = 0.0 + rand() * 0.17
    const xi = -1.5 + rand() * 2.2
    const hw = halfW(z)
    const x = spine(z) + hw * xi
    if (xi > 0.72 || Math.abs(x - spine(z)) < 0.008) continue
    if (z > TZ - 0.04 && xi > -0.75) continue
    let y: number
    if (xi < -1) {
      const t = 1 - (-xi - 1) * hw / (0.012 + 0.34 * crest(z))
      if (t < 0.25) continue
      y = THREE.MathUtils.lerp(SEA, topAt(z, -1), Math.pow(t, 0.8))
    } else y = topAt(z, xi)
    if (y < 0.004) continue
    const s = 0.005 + rand() * 0.0045
    tree(k, x, z, s, s * 1.9, placed % 3 ? 'foliage' : 'foliageDeep', i, y - s * 0.4)
    placed++
  }

  /* ---- the harbour: quay, tank farm, boats ---- */
  k.drape = 0
  // stone quay running out into the lee of the rock
  const quay = (_p: THREE.Vector3, n: THREE.Vector3): Paint => (n.y > 0.5 ? { c: 'stoneWarm', c2: 'rampart', t: 0.25 } : { c: 'rampart', c2: 'rampartDark', t: 0.4 })
  k.box(0.0095, 0.0025 - SEA, 0.07, quay, { p: [-0.118, SEA, 0.026] })
  k.box(0.034, 0.0025 - SEA, 0.0095, quay, { p: [-0.106, SEA, 0.056] })
  for (const z of [0.004, 0.03, 0.056]) k.add(new THREE.OctahedronGeometry(0.0015, 0), { c: 'light', glow: 1.5, phase: 0.2 + z * 7 }, { p: [-0.118, 0.008, z] })
  // China Bay's oil tanks
  for (const [x, z, r] of [[-0.152, -0.03, 0.0105], [-0.128, -0.045, 0.0105], [-0.155, -0.058, 0.0105], [-0.105, -0.026, 0.008]] as const) {
    k.cyl(r, r, 0.0105, 12, { c: 'white', c2: 'stone', t: 0.25, k: (p) => 0.84 + 0.16 * smooth(0, 0.01, p.y) }, { p: [x, 0.0015, z] })
    k.cone(r, 0.0022, 12, 'whiteShade', { p: [x, 0.012, z] })
  }
  // harbour sheds
  for (const [x, z] of [[-0.085, -0.02], [-0.064, -0.034]] as const) {
    k.box(0.022, 0.006, 0.011, { c: 'cream', glow: 0.12 }, { p: [x, 0.0015, z], r: [0, 0.2, 0] })
    hipRoof(k, 0.024, 0.013, 0.004, 0.016, 'roof', { p: [x, 0.0075, z], r: [0, 0.2, 0] })
  }
  for (const [x, z] of [[0.09, -0.03], [0.12, -0.05], [0.06, -0.06], [-0.03, -0.05], [0.15, -0.03]] as const) palm(k, x, z, 0.03 + rand() * 0.01, 0.15 + rand() * 0.15, rand() * 6, 'palmTrunk', 'palmLeaf', 0.0015)
  // fishing boats drawn up by the quay
  for (const [x, z, ry, c] of [[-0.09, 0.03, 0.4, 'koneBlue'], [-0.078, 0.012, 1.2, 'red'], [-0.14, 0.04, -0.3, 'white']] as const) {
    k.prism([[-0.008, -0.0022], [0.006, -0.0022], [0.0105, 0], [0.006, 0.0022], [-0.008, 0.0022]], 0, 0.0028, c, { p: [x, SEA, z], r: [0, ry, 0] })
  }

  /* ---- Round Island and its light ---- */
  const IX = 0.082, IZ = 0.252
  k.add(new THREE.IcosahedronGeometry(1, 1).translate(0, 0.92, 0), rockPaint, { p: [IX, SEA, IZ], s: [0.024, 0.0068, 0.019], r: [0, 0.7, 0] })
  k.add(new THREE.IcosahedronGeometry(1, 0).translate(0, 0.9, 0), rockPaint, { p: [IX + 0.021, SEA, IZ - 0.012], s: [0.009, 0.0036, 0.008], r: [0, 0.2, 0] })
  k.at({ p: [IX - 0.002, SEA + 0.012, IZ] }, () => {
    k.cyl(0.0052, 0.006, 0.0032, 8, 'stone')
    k.lathe([[0.0042, 0.0032], [0.003, 0.034]], 10, { c: 'white', smooth: true, k: (p) => 0.85 + 0.15 * smooth(SEA + 0.014, SEA + 0.04, p.y) })
    k.cyl(0.0044, 0.0044, 0.0012, 10, 'iron', { p: [0, 0.034, 0] })
    k.cyl(0.0025, 0.0025, 0.0046, 8, { c: 'light', glow: 2.6, phase: 0.45 }, { p: [0, 0.0352, 0] })
    k.cone(0.0034, 0.0038, 8, 'red', { p: [0, 0.0398, 0] })
  })
  k.surface(
    14, 1,
    (u, v, out) => {
      const a = -u * Math.PI * 2
      const r = 1 + v * 0.3
      out.set(IX + Math.cos(a) * 0.024 * r, SEA + 0.0009, IZ + Math.sin(a) * 0.019 * r)
    },
    surf,
    { closeU: true },
  )

  /* ---- a navy patrol ship riding at anchor in the bay ---- */
  const ship: Mover = {
    geometry: shipGeometry(),
    update: (obj, t) => {
      obj.position.set(-0.118, SEA + 0.0006 + Math.sin(t * 1.1) * 0.0005, 0.118)
      obj.rotation.set(Math.sin(t * 0.8) * 0.035, 0.55 + Math.sin(t * 0.17) * 0.05, Math.sin(t * 0.9 + 1) * 0.02)
    },
  }

  return { body: k.build(), movers: [ship] }
}
