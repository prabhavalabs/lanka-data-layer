// Nallur Kandaswamy Kovil, Jaffna.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - the main entrance faces east: a five-storey Dravidian gopuram in golden
//     ochre behind a broad arched, gilded entrance porch (the vasantha mandapam)
//   - Shanmuha Raja Gopuram on the south (2011, nine storeys) and Gubera Raja
//     Gopuram on the north (2015, a little larger: the tallest on the island)
//   - each gopuram is a stepped rectangular pyramid of diminishing storeys with
//     projecting cornices, closed by a barrel vault (sala) carrying a row of
//     gilded kalasams
//   - the compound wall is painted in vertical red and white stripes: the mark
//     of a Jaffna kovil; domed bell towers stand over it
//   - flat-roofed mandapam halls fill the enclosure, a gilded vimana over the
//     sanctum; the holy pond (Shanmuha Theertham) lies in the southern court
//   - white sand forecourt, palmyra palms
//
// Model frame: +z = east (the entrance front), −x = south.
import * as THREE from 'three'
import { Kit, apron, blob, rng, smooth, type Paint } from '../kit'
import type { Built } from '../types'

const WX = 0.118, WZ0 = -0.1, WZ1 = 0.078 // compound wall
const WH = 0.021
const Y0 = 0.003

/**
 * A gopuram centred on the local origin, its long face on local +z.
 * w × d is the base storey; `tiers` storeys diminish above it.
 */
function gopuram(k: Kit, w: number, d: number, tiers: number, tierH: number, baseH: number) {
  const body = (i: number): Paint => ({
    c: i % 2 ? 'gopuram' : 'gopuramLite',
    c2: 'gopuramDeep',
    t: (_p, n) => (Math.abs(n.x) > 0.5 ? 0.35 : 0),
    glow: 0.12,
  })
  // base storey with the doorway
  k.box(w, baseH, d, body(1))
  k.box(w * 0.3, baseH * 0.74, d * 1.02, 'timber')
  k.box(w * 1.08, baseH * 0.12, d * 1.1, 'gopuramDeep', { p: [0, baseH, 0] })
  let y = baseH * 1.12
  const taper = 0.56 / tiers
  for (let i = 0; i < tiers; i++) {
    const f = 1 - i * taper
    const tw = w * f, td = d * (1 - i * taper * 0.86)
    k.box(tw, tierH * 0.78, td, body(i), { p: [0, y, 0] })
    // niches: a row of little aedicules that give the storey its serrated outline
    const n = Math.max(3, Math.round(tw / 0.0075))
    for (let j = 0; j < n; j++) {
      const x = -tw / 2 + ((j + 0.5) / n) * tw
      for (const sz of [-1, 1]) k.box((tw / n) * 0.62, tierH * 0.6, 0.0022, { c: 'gopuramDeep', c2: 'kovilRed', t: j % 2 ? 0.5 : 0 }, { p: [x, y + tierH * 0.1, sz * (td / 2 + 0.0008)] })
    }
    // corner turrets and the cornice
    for (const sx of [-1, 1]) for (const sz of [-1, 1]) k.box(tierH * 0.42, tierH * 0.98, tierH * 0.42, 'gopuramLite', { p: [sx * (tw / 2 - tierH * 0.1), y, sz * (td / 2 - tierH * 0.1)] })
    k.box(tw * 1.07, tierH * 0.22, td * 1.12, { c: 'gopuramDeep', k: 0.92 }, { p: [0, y + tierH * 0.78, 0] })
    y += tierH
  }
  // sala: the barrel vault, horseshoe gables, kalasams
  const tw = w * (1 - tiers * taper) * 1.02, td = d * (1 - tiers * taper * 0.86)
  const r = td * 0.56
  k.add(new THREE.CylinderGeometry(r, r, tw, 10, 1, false, 0, Math.PI).rotateZ(Math.PI / 2), { c: 'gopuramLite', glow: 0.14 }, { p: [0, y, 0] })
  for (const sx of [-1, 1]) k.add(new THREE.CylinderGeometry(r * 1.12, r * 1.12, 0.0016, 10, 1, false, 0, Math.PI).rotateZ(Math.PI / 2), 'gopuramDeep', { p: [sx * (tw / 2 + 0.0008), y, 0] })
  const kal = Math.max(3, Math.round(tw / 0.006))
  for (let j = 0; j < kal; j++) {
    const x = -tw / 2 + ((j + 0.5) / kal) * tw
    k.lathe([[0.0011, y + r], [0.0017, y + r + 0.0016], [0.0007, y + r + 0.0034], [0.0002, y + r + 0.0062]], 6, { c: 'gold', glow: 0.8, gloss: 0.7 }, { p: [x, 0, 0] })
  }
  return y + r + 0.0062
}

