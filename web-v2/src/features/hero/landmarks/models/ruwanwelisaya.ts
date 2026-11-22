// Ruwanwelisaya (Mahathupa), Anuradhapura, c. 140 BC.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - 103 m high, 290 m round the base (≈92 m across): height ≈ 1.12 × diameter
//   - whitewashed "bubble" dome (bubbulakara) on three basal rings (pesa walalu),
//     the lowest dressed with a red band
//   - square harmika (hatharas kotuwa), cylindrical devata kotuwa, a ringed
//     conical spire (kot kerella) and a gilded pinnacle carrying a rock crystal
//   - four vahalkada frontispieces at the cardinal points
//   - it stands on a square paved platform whose retaining wall is a parade of
//     elephants shoulder to shoulder (the hasthi prakara), with a small stupa at
//     each corner; a sand terrace and an outer wall surround that
//
// Proportion kept at 1.12 : 1 from platform to pinnacle.
import * as THREE from 'three'
import { Kit, apron, blob, rng, smooth, tree, type Paint } from '../kit'
import type { Built } from '../types'

const DR = 0.086 // dome radius
const PLAT = 0.134 // half side of the elephant platform
const PH = 0.016 // platform height

function miniStupa(k: Kit, x: number, z: number, s: number) {
  k.at({ p: [x, PH, z], s }, () => {
    k.cyl(0.0115, 0.0125, 0.003, 12, 'white')
    k.lathe([[0.0105, 0.003], [0.0105, 0.0065], [0.0092, 0.0105], [0.0062, 0.014], [0.0028, 0.0156]], 12, { c: 'white', smooth: true })
    k.box(0.0056, 0.003, 0.0056, 'white', { p: [0, 0.0152, 0] })
    k.cone(0.0024, 0.0105, 8, { c: 'white', smooth: true }, { p: [0, 0.0182, 0] })
    k.cone(0.0009, 0.004, 5, { c: 'gold', glow: 0.5, gloss: 0.6 }, { p: [0, 0.0282, 0] })
  })
}

