// Sri Pada (Adam's Peak), 2,243 m, Ratnapura / Nuwara Eliya border.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - a lone, steep, conical peak: a sharp pyramidal summit cone standing clear of
//     forested shoulders (Peak Wilderness); nothing of comparable height nearby
//   - flanks are dense montane forest, bare gneiss shows on the summit cone
//   - the summit is a small walled platform: the footprint shrine under its roof,
//     a Saman shrine, a bell
//   - the Hatton–Nallathanni pilgrim stair zig-zags up the cone and is lit by
//     lamps all night through the season; from afar it reads as a string of lights
//   - a white Japanese Peace Pagoda stands on the lower trail
//
// The peak is the tallest thing on the island model. Its foot follows the real
// terrain (drape weight rises toward the rim) so it grows out of the hill country
// instead of sitting on it.
import * as THREE from 'three'
import { Kit, clamp, fbm, hash2, hipRoof, rng, smooth, tree, type Paint } from '../kit'
import type { Built } from '../types'

const R = 0.21
const HM = 0.275
const SPURS = [0.5, 1.75, 2.85, 4.0, 5.25]

function spur(th: number) {
  let m = 0
  for (const a of SPURS) m = Math.max(m, Math.pow(Math.max(0, Math.cos(th - a)), 5))
  return m
}
/** mountain height above the model's ground plane */
function height(x: number, z: number) {
  const r = Math.hypot(x, z)
  const th = Math.atan2(z, x)
  const sp = spur(th + r * 2.2) // spurs curl a little as they fall
  // a few metres of level rock at the very top for the shrine, then the cone
  const rho = Math.max(0, r / (R * (0.8 + 0.2 * sp)) - 0.04) / 0.96
  if (rho >= 1) return -0.004
  // a needle of a summit on broad shoulders
  const cone = 0.3 * Math.pow(1 - rho, 2.7) + 0.7 * Math.pow(1 - rho, 1.18)
  const rib = 1 + (sp - 0.42) * 0.5 * smooth(0.07, 0.4, rho)
  const rough = (fbm(x * 21 + 3, z * 21, 3) - 0.5) * 0.02 * smooth(0.08, 0.4, rho)
  const h = HM * cone * rib + rough
  // flat enough on top for the shrine; tucked just under the land at the rim
  return Math.min(h, HM - 0.004) * smooth(1, 0.86, rho) - 0.004 * (1 - smooth(1, 0.86, rho))
}