function bellTower(k: Kit, x: number, z: number) {
  k.at({ p: [x, Y0, z] }, () => {
    for (let i = 0; i < 4; i++) k.box(0.0036, 0.034, 0.0144, i % 2 ? 'kovilWhite' : 'kovilRed', { p: [-0.0054 + i * 0.0036, 0, 0] })
    k.box(0.017, 0.0022, 0.017, 'gopuramDeep', { p: [0, 0.034, 0] })
    // open belfry
    for (const sx of [-1, 1]) for (const sz of [-1, 1]) k.box(0.0026, 0.011, 0.0026, 'gopuramLite', { p: [sx * 0.0056, 0.0362, sz * 0.0056] })
    k.box(0.006, 0.006, 0.006, { c: 'gold', glow: 0.5, gloss: 0.6 }, { p: [0, 0.039, 0] })
    k.box(0.016, 0.002, 0.016, 'gopuramDeep', { p: [0, 0.0472, 0] })
    k.add(new THREE.SphereGeometry(0.0066, 10, 6, 0, Math.PI * 2, 0, Math.PI / 2), { c: 'kovilRed', smooth: true }, { p: [0, 0.0492, 0], s: [1, 1.15, 1] })
    k.cone(0.0009, 0.005, 5, { c: 'gold', glow: 0.8, gloss: 0.7 }, { p: [0, 0.0566, 0] })
  })
}

/** palmyra: a straight trunk and a tight round crown of stiff fans */
function palmyra(k: Kit, x: number, z: number, h: number, seed: number) {
  k.rigidAt(x, z, () => k.at({ p: [x, 0.0015, z], r: [0, seed * 2.4, 0] }, () => {
    k.cyl(h * 0.022, h * 0.03, h, 5, 'palmTrunk')
    for (let i = 0; i < 9; i++) {
      const a = (i / 9) * Math.PI * 2, tilt = 0.25 + (i % 3) * 0.32
      k.at({ p: [0, h, 0], r: [0, a, 0] }, () => {
        const L = h * 0.26, w = h * 0.1
        const up = Math.sin(tilt) * L, out = Math.cos(tilt) * L
        k.quad([0, 0, 0], [out, up, -w], [out * 1.25, up * 1.25, 0], [out, up, w], 'palmLeaf')
        k.quad([0, 0, 0], [out, up, w], [out * 1.25, up * 1.25, 0], [out, up, -w], 'palmLeaf')
      })
    }
  }))
}

