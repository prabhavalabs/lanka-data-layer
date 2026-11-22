// Lotus Tower (Nelum Kuluna), Colombo, 351.5 m, on the bank of Beira Lake.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - 351.5 m to the tip of the mast: the tallest structure in South Asia
//   - a slender concrete shaft, green as a lotus stalk, tapering from the podium
//   - the tower house is a lotus bud: green sepals at its base, magenta-pink
//     petals closing over seven floors, widest (~45 m) a little below its middle;
//     the bud sits at roughly 60–75 % of the total height
//   - above the bud a short crown, then a long white lattice mast (the top quarter)
//   - the podium is a lotus throne of two inverted trapezoids, six floors, petal
//     fins in purple-pink
//   - lit at night in its own colours: the one landmark here allowed to glow in
//     colour rather than warm white
//
// Shaft and bud radii are ~1.4× true against height so the tower survives being
// 50 px tall; the stations along its height are true.
import * as THREE from 'three'
import { Kit, apron, blob, curve, hash2, rng, smooth, tree, type Paint } from '../kit'
import type { Built } from '../types'

const H = 0.44
const POD = 0.036 // podium height
const BUD0 = 0.262, BUD1 = 0.33 // 60 % → 75 % of H
const MAST0 = 0.346

const budR = (v: number) => curve([[0, 0.0086], [0.08, 0.0145], [0.24, 0.0236], [0.42, 0.0276], [0.6, 0.0262], [0.78, 0.0206], [0.92, 0.0138], [1, 0.0096]], v)
const PETALS = 8