export function buildSriPada(): Built {
  const k = new Kit()
  const rand = rng(11)

  // rigid at the summit, part of the land at the foot
  k.drape = (p) => smooth(R * 0.42, R * 0.98, Math.hypot(p.x, p.z))

  const rock = (p: THREE.Vector3, n: THREE.Vector3): Paint => {
    const e = clamp(p.y / HM)
    const steep = 1 - smooth(0.45, 0.8, n.y)
    const bare = clamp(Math.max(smooth(0.6, 0.86, e), steep * smooth(0.4, 0.62, e) * 0.8) + (fbm(p.x * 30, p.z * 30, 2) - 0.5) * 0.3)
    const leaf = fbm(p.x * 26 + 7, p.z * 26, 2)
    return {
      c: 'foliageDeep',
      c2: 'foliage',
      t: smooth(0.35, 0.7, leaf),
      m: [
        ['tea', smooth(0.14, 0.02, e) * 0.5],
        ['plain', smooth(0.05, -0.01, e)],
        ['peakRock', bare],
        ['peakRockLite', smooth(0.74, 0.97, e) * 0.75],
      ],
      face: true,
    }
  }
  k.surface(
    56, 22,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      // rings bunch toward the summit where the shape changes fastest
      const r = R * 1.02 * Math.pow(1 - v, 1.35)
      const x = Math.cos(th) * r, z = Math.sin(th) * r
      out.set(x, height(x, z), z)
    },
    rock,
    { closeU: true },
  )

  /* ---- forest on the lower flanks ---- */
  for (let i = 0, placed = 0; i < 900 && placed < 230; i++) {
    const a = rand() * Math.PI * 2, r = R * (0.32 + rand() * 0.6)
    const x = Math.cos(a) * r, z = Math.sin(a) * r
    const y = height(x, z)
    if (y < 0.006 || y > HM * 0.62) continue
    const s = 0.0052 + rand() * 0.0048
    const pick = hash2(x * 211, z * 97)
    tree(k, x, z, s, s * 1.8, pick > 0.6 ? 'foliageDeep' : pick > 0.25 ? 'foliage' : 'foliageLite', i, y - s * 0.4)
    placed++
  }

  /* ---- the pilgrim stair: a lamp-lit zig-zag up the front of the cone ---- */
  const N = 120
  const pts: THREE.Vector3[] = []
  for (let i = 0; i <= N; i++) {
    const t = i / N
    const r = R * (0.74 * Math.pow(1 - t, 1.25) + 0.012)
    // triangle wave: switchbacks that tighten as the cone narrows
    const w = Math.abs(((t * 6.5) % 2) - 1) * 2 - 1
    const th = Math.PI / 2 + 0.15 + w * (0.5 + 0.35 * (1 - t))
    const x = Math.cos(th) * r, z = Math.sin(th) * r
    pts.push(new THREE.Vector3(x, height(x, z) + 0.0022, z))
  }
  const W = 0.0026
  for (let i = 0; i < N; i++) {
    const a = pts[i], b = pts[i + 1]
    const dx = b.x - a.x, dz = b.z - a.z
    const L = Math.hypot(dx, dz) || 1
    const rx = (-dz / L) * W, rz = (dx / L) * W
    k.quad([a.x - rx, a.y, a.z - rz], [a.x + rx, a.y, a.z + rz], [b.x + rx, b.y, b.z + rz], [b.x - rx, b.y, b.z - rz], { c: 'path', c2: 'peakRockLite', t: 0.3 })
  }
  // lamps: pale by day, the only warm thing on the mountain by night. Big enough to hold a pixel at hero scale.
  for (let i = 2; i <= N; i += 3) {
    const p = pts[i]
    k.rigidAt(p.x, p.z, () => k.add(new THREE.OctahedronGeometry(0.0038, 0), { c: 'peakRockLite', c2: 'path', t: 0.5, glow: 2.4, phase: 0.02 + (i / N) * 0.9 }, { p: [p.x, p.y + 0.0044, p.z] }))
  }

  /* ---- the summit: walled platform, footprint shrine, Saman shrine, bell ---- */
  k.drape = 0
  const top = HM - 0.006
  // rock shoulder under a low whitewashed retaining wall
  k.cyl(0.0112, 0.0135, 0.005, 7, { c: 'peakRock', c2: 'peakRockLite', t: 0.6 }, { p: [0, top - 0.002, 0], r: [0, 0.4, 0] })
  k.cyl(0.0115, 0.0115, 0.0026, 7, (_p, n) => (n.y > 0.5 ? { c: 'stone' } : { c: 'white' }), { p: [0, top + 0.003, 0], r: [0, 0.4, 0] })
  // the footprint shrine and the Saman shrine beside it
  k.box(0.0105, 0.0068, 0.0082, { c: 'white', glow: 0.25 }, { p: [-0.0015, top + 0.0056, -0.0012] })
  hipRoof(k, 0.0138, 0.0112, 0.0048, 0.005, 'roof', { p: [-0.0015, top + 0.0124, -0.0012] })
  k.box(0.0052, 0.0052, 0.0052, { c: 'white', glow: 0.2 }, { p: [0.0072, top + 0.0056, 0.0046] })
  hipRoof(k, 0.0072, 0.0072, 0.0034, 0, 'roof', { p: [0.0072, top + 0.0108, 0.0046] })
  k.cyl(0.0006, 0.0006, 0.016, 4, 'white', { p: [-0.008, top + 0.0056, 0.006] })
  k.add(new THREE.OctahedronGeometry(0.003, 0), { c: 'light', glow: 2.2, phase: 0.31 }, { p: [-0.0015, top + 0.021, -0.0015] })

  /* ---- Peace Pagoda and the trailhead at Nallathanni ---- */
  k.drape = 1
  const foot = pts[4]
  k.rigidAt(foot.x + 0.02, foot.z + 0.006, () =>
    k.at({ p: [foot.x + 0.02, foot.y - 0.003, foot.z + 0.006] }, () => {
      k.cyl(0.011, 0.012, 0.004, 10, 'white')
      k.lathe([[0.0085, 0.004], [0.0085, 0.008], [0.0072, 0.012], [0.0042, 0.0152], [0.0018, 0.0166], [0.0015, 0.021], [0.0004, 0.026]], 10, { c: 'white', smooth: true, glow: 0.12 })
    }),
  )
  for (const [dx, dz, ry] of [[-0.022, 0.012, 0.3], [-0.034, 0.02, -0.4], [-0.012, 0.026, 0.9]] as const) {
    const x = foot.x + dx, z = foot.z + dz
    k.rigidAt(x, z, () =>
      k.at({ p: [x, height(x, z) + 0.0005, z], r: [0, ry, 0] }, () => {
        k.box(0.011, 0.005, 0.008, { c: 'cream', glow: 0.2 })
        hipRoof(k, 0.013, 0.01, 0.0035, 0.006, 'roof', { p: [0, 0.005, 0] })
      }),
    )
  }

  return { body: k.build() }
}