export function buildNallur(): Built {
  const k = new Kit()
  const rand = rng(89)

  /* ---- the white sand of the peninsula ---- */
  apron(k, blob(0.225, 0.205, 0.07, 8), (p) => ({ c: 'ground', c2: 'sand', t: smooth(0.215, 0.165, Math.hypot(p.x * 0.9, p.z)) }), { seg: 36, rings: 7 })

  /* ---- the striped compound wall ---- */
  const STR = 0.0079
  const stripe = (i: number): Paint => ({ c: i % 2 ? 'kovilWhite' : 'kovilRed', k: (p) => 0.84 + 0.16 * smooth(Y0, Y0 + WH, p.y) })
  const run = (x0: number, z0: number, x1: number, z1: number, gap?: [number, number]) => {
    const len = Math.hypot(x1 - x0, z1 - z0)
    const n = Math.round(len / STR)
    const alongX = Math.abs(x1 - x0) > Math.abs(z1 - z0)
    for (let i = 0; i < n; i++) {
      const t = (i + 0.5) / n
      const x = x0 + (x1 - x0) * t, z = z0 + (z1 - z0) * t
      const s = alongX ? x : z
      if (gap && s > gap[0] && s < gap[1]) continue
      k.box(alongX ? len / n : 0.0046, WH, alongX ? 0.0046 : len / n, stripe(i), { p: [x, Y0, z] })
    }
  }
  run(-WX, WZ1, WX, WZ1, [-0.034, 0.034]) // east front, open at the porch
  run(-WX, WZ0, WX, WZ0)
  run(-WX, WZ0, -WX, WZ1, [-0.032, 0.03]) // south, broken by its gopuram
  run(WX, WZ0, WX, WZ1, [-0.034, 0.032]) // north
  // coping
  for (const [x, z, w, d] of [[0, WZ1, WX * 2 + 0.006, 0.0062], [0, WZ0, WX * 2 + 0.006, 0.0062], [-WX, (WZ0 + WZ1) / 2, 0.0062, WZ1 - WZ0], [WX, (WZ0 + WZ1) / 2, 0.0062, WZ1 - WZ0]] as const) k.box(w, 0.0018, d, 'gopuramDeep', { p: [x, Y0 + WH, z] })

  /* ---- mandapam halls: flat roofs inside the wall ---- */
  const roofTop = Y0 + WH + 0.004
  const hall = (x0: number, z0: number, x1: number, z1: number, h = roofTop) => {
    k.box(x1 - x0, h - Y0, z1 - z0, (_p, n) => (n.y > 0.5 ? { c: 'cream', c2: 'stone', t: 0.35 } : { c: 'kovilWhite', glow: 0.1 }), { p: [(x0 + x1) / 2, Y0, (z0 + z1) / 2] })
    k.box(x1 - x0 + 0.004, 0.0024, z1 - z0 + 0.004, 'kovilRed', { p: [(x0 + x1) / 2, h - 0.0036, (z0 + z1) / 2] })
  }
  hall(-0.05, -0.085, 0.105, 0.066)
  hall(-0.105, -0.085, -0.05, -0.02, roofTop - 0.004)
  // gilded vimana over the sanctum, and a smaller one over the Shanmugar shrine
  for (const [x, z, s] of [[0.02, -0.03, 1], [0.07, 0.01, 0.7]] as const) {
    k.at({ p: [x, roofTop + 0.0022, z], s }, () => {
      for (let i = 0; i < 4; i++) k.box(0.024 - i * 0.0046, 0.0052, 0.024 - i * 0.0046, { c: i % 2 ? 'gopuram' : 'gopuramLite', glow: 0.12 }, { p: [0, i * 0.0052, 0] })
      k.add(new THREE.SphereGeometry(0.0062, 10, 6, 0, Math.PI * 2, 0, Math.PI / 2), { c: 'gold', smooth: true, glow: 0.7, gloss: 0.7 }, { p: [0, 0.0208, 0], s: [1, 1.2, 1] })
      k.cone(0.001, 0.006, 5, { c: 'gold', glow: 0.9, gloss: 0.7 }, { p: [0, 0.028, 0] })
    })
  }
  // Shanmuha Theertham: the stepped holy pond in the southern court
  k.box(0.046, 0.0034, 0.07, (_p, n) => (n.y > 0.5 ? { c: 'stone' } : { c: 'kovilWhite' }), { p: [-0.079, Y0 - 0.001, 0.026] })
  k.plate(-0.097, -0.004, -0.061, 0.056, Y0 + 0.0028, 'cream')
  k.plate(-0.093, 0.0, -0.065, 0.052, Y0 + 0.0031, { c: 'water', gloss: 0.9 })

  /* ---- the three gopurams ---- */
  let top = 0
  // east: five storeys, behind the porch
  k.at({ p: [0, Y0, WZ1 - 0.006] }, () => {
    top = Math.max(top, gopuram(k, 0.058, 0.03, 5, 0.0158, 0.03))
  })
  // south (Shanmuha Raja) and north (Gubera Raja), nine storeys, facing outward
  k.at({ p: [-WX + 0.004, Y0, -0.001], r: [0, -Math.PI / 2, 0] }, () => {
    top = Math.max(top, gopuram(k, 0.062, 0.034, 9, 0.0158, 0.032))
  })
  k.at({ p: [WX - 0.004, Y0, -0.001], r: [0, Math.PI / 2, 0], s: 1.06 }, () => {
    top = Math.max(top, gopuram(k, 0.062, 0.034, 9, 0.0158, 0.032))
  })

  /* ---- the gilded entrance porch ---- */
  k.at({ p: [0, Y0, WZ1 + 0.022] }, () => {
    const arch = new THREE.Shape()
    arch.moveTo(-0.04, 0)
    arch.lineTo(-0.04, 0.02)
    arch.absellipse(0, 0.02, 0.04, 0.026, Math.PI, 0, true)
    arch.lineTo(0.04, 0)
    arch.lineTo(0.032, 0)
    arch.lineTo(0.032, 0.02)
    arch.absellipse(0, 0.02, 0.032, 0.019, 0, Math.PI, false)
    arch.lineTo(-0.032, 0)
    // three gilded ribs carry the vault forward
    for (const z of [-0.014, 0, 0.014]) k.slab(arch, 0.0046, { c: 'gopuramLite', c2: 'gold', t: 0.5, glow: 0.3, gloss: 0.4 }, { p: [0, 0, z] })
    // the vault skin between them, red beneath
    const skin = new THREE.Shape()
    skin.moveTo(-0.037, 0.02)
    skin.absellipse(0, 0.02, 0.037, 0.0235, Math.PI, 0, true)
    skin.lineTo(0.034, 0.02)
    skin.absellipse(0, 0.02, 0.034, 0.0208, 0, Math.PI, false)
    k.slab(skin, 0.03, (_p, n) => (n.y > 0.1 ? { c: 'gopuram' } : { c: 'kovilRed', glow: 0.25 }))
    // pediment crest and kalasams
    k.box(0.02, 0.0052, 0.004, 'gopuramDeep', { p: [0, 0.0452, 0.0146] })
    for (const x of [-0.006, 0, 0.006]) k.cone(0.001, 0.0052, 5, { c: 'gold', glow: 0.9, gloss: 0.7 }, { p: [x, 0.0504, 0.0146] })
    for (const sx of [-1, 1]) for (const z of [-0.014, 0.014]) k.box(0.005, 0.0022, 0.0062, 'gopuramDeep', { p: [sx * 0.036, 0, z] })
    // lamps under the vault
    for (const x of [-0.02, 0, 0.02]) k.add(new THREE.OctahedronGeometry(0.0017, 0), { c: 'light', glow: 2, phase: 0.2 + x * 9 }, { p: [x, 0.03, 0.004] })
  })

  bellTower(k, -WX + 0.012, WZ1 - 0.002)
  bellTower(k, WX - 0.012, WZ1 - 0.002)

  /* ---- palmyras, a chariot shed, lamps ---- */
  k.drape = 1
  const spots: [number, number][] = [[-0.16, 0.1], [-0.175, 0.03], [-0.165, -0.06], [0.16, 0.11], [0.178, 0.02], [0.168, -0.08], [-0.09, 0.14], [0.1, 0.145], [-0.06, -0.145], [0.05, -0.15], [0.14, -0.13], [-0.14, -0.125]]
  spots.forEach(([x, z], i) => palmyra(k, x + (rand() - 0.5) * 0.02, z + (rand() - 0.5) * 0.02, 0.05 + rand() * 0.022, i))
  k.drape = 0
  for (const [x, z] of [[-0.06, 0.1], [0.06, 0.1], [-0.1, 0.092], [0.1, 0.092]] as const) {
    k.cyl(0.0006, 0.0006, 0.011, 4, 'timber', { p: [x, Y0, z] })
    k.add(new THREE.OctahedronGeometry(0.0017, 0), { c: 'light', glow: 1.7, phase: 0.3 + x * 4 }, { p: [x, Y0 + 0.012, z] })
  }
  return { body: k.build() }
}