export function buildRuwanwelisaya(): Built {
  const k = new Kit()
  const rand = rng(41)

  /* ---- sand terrace and its outer wall ---- */
  apron(k, blob(0.25, 0.25, 0.05, 3), (p) => ({ c: 'ground', c2: 'sand', t: smooth(0.24, 0.19, Math.max(Math.abs(p.x), Math.abs(p.z))) }), { seg: 32, rings: 4 })
  const OUT = 0.186
  k.plate(-OUT, -OUT, OUT, OUT, 0.0022, { c: 'sand' })
  const outer: Paint = { c: 'white', k: (p) => 0.85 + 0.15 * smooth(0.002, 0.008, p.y) }
  for (const s of [-1, 1]) {
    for (const half of [-1, 1]) {
      // each side is two runs with a gate between them
      k.box(OUT - 0.02, 0.0062, 0.0038, outer, { p: [half * (0.02 + (OUT - 0.02) / 2), 0.0022, s * OUT] })
      k.box(0.0038, 0.0062, OUT - 0.02, outer, { p: [s * OUT, 0.0022, half * (0.02 + (OUT - 0.02) / 2)] })
    }
  }

  /* ---- the elephant platform ---- */
  k.box(PLAT * 2, PH, PLAT * 2, (p, n) => (n.y > 0.5 ? { c: 'stone', c2: 'sand', t: 0.35 + 0.2 * Math.sin(p.x * 180) * Math.sin(p.z * 180) } : { c: 'whiteShade' }), { p: [0, 0.0022 - 0.002, 0] })
  // coping
  k.box(PLAT * 2 + 0.006, 0.0026, PLAT * 2 + 0.006, 'white', { p: [0, PH - 0.0016, 0] })
  // the elephants: forequarters standing out of the wall on all four sides
  const ele: Paint = { c: 'stoneDark', c2: 'iron', t: 0.35, k: (p) => 0.78 + 0.22 * smooth(0.003, 0.012, p.y) }
  const NE = 15
  for (let side = 0; side < 4; side++) {
    k.at({ r: [0, (side * Math.PI) / 2, 0] }, () => {
      for (let i = 0; i < NE; i++) {
        const x = -PLAT + ((i + 0.5) / NE) * PLAT * 2
        if (Math.abs(x) < 0.022) continue // stairway
        k.at({ p: [x, 0.0022, PLAT] }, () => {
          k.box(0.0118, 0.0088, 0.006, ele, { p: [0, 0, 0.003] }) // shoulders
          k.box(0.0072, 0.0074, 0.0044, ele, { p: [0, 0.0012, 0.0078] }) // head
          k.box(0.0024, 0.0062, 0.0024, ele, { p: [0, 0, 0.0108] }) // trunk
          for (const e of [-1, 1]) k.box(0.0034, 0.0044, 0.0012, ele, { p: [e * 0.0052, 0.0036, 0.0064] }) // ears
        })
      }
      // stair with guard stones
      k.at({ p: [0, 0.0022, PLAT] }, () => {
        const stair = new THREE.Shape([new THREE.Vector2(0, 0), new THREE.Vector2(0.026, 0), new THREE.Vector2(0, PH - 0.0022)])
        k.slab(stair, 0.03, (_p, n) => (n.y > 0.3 ? { c: 'stone' } : { c: 'whiteShade' }), { r: [0, -Math.PI / 2, 0] })
        for (const e of [-1, 1]) k.box(0.0046, PH + 0.003, 0.028, 'white', { p: [e * 0.0176, 0, 0.013] })
      })
    })
  }

  /* ---- three basal rings, the lowest banded red ---- */
  const y0 = PH
  const white: Paint = { c: 'white', smooth: true }
  k.lathe([[DR + 0.02, y0], [DR + 0.02, y0 + 0.0062], [DR + 0.0135, y0 + 0.0062], [DR + 0.0135, y0 + 0.0124], [DR + 0.007, y0 + 0.0124], [DR + 0.007, y0 + 0.0186], [DR, y0 + 0.0186]], 48, 'white')
  k.lathe([[DR + 0.0206, y0 + 0.0012], [DR + 0.0206, y0 + 0.0046]], 48, { c: 'red', c2: 'roof', t: 0.4 })

  /* ---- the dome: a bubble, a touch fuller than a hemisphere at the haunch ---- */
  const yd = y0 + 0.0186
  const HD = DR * 0.97
  const dome: [number, number][] = []
  for (let i = 0; i <= 16; i++) {
    const t = i / 16
    const a = (t * Math.PI) / 2
    // superellipse: r^p + y^p = 1
    const p = 2.35
    dome.push([DR * Math.pow(Math.cos(a), 2 / p), yd + HD * Math.pow(Math.sin(a), 2 / p)])
  }
  k.lathe(dome, 48, {
    ...white,
    glow: 0.16,
    // grounded at the springing, brightest on the shoulder
    k: (p) => 0.8 + 0.2 * smooth(yd, yd + HD * 0.4, p.y),
  })

  /* ---- harmika, devata kotuwa, spire, pinnacle ---- */
  const yh = yd + HD - 0.003
  k.box(0.047, 0.018, 0.047, { c: 'white', glow: 0.12, k: (p) => 0.86 + 0.14 * smooth(yh, yh + 0.018, p.y) }, { p: [0, yh, 0] })
  k.box(0.052, 0.003, 0.052, 'white', { p: [0, yh + 0.018, 0] })
  // the sun disc on each face
  for (let side = 0; side < 4; side++) k.at({ r: [0, (side * Math.PI) / 2, 0] }, () => k.cyl(0.0052, 0.0052, 0.0008, 10, { c: 'gold', glow: 0.5, gloss: 0.5 }, { p: [0, yh + 0.0092, 0.0236], r: [Math.PI / 2, 0, 0] }))
  const yk = yh + 0.021
  k.cyl(0.0145, 0.0155, 0.011, 16, { c: 'white', smooth: true, glow: 0.1 }, { p: [0, yk, 0] })
  const spire: [number, number][] = [[0.0152, yk + 0.011]]
  const RINGS = 9, SH = 0.05
  for (let i = 0; i < RINGS; i++) {
    const r0 = 0.0136 * (1 - i / RINGS) + 0.0028, r1 = 0.0136 * (1 - (i + 1) / RINGS) + 0.0028
    const ya = yk + 0.011 + (i / RINGS) * SH, yb = yk + 0.011 + ((i + 1) / RINGS) * SH
    spire.push([r0, ya], [(r0 + r1) / 2 + 0.0006, (ya + yb) / 2], [r1, yb])
  }
  k.lathe(spire, 16, { c: 'white', glow: 0.1, k: (p) => 0.9 + 0.1 * Math.sin(p.y * 1100) })
  const yp = yk + 0.011 + SH
  k.lathe([[0.0034, yp], [0.0042, yp + 0.0022], [0.0026, yp + 0.0052], [0.0018, yp + 0.0088], [0.0008, yp + 0.0128]], 10, { c: 'gold', smooth: true, glow: 0.7, gloss: 0.7 })
  k.add(new THREE.OctahedronGeometry(0.0026, 0), { c: 'light', glow: 2.4, gloss: 0.8 }, { p: [0, yp + 0.0152, 0] })

  /* ---- vahalkadas at the cardinal points ---- */
  for (let side = 0; side < 4; side++) {
    k.at({ r: [0, (side * Math.PI) / 2, 0] }, () => {
      k.at({ p: [0, PH, DR + 0.021] }, () => {
        k.box(0.04, 0.019, 0.011, { c: 'white', glow: 0.14 })
        k.box(0.044, 0.0028, 0.013, 'whiteShade', { p: [0, 0.019, 0] })
        k.box(0.03, 0.0052, 0.01, 'white', { p: [0, 0.0218, 0] })
        k.box(0.018, 0.0046, 0.009, 'white', { p: [0, 0.027, 0] })
        for (const e of [-1, 1]) k.box(0.0052, 0.024, 0.0052, 'white', { p: [e * 0.0226, 0, 0.001] })
        // flower altar and its lamp
        k.box(0.026, 0.0046, 0.006, { c: 'stone' }, { p: [0, 0, 0.0115] })
        k.add(new THREE.OctahedronGeometry(0.0017, 0), { c: 'light', glow: 1.8, phase: 0.2 + side * 0.17 }, { p: [0, 0.0072, 0.0115] })
      })
    })
  }

  /* ---- corner stupas, lamp standards ---- */
  for (const sx of [-1, 1]) for (const sz of [-1, 1]) miniStupa(k, sx * (PLAT - 0.017), sz * (PLAT - 0.017), 0.85)
  for (let i = 0; i < 12; i++) {
    const side = i % 4, t = (Math.floor(i / 4) + 0.5) / 3 - 0.5
    const a = (side * Math.PI) / 2
    const lx = t * PLAT * 1.5, lz = PLAT - 0.006
    const x = lx * Math.cos(a) + lz * Math.sin(a), z = -lx * Math.sin(a) + lz * Math.cos(a)
    if (Math.abs(t) < 0.1) continue
    k.cyl(0.0006, 0.0006, 0.012, 4, 'stoneDark', { p: [x, PH, z] })
    k.add(new THREE.OctahedronGeometry(0.0016, 0), { c: 'light', glow: 1.6, phase: 0.13 + i * 0.07 }, { p: [x, PH + 0.013, z] })
  }

  /* ---- the park around the terrace ---- */
  k.drape = 1
  for (let i = 0; i < 26; i++) {
    const a = rand() * Math.PI * 2
    const r = 0.2 + rand() * 0.04
    const x = Math.cos(a) * r, z = Math.sin(a) * r
    if (Math.max(Math.abs(x), Math.abs(z)) < OUT + 0.008) continue
    const s = 0.009 + rand() * 0.008
    tree(k, x, z, s * 1.2, s * 1.9, i % 3 ? 'foliage' : 'foliageDeep', i, 0.002)
  }
  k.drape = 0

  return { body: k.build() }
}