export function buildLotusTower(): Built {
  const k = new Kit()
  const rand = rng(53)

  /* ---- site: plaza, Beira Lake ---- */
  // Colombo's shore is a few hundred metres west of the tower, so the site leans east: lake and city on the landward side
  apron(k, blob(0.185, 0.165, 0.08, 6), (p) => ({ c: 'ground', c2: 'stone', t: smooth(0.078, 0.07, Math.hypot(p.x, p.z)), m: [['cream', smooth(0.078, 0.07, Math.hypot(p.x, p.z)) * 0.4]] }), { cx: 0.03, cz: -0.012, seg: 36, rings: 9 })
  k.drape = 1
  const LKX = 0.092, LKZ = 0.082
  const lake: [number, number][] = []
  for (let i = 0; i < 20; i++) {
    const a = (i / 20) * Math.PI * 2
    const r = 1 + 0.16 * Math.sin(a * 3 + 1)
    lake.push([LKX + Math.cos(a) * 0.07 * r, LKZ + Math.sin(a) * 0.04 * r])
  }
  k.rigidAt(LKX, LKZ, () => {
    k.prism(lake.map(([x, z]) => [LKX + (x - LKX) * 1.06, LKZ + (z - LKZ) * 1.08]), 0.0012, 0.0026, 'stone')
    k.prism(lake, 0.0016, 0.0032, { c: 'water', gloss: 0.9 })
  })
  k.drape = 0

  /* ---- podium: the lotus throne ---- */
  const fins = 16
  const podium = (p: THREE.Vector3): Paint => {
    const a = Math.atan2(p.z, p.x)
    const rib = 0.5 + 0.5 * Math.cos(a * fins)
    return { c: 'lotusPodium', c2: 'lotusPetalLite', t: smooth(0.55, 0.95, rib) * 0.8, glow: -0.22, k: 0.84 + 0.16 * smooth(0.003, POD, p.y) }
  }
  k.surface(
    64, 9,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      // two inverted trapezoids, the upper smaller, with a waist between
      const r = curve([[0, 0.044], [0.34, 0.058], [0.4, 0.05], [0.48, 0.043], [0.82, 0.05], [0.9, 0.04], [1, 0.0155]], v)
      const y = curve([[0, 0], [0.34, 0.0125], [0.4, 0.0155], [0.48, 0.017], [0.82, 0.029], [0.9, 0.0325], [1, POD]], v)
      const flute = 1 + 0.045 * Math.cos(th * fins) * smooth(0.02, 0.3, v) * (1 - smooth(0.86, 1, v))
      out.set(Math.cos(th) * r * flute, 0.0034 + y, Math.sin(th) * r * flute)
    },
    podium,
    { closeU: true },
  )
  // glazed band in the waist
  k.cyl(0.0445, 0.0445, 0.0026, 32, { c: 'glass', glow: 0.55, gloss: 0.7 }, { p: [0, 0.0034 + 0.0152, 0] })

  /* ---- shaft ---- */
  const shaft = (p: THREE.Vector3): Paint => {
    const a = Math.atan2(p.z, p.x)
    // vertical ribs: the service slots that run the height of the stalk
    const rib = smooth(0.82, 0.95, Math.abs(Math.cos(a * 4)))
    return { c: 'lotusStem', c2: 'white', t: rib * 0.5, smooth: true, glow: -0.16, k: 0.86 + 0.14 * smooth(POD, POD + 0.05, p.y) }
  }
  k.lathe([[0.0146, POD], [0.0118, POD + 0.05], [0.0098, 0.16], [0.0086, BUD0 + 0.004]], 24, shaft)
  // maintenance rings
  for (const y of [0.105, 0.17, 0.228]) k.cyl(0.0112 - (y - 0.1) * 0.012, 0.0112 - (y - 0.1) * 0.012, 0.0016, 16, { c: 'lotusCalyx', smooth: true }, { p: [0, y, 0] })

  /* ---- the bud ---- */
  const lobeAt = (th: number) => 0.5 + 0.5 * Math.cos(th * PETALS) // 1 on a petal's spine, 0 in the seam
  // each petal bellies out along its spine, the seams tuck in, and near the top the tips stand proud of one another
  const budAt = (th: number, v: number) => budR(v) * (1 + 0.13 * (lobeAt(th) - 0.45) * Math.sin(Math.min(1, v * 1.15) * Math.PI) + 0.12 * (lobeAt(th) - 0.5) * smooth(0.72, 1, v))
  const bud = (p: THREE.Vector3): Paint => {
    const v = (p.y - BUD0) / (BUD1 - BUD0)
    const lobe = lobeAt(Math.atan2(p.z, p.x))
    return {
      c: 'lotusPetal',
      c2: 'lotusPetalLite',
      t: smooth(0.3, 1, lobe) * 0.55 + smooth(0.7, 1, v) * 0.35,
      m: [['lotusCalyx', smooth(0.07, 0.03, v)]],
      // seams between petals read darker
      k: 0.82 + 0.18 * smooth(0.0, 0.35, lobe),
      glow: -0.6,
      smooth: true,
    }
  }
  k.surface(
    64, 16,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      const r = budAt(th, v)
      out.set(Math.cos(th) * r, BUD0 + (BUD1 - BUD0) * v, Math.sin(th) * r)
    },
    bud,
    { closeU: true },
  )
  // sepals: eight pointed green leaves cupping the foot of the bud, one over each seam
  for (let i = 0; i < PETALS; i++) {
    const mid = ((i + 0.5) / PETALS) * Math.PI * 2
    k.surface(
      4, 6,
      (u, v, out) => {
        const vv = v * 0.46
        const half = (Math.PI / PETALS) * 0.92 * Math.pow(1 - v, 0.8)
        const th = -(mid + (u - 0.5) * 2 * half)
        // a hair proud of the petals; the tip curls away a little
        const r = budAt(th, vv) + 0.0007 + 0.0012 * v * v
        out.set(Math.cos(th) * r, BUD0 + (BUD1 - BUD0) * vv, Math.sin(th) * r)
      },
      { c: 'lotusCalyx', c2: 'lotusStem', t: (p) => smooth(BUD0, BUD0 + (BUD1 - BUD0) * 0.46, p.y) * 0.45, glow: -0.3, smooth: true },
    )
  }
  // observation deck: a glazed ring at the bud's widest
  k.cyl(0.0283, 0.0283, 0.0022, 32, { c: 'glass', glow: 0.8, gloss: 0.7, smooth: true }, { p: [0, BUD0 + (BUD1 - BUD0) * 0.44, 0] })

  /* ---- crown and mast ---- */
  k.lathe([[0.0096, BUD1], [0.0108, BUD1 + 0.004], [0.0084, BUD1 + 0.0095], [0.0052, MAST0]], 16, { c: 'lotusGold', c2: 'white', t: 0.3, smooth: true, glow: 0.3 })
  // lattice mast: white, stepped in three sections
  k.lathe([[0.0038, MAST0], [0.0031, MAST0 + 0.03], [0.0024, MAST0 + 0.03], [0.002, MAST0 + 0.06], [0.0014, MAST0 + 0.06], [0.0009, H - 0.006]], 8, { c: 'white', c2: 'stone', t: 0.3, smooth: true, glow: 0.2 })
  k.add(new THREE.OctahedronGeometry(0.0026, 0), { c: 'red', glow: -3, phase: 0.5 }, { p: [0, H - 0.004, 0] })

  /* ---- the city behind it: quiet, lower, lit by night ---- */
  const blocks: [number, number, number, number, number][] = [
    [0.1, -0.07, 0.026, 0.026, 0.118], [0.132, -0.044, 0.026, 0.026, 0.118], // the twin towers
    [0.056, -0.108, 0.03, 0.022, 0.082], [0.012, -0.126, 0.024, 0.03, 0.062], [-0.034, -0.112, 0.034, 0.02, 0.094],
    [-0.074, -0.082, 0.022, 0.026, 0.07], [0.166, -0.006, 0.026, 0.022, 0.058], [0.15, -0.092, 0.024, 0.024, 0.074],
    [0.102, -0.128, 0.03, 0.02, 0.04], [0.176, 0.04, 0.028, 0.02, 0.032], [-0.066, -0.132, 0.03, 0.02, 0.04],
  ]
  k.drape = 1
  blocks.forEach(([x, z, w, d, h], i) => {
    const tone = hash2(i * 7.3, 2.1)
    const wall: Paint = { c: 'glass', c2: 'stone', t: 0.35 + tone * 0.45, k: (p) => 0.8 + 0.2 * smooth(0, h * 0.6, p.y) }
    k.rigidAt(x, z, () => {
      k.box(w, h, d, wall, { p: [x, 0.0015, z] })
      k.box(w * 0.6, 0.004, d * 0.6, 'stoneDark', { p: [x, 0.0015 + h, z] })
      // lit floors
      const floors = Math.max(2, Math.round(h / 0.012))
      for (let f = 1; f < floors; f++) {
        if (hash2(i * 3.1 + f, f * 1.7) < 0.3) continue
        k.box(w * 1.012, 0.0022, d * 1.012, { c: 'glass', c2: 'light', t: 0.5, glow: 0.55 + 0.5 * hash2(f, i) }, { p: [x, 0.0015 + (f / floors) * h, z] })
      }
    })
  })
  for (let i = 0; i < 40; i++) {
    const a = rand() * Math.PI * 2, r = 0.09 + rand() * 0.085
    const x = 0.03 + Math.cos(a) * r, z = -0.012 + Math.sin(a) * r * 0.9
    if (Math.hypot(x, z) < 0.084) continue
    if (blocks.some(([bx, bz, w, d]) => Math.abs(x - bx) < w / 2 + 0.01 && Math.abs(z - bz) < d / 2 + 0.01)) continue
    if (Math.hypot((x - LKX) / 0.085, (z - LKZ) / 0.052) < 1) continue
    const s = 0.006 + rand() * 0.005
    tree(k, x, z, s * 1.1, s * 1.9, i % 2 ? 'foliage' : 'foliageLite', i, 0.0015)
  }
  k.drape = 0

  return { body: k.build() }
}
