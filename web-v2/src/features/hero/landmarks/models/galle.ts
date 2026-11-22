// Galle Fort and its lighthouse, Point Utrecht Bastion.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - the lighthouse (1939) is a white, slightly tapered round concrete tower,
//     26.5 m high, with a railed gallery and a glazed lantern under a white cap;
//     it stands on the bastion ≈6 m above the road, on an ochre plinth building
//   - it is ringed by coconut palms, taller than the gallery in every photograph
//   - Dutch ramparts (1663): dark, battered granite-and-coral walls with a grassed
//     terreplein, bastions pushing out into the surf, black rocks at their foot
//   - the white Meeran Jumma Mosque (baroque gables, two small towers) stands just
//     inland of the light; low Dutch houses with red-tile roofs behind; the
//     clock tower on the landward rampart
//
// Model frame: +z = out to sea. The lighthouse is ≈2.5× true size against the
// fort so it reads as the subject; its own proportions (height ≈ 11 × diameter
// at the gallery) are kept.
import * as THREE from 'three'
import { Kit, apron, blob, fbm, hipRoof, palm, rng, smooth, tree, type Paint } from '../kit'
import type { BuildCtx, Built, Mover } from '../types'

const GY = 0.013 // terreplein height above the land
const OUTLINE: [number, number][] = [
  [-0.17, -0.05], [-0.172, 0.012], [-0.15, 0.042], [-0.1, 0.05], [-0.05, 0.044], [-0.01, 0.062], [0.03, 0.094], [0.07, 0.118], // Point Utrecht
  [0.104, 0.1], [0.112, 0.062], [0.102, 0.03], [0.14, 0.012], [0.172, -0.012], [0.17, -0.05],
]
const LX = 0.066, LZ = 0.084 // lighthouse

export function buildGalle(ctx: BuildCtx): Built {
  const k = new Kit()
  const rand = rng(67)
  const SEA = ctx.sea

  /* ---- the town's ground behind the walls ---- */
  apron(k, blob(0.21, 0.1, 0.07, 9), 'ground', { cz: -0.1, seg: 30, rings: 4 })

  /* ---- ramparts: battered curtain, grassed terreplein ---- */
  const stone = (p: THREE.Vector3): Paint => {
    const s = fbm(p.x * 70 + p.z * 70, p.y * 90, 3)
    return { c: 'rampart', c2: 'rampartDark', t: smooth(0.35, 0.75, s) * 0.8 + smooth(SEA + 0.012, SEA, p.y) * 0.5, k: 0.84 + 0.16 * smooth(SEA, GY, p.y), m: [['foliage', smooth(0.62, 0.8, s) * smooth(SEA + 0.016, GY, p.y) * 0.35]] }
  }
  // the batter: a skirt that leans in from the waterline to the wall top
  const N = OUTLINE.length
  const cx = 0, cz = 0.02
  const ring = (grow: number, y: number): [number, number, number][] => OUTLINE.map(([x, z]) => [cx + (x - cx) * grow, y, cz + (z - cz) * grow])
  const foot = ring(1.075, SEA), head = ring(1.0, GY) // nothing below the waterline: the sea is drawn without depth
  for (let i = 0; i < N - 1; i++) {
    // subdivide each face so the stains have vertices to live on
    const a0 = foot[i], a1 = foot[i + 1], b0 = head[i], b1 = head[i + 1]
    const SEG = 4, ROWS = 3
    for (let s = 0; s < SEG; s++)
      for (let r = 0; r < ROWS; r++) {
        const pt = (u: number, v: number): [number, number, number] => {
          const lo = [a0[0] + (a1[0] - a0[0]) * u, a0[1], a0[2] + (a1[2] - a0[2]) * u]
          const hi = [b0[0] + (b1[0] - b0[0]) * u, b0[1], b0[2] + (b1[2] - b0[2]) * u]
          return [lo[0] + (hi[0] - lo[0]) * v, lo[1] + (hi[1] - lo[1]) * v, lo[2] + (hi[2] - lo[2]) * v]
        }
        k.quad(pt(s / SEG, r / ROWS), pt((s + 1) / SEG, r / ROWS), pt((s + 1) / SEG, (r + 1) / ROWS), pt(s / SEG, (r + 1) / ROWS), stone)
      }
  }
  k.prism(OUTLINE, GY - 0.004, GY, (p) => ({ c: 'lawn', c2: 'foliageLite', t: fbm(p.x * 60, p.z * 60, 2) * 0.7, m: [['sand', smooth(0.6, 0.8, fbm(p.x * 25 + 3, p.z * 25, 2)) * 0.5]] }))
  // parapet along the seaward edges
  for (let i = 1; i < N - 2; i++) {
    const [x0, z0] = OUTLINE[i], [x1, z1] = OUTLINE[i + 1]
    const len = Math.hypot(x1 - x0, z1 - z0)
    k.box(len + 0.003, 0.0046, 0.0042, stone, { p: [(x0 + x1) / 2, GY, (z0 + z1) / 2], r: [0, -Math.atan2(z1 - z0, x1 - x0), 0] })
  }
  // black rocks and surf under the bastion
  const surf: Paint = { c: 'foam', c2: 'sea', t: 0.3, gloss: 0.4 }
  for (let i = 1; i < N - 2; i++) {
    const [x0, z0] = OUTLINE[i], [x1, z1] = OUTLINE[i + 1]
    for (let j = 0; j < 3; j++) {
      const t = (j + rand()) / 3
      const x = cx + (x0 + (x1 - x0) * t - cx) * 1.12, z = cz + (z0 + (z1 - z0) * t - cz) * 1.12
      const s = 0.005 + rand() * 0.006
      k.add(new THREE.IcosahedronGeometry(1, 0).translate(0, 0.9, 0), { c: 'rampartDark', c2: 'iron', t: rand() * 0.5 }, { p: [x, SEA, z], r: [0, rand() * 3, 0], s: [s * 1.3, s * 0.5, s] })
    }
    const o0: [number, number] = [cx + (x0 - cx) * 1.085, cz + (z0 - cz) * 1.085], o1: [number, number] = [cx + (x1 - cx) * 1.085, cz + (z1 - cz) * 1.085]
    const p0: [number, number] = [cx + (x0 - cx) * 1.17, cz + (z0 - cz) * 1.17], p1: [number, number] = [cx + (x1 - cx) * 1.17, cz + (z1 - cz) * 1.17]
    k.quad([o0[0], SEA + 0.0009, o0[1]], [p0[0], SEA + 0.0009, p0[1]], [p1[0], SEA + 0.0009, p1[1]], [o1[0], SEA + 0.0009, o1[1]], surf)
  }

  /* ---- the lighthouse ---- */
  const TH = 0.118 // tower shaft
  k.at({ p: [LX, GY, LZ] }, () => {
    // ochre keeper's plinth with white trim
    k.box(0.034, 0.011, 0.028, { c: 'ochreWall', k: (p) => 0.86 + 0.14 * smooth(GY, GY + 0.011, p.y) }, { r: [0, 0.5, 0] })
    k.box(0.036, 0.0016, 0.03, 'white', { p: [0, 0.011, 0], r: [0, 0.5, 0] })
    const y0 = 0.0126
    k.cyl(0.0128, 0.0138, 0.005, 16, { c: 'white', smooth: true }, { p: [0, y0, 0] })
    // the shaft: 11 diameters tall, gently tapered
    k.lathe([[0.0118, y0 + 0.005], [0.0102, y0 + TH * 0.5], [0.009, y0 + TH]], 20, { c: 'white', smooth: true, glow: 0.1, k: (p) => 0.84 + 0.16 * smooth(GY + 0.015, GY + 0.08, p.y) })
    // window slits up the seaward side
    for (let i = 0; i < 4; i++) k.box(0.0026, 0.0052, 0.0012, 'iron', { p: [0, y0 + 0.024 + i * 0.024, 0.0111 - i * 0.0005], r: [0, 0, 0] })
    k.box(0.0044, 0.0085, 0.0014, 'iron', { p: [0, y0 + 0.005, 0.0122] })
    // gallery, lantern, cap
    const yg = y0 + TH
    k.cyl(0.0138, 0.011, 0.0026, 16, { c: 'white', smooth: true }, { p: [0, yg, 0] })
    k.cyl(0.014, 0.014, 0.0009, 16, 'iron', { p: [0, yg + 0.0026, 0] })
    for (let i = 0; i < 12; i++) {
      const a = (i / 12) * Math.PI * 2
      k.cyl(0.0004, 0.0004, 0.0046, 4, 'iron', { p: [Math.cos(a) * 0.0132, yg + 0.0035, Math.sin(a) * 0.0132] })
    }
    k.add(new THREE.TorusGeometry(0.0132, 0.0004, 4, 20).rotateX(Math.PI / 2), 'iron', { p: [0, yg + 0.0082, 0] })
    k.cyl(0.0074, 0.0078, 0.0026, 12, { c: 'white', smooth: true }, { p: [0, yg + 0.0035, 0] })
    k.cyl(0.0068, 0.0068, 0.0092, 12, { c: 'light', c2: 'glass', t: 0.25, glow: 2.6, gloss: 0.8 }, { p: [0, yg + 0.0061, 0] })
    for (let i = 0; i < 6; i++) {
      const a = (i / 6) * Math.PI * 2
      k.cyl(0.0005, 0.0005, 0.0092, 4, 'iron', { p: [Math.cos(a) * 0.0069, yg + 0.0061, Math.sin(a) * 0.0069] })
    }
    k.cyl(0.0082, 0.0082, 0.0012, 12, 'white', { p: [0, yg + 0.0153, 0] })
    k.add(new THREE.SphereGeometry(0.0076, 12, 6, 0, Math.PI * 2, 0, Math.PI / 2), { c: 'whiteShade', smooth: true }, { p: [0, yg + 0.0165, 0], s: [1, 0.78, 1] })
    k.cyl(0.0005, 0.0009, 0.0072, 5, 'iron', { p: [0, yg + 0.0222, 0] })
  })

  /* ---- the palms that frame it ---- */
  for (const [dx, dz, h, lean, yaw] of [[-0.03, 0.006, 0.078, 0.22, 2.6], [-0.02, -0.022, 0.092, 0.16, 1.9], [0.022, -0.02, 0.07, 0.24, 0.4], [-0.046, -0.01, 0.062, 0.3, 3.4], [0.012, 0.022, 0.058, 0.26, 5.4], [-0.012, 0.026, 0.066, 0.2, 4.4], [0.03, 0.004, 0.084, 0.14, 0.1]] as const)
    palm(k, LX + dx, LZ + dz, h, lean, yaw, 'palmTrunk', 'palmLeaf', GY)

  /* ---- Meeran Mosque ---- */
  k.at({ p: [-0.012, GY, 0.022], r: [0, 0.18, 0] }, () => {
    const w: Paint = { c: 'white', glow: 0.12, k: (p) => 0.86 + 0.14 * smooth(GY, GY + 0.02, p.y) }
    k.box(0.05, 0.018, 0.028, w)
    hipRoof(k, 0.052, 0.03, 0.006, 0.03, 'whiteShade', { p: [0, 0.018, 0] })
    // baroque frontispiece: a stepped, scrolled gable between two little towers
    const gable = new THREE.Shape()
    gable.moveTo(-0.017, 0)
    gable.lineTo(0.017, 0)
    gable.lineTo(0.017, 0.02)
    gable.quadraticCurveTo(0.012, 0.022, 0.011, 0.027)
    gable.quadraticCurveTo(0.006, 0.028, 0, 0.034)
    gable.quadraticCurveTo(-0.006, 0.028, -0.011, 0.027)
    gable.quadraticCurveTo(-0.012, 0.022, -0.017, 0.02)
    k.slab(gable, 0.004, w, { p: [0, 0, 0.0152] }, 4)
    k.box(0.0062, 0.0105, 0.0046, 'iron', { p: [0, 0, 0.0156] })
    for (const e of [-1, 1]) {
      k.box(0.0085, 0.03, 0.0085, w, { p: [e * 0.0225, 0, 0.0128] })
      k.add(new THREE.SphereGeometry(0.0046, 8, 5, 0, Math.PI * 2, 0, Math.PI / 2), { c: 'white', smooth: true }, { p: [e * 0.0225, 0.03, 0.0128] })
      k.cyl(0.0004, 0.0004, 0.005, 4, 'gold', { p: [e * 0.0225, 0.0345, 0.0128] })
    }
  })

  /* ---- Dutch houses, church, clock tower ---- */
  const houses: [number, number, number, number, number][] = [
    [-0.13, 0.016, 0.03, 0.016, 0.1], [-0.094, 0.02, 0.028, 0.015, -0.05], [-0.06, 0.012, 0.026, 0.016, 0.12], [-0.136, -0.016, 0.03, 0.017, 0.0], [-0.098, -0.014, 0.03, 0.016, 0.06],
    [-0.058, -0.02, 0.028, 0.016, -0.08], [0.034, -0.022, 0.03, 0.016, 0.2], [0.076, -0.02, 0.028, 0.017, 0.1], [0.118, -0.018, 0.03, 0.016, -0.1], [0.132, 0.0, 0.022, 0.014, 0.5],
  ]
  for (const [x, z, w, d, ry] of houses) {
    k.at({ p: [x, GY, z], r: [0, ry, 0] }, () => {
      k.box(w, 0.0078, d, { c: 'cream', c2: 'white', t: rand() * 0.8, glow: 0.14, k: (p) => 0.86 + 0.14 * smooth(GY, GY + 0.008, p.y) })
      hipRoof(k, w + 0.004, d + 0.004, 0.0062, w * 0.55, { c: 'roof', c2: 'roofDark', t: rand() * 0.5 }, { p: [0, 0.0078, 0] })
      // verandah lamp
      k.add(new THREE.OctahedronGeometry(0.0013, 0), { c: 'light', glow: 1.5, phase: 0.1 + rand() * 0.8 }, { p: [w * 0.3, 0.004, d / 2 + 0.002] })
    })
  }
  // clock tower on the landward wall
  k.at({ p: [-0.15, GY, -0.03] }, () => {
    k.box(0.013, 0.04, 0.013, { c: 'cream', glow: 0.1, k: (p) => 0.84 + 0.16 * smooth(GY, GY + 0.04, p.y) })
    k.box(0.0152, 0.002, 0.0152, 'stone', { p: [0, 0.04, 0] })
    k.box(0.011, 0.01, 0.011, 'cream', { p: [0, 0.042, 0] })
    for (let i = 0; i < 4; i++) k.at({ r: [0, (i * Math.PI) / 2, 0] }, () => k.cyl(0.0036, 0.0036, 0.0007, 10, { c: 'white', glow: 1.1 }, { p: [0, 0.047, 0.0057], r: [Math.PI / 2, 0, 0] }))
    hipRoof(k, 0.0135, 0.0135, 0.009, 0, 'roofDark', { p: [0, 0.052, 0] })
  })
  for (const [x, z] of [[-0.02, -0.03], [0.0, -0.012], [0.1, 0.03], [-0.16, 0.02], [0.15, -0.03], [-0.08, -0.04], [0.058, 0.02]] as const) {
    const s = 0.008 + rand() * 0.005
    tree(k, x, z, s * 1.2, s * 2, rand() > 0.5 ? 'foliage' : 'foliageDeep', 1, GY)
  }
  for (const [x, z] of [[-0.11, 0.034], [0.094, 0.07], [0.13, -0.034], [-0.03, 0.05]] as const) palm(k, x, z, 0.05 + rand() * 0.02, 0.2 + rand() * 0.12, rand() * 6, 'palmTrunk', 'palmLeaf', GY)

  /* ---- the light: two beams sweeping the sea by night ---- */
  // open cones whose vertex colour falls to black at the far end: with additive blending the beam simply runs out
  const beamGeo = new THREE.BufferGeometry()
  {
    const LEN = 0.26
    const one = new THREE.ConeGeometry(0.022, LEN, 14, 4, true).rotateZ(Math.PI / 2).translate(LEN / 2, 0, 0).toNonIndexed()
    const a = one.attributes.position.array as Float32Array
    const n = a.length
    const pos = new Float32Array(n * 2), col = new Float32Array(n * 2)
    for (let i = 0; i < n; i += 3) {
      pos.set([a[i], a[i + 1], a[i + 2]], i)
      pos.set([-a[i], a[i + 1], -a[i + 2]], n + i) // the opposite beam: a half-turn about y
      const fade = Math.pow(1 - Math.min(1, Math.abs(a[i]) / LEN), 1.6)
      col.set([fade, fade, fade], i)
      col.set([fade, fade, fade], n + i)
    }
    beamGeo.setAttribute('position', new THREE.BufferAttribute(pos, 3))
    beamGeo.setAttribute('color', new THREE.BufferAttribute(col, 3))
    one.dispose()
  }
  const beam: Mover = {
    geometry: beamGeo,
    kind: 'beam',
    update: (obj, t) => {
      obj.position.set(LX, GY + 0.0126 + TH + 0.0105, LZ)
      obj.rotation.y = t * 0.55
    },
  }

  return { body: k.build(), movers: [beam] }
}
