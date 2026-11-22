// Nine Arch Bridge, Demodara (between Ella and Demodara stations), 1919–21.
//
// Reference (Wikipedia + Commons, checked 2026-11):
//   - masonry viaduct of stone, brick and cement, no steel: 91 m long, 24 m high,
//     7.6 m wide, exactly nine semicircular arches
//   - built on a nine-degree railway curve (radius ≈ 194 m), so the deck sweeps
//     through ≈27° end to end
//   - tall slender piers with a slight batter; arch rings, a string course under
//     the parapet; weathered grey-brown stone with dark stains
//   - spans a jungle ravine between tea-covered hills; the line dives into a
//     tunnel just beyond the bridge
//   - the train people photograph here is a blue Class S12 set
//
// Authored straight in (s, y, w) = (along the deck, up, across the deck) and then
// bent onto the curve, so the arches stay true semicircles. Height is exaggerated
// ≈1.15× against length. The train is one mesh per car: each car is a rigid
// rotation about the centre of the curve and hides once it is inside a tunnel.
import * as THREE from 'three'
import { Kit, clamp, fbm, hash2, rng, smooth, tree, type Paint } from '../kit'
import type { Built, Mover } from '../types'

const L = 0.33 // deck length along the curve
const ARC = 0.5 // radians swept (27° real, 29° here)
const R = L / ARC
const DECK = 0.1 // rail level above the valley floor
const WD = 0.025 // deck width
const N = 9
const ABUT = 0.012
const PIER = 0.0086
const SPAN = (L - 2 * ABUT - (N + 1) * PIER) / N
const RAD = SPAN / 2
const SPRING = DECK - 0.0165 - RAD
const HOOD = 0.036 // tunnel mouth length
const HOOD_AT = L / 2 + 0.012

/** bend straight (s, y, w) coordinates onto the curve; the bulge faces +z */
function bend<T extends THREE.BufferGeometry>(g: T): T {
  const p = g.attributes.position
  for (let i = 0; i < p.count; i++) {
    const a = p.getX(i) / R, w = p.getZ(i)
    p.setXYZ(i, (R + w) * Math.sin(a), p.getY(i), (R + w) * Math.cos(a) - R)
  }
  return g
}
const onCurve = (s: number, w: number): [number, number] => [(R + w) * Math.sin(s / R), (R + w) * Math.cos(s / R) - R]
/** model x, z → (s, w) */
const toSW = (x: number, z: number): [number, number] => [Math.atan2(x, z + R) * R, Math.hypot(x, z + R) - R]

/** the ravine and the two hills it cuts between, in model space */
function ground(x: number, z: number) {
  const [s, w] = toSW(x, z)
  const e = Math.abs(s) / (L / 2)
  // knolls, not walls: both hills are highest where the line meets them and fall away up- and downstream
  const near = Math.exp(-((w / 0.105) ** 2))
  // ravine sides climb to rail level at the abutments…
  let h = DECK * 0.985 * Math.pow(Math.min(e, 1), 1.45) * (0.5 + 0.5 * near)
  // …and the hills carry on above the line
  h += 0.05 * smooth(L / 2 - 0.004, L / 2 + 0.05, Math.abs(s)) * near
  // the valley climbs upstream (behind the bridge) and opens downstream
  h += 0.024 * smooth(0.0, -0.15, w) * (1 - Math.min(e, 1) * 0.4)
  h += (fbm(x * 16 + 2, z * 16, 3) - 0.5) * 0.018 * smooth(0.1, 0.5, e)
  return h
}
const RX = 0.33, RZ = 0.19, CZ = -0.012
const rimFade = (x: number, z: number) => 1 - smooth(0.55, 1, Math.hypot(x / RX, (z - CZ) / RZ))
const surfaceY = (x: number, z: number) => ground(x, z) * rimFade(x, z)

function carGeometry(lead: -1 | 0 | 1) {
  const k = new Kit()
  const c = 0.029, h = 0.0098, wd = 0.0108, y0 = DECK + 0.0034
  const add = (g: THREE.BufferGeometry, paint: Paint) => k.add(bend(g), paint)
  // underframe and bogies
  add(new THREE.BoxGeometry(c * 0.92, 0.0022, wd * 0.8, 2).translate(0, y0 - 0.0011, 0), { c: 'iron' })
  // body: deep blue skirt, blue sides, a pale window band that lights up at night
  add(new THREE.BoxGeometry(c, h * 0.36, wd, 2).translate(0, y0 + h * 0.18, 0), { c: 'trainBlueDeep' })
  add(new THREE.BoxGeometry(c, h * 0.34, wd * 1.005, 2).translate(0, y0 + h * 0.53, 0), { c: 'trainStripe', glow: 0.5 })
  add(new THREE.BoxGeometry(c, h * 0.3, wd, 2).translate(0, y0 + h * 0.85, 0), { c: 'trainBlue' })
  // window mullions break the band into panes
  for (let i = -3; i <= 3; i++) add(new THREE.BoxGeometry(0.001, h * 0.36, wd * 1.012).translate(i * c * 0.135, y0 + h * 0.53, 0), { c: 'trainBlue' })
  // roof
  add(new THREE.BoxGeometry(c * 0.97, 0.0016, wd * 0.8, 2).translate(0, y0 + h + 0.0008, 0), { c: 'trainRoof' })
  if (lead) {
    // driving cab: raked nose, dark windscreen, headlamp
    const x = (lead * c) / 2
    const nose = new THREE.Shape([new THREE.Vector2(0, 0), new THREE.Vector2(lead * 0.0042, 0), new THREE.Vector2(lead * 0.0018, h), new THREE.Vector2(0, h)])
    add(new THREE.ExtrudeGeometry(nose, { depth: wd, bevelEnabled: false }).translate(x, y0, -wd / 2), { c: 'trainBlue' })
    add(new THREE.BoxGeometry(0.0006, h * 0.3, wd * 0.8).translate(x + lead * 0.0031, y0 + h * 0.66, 0), { c: 'iron' })
    add(new THREE.BoxGeometry(0.0006, h * 0.16, wd * 0.5).translate(x + lead * 0.0041, y0 + h * 0.2, 0), { c: 'light', glow: 2.2 })
  }
  return k.build()
}

export function buildNineArch(): Built {
  const k = new Kit()
  const rand = rng(23)

  /* ---- terrain: ravine, hills, apron ---- */
  // rigid under the bridge and through both tunnels (so the hills always bury them), part of the land at the rim
  k.drape = (p) => smooth(0.66, 1, Math.hypot(p.x / RX, (p.z - CZ) / RZ))
  const earth = (p: THREE.Vector3, n: THREE.Vector3): Paint => {
    const [s, w] = toSW(p.x, p.z)
    const teaSide = smooth(-0.02, 0.05, w) * smooth(0.02, 0.07, Math.abs(s))
    // tea is planted in contour rows: stripe by height
    const rows = 0.5 + 0.5 * Math.sin(p.y * 520)
    const leaf = fbm(p.x * 30, p.z * 30, 2)
    return {
      c: 'foliageDeep',
      c2: 'foliage',
      t: smooth(0.3, 0.7, leaf),
      m: [
        ['tea', teaSide * (0.55 + 0.35 * rows)],
        ['viaductDark', (1 - smooth(0.5, 0.8, n.y)) * 0.5],
        ['ground', 1 - smooth(0.0, 0.25, rimFade(p.x, p.z))],
      ],
      k: 0.86 + 0.14 * smooth(0, 0.07, p.y),
      face: true,
    }
  }
  k.surface(
    64, 16,
    (u, v, out) => {
      const th = -u * Math.PI * 2
      const r = 1 - v
      const x = Math.cos(th) * RX * r, z = CZ + Math.sin(th) * RZ * r
      // the rim dips under the land so the diorama has no edge
      out.set(x, 0.0015 + surfaceY(x, z) - (v === 0 ? 0.006 : 0), z)
    },
    earth,
    { closeU: true },
  )

  /* ---- the stream on the valley floor ---- */
  const streamAt = (w: number): [number, number] => onCurve(0.006 * Math.sin(w * 42) + 0.01 * w, w)
  for (let i = 0; i < 18; i++) {
    const w0 = -0.12 + i * 0.014, w1 = w0 + 0.014
    const [x0, z0] = streamAt(w0), [x1, z1] = streamAt(w1)
    const hw = 0.0045 + 0.002 * smooth(-0.1, 0.12, w0)
    const y0 = surfaceY(x0, z0) + 0.0032, y1 = surfaceY(x1, z1) + 0.0032
    k.quad([x0 + hw, y0, z0], [x0 - hw, y0, z0], [x1 - hw, y1, z1], [x1 + hw, y1, z1], { c: 'water', gloss: 0.85 })
  }
  for (let i = 0; i < 10; i++) {
    const w = -0.1 + rand() * 0.2
    const [x, z] = streamAt(w)
    const dx = (rand() < 0.5 ? -1 : 1) * (0.007 + rand() * 0.006), s = 0.003 + rand() * 0.003
    k.ball(1, 0, { c: 'stone', c2: 'viaductDark', t: rand() * 0.6 }, { p: [x + dx, surfaceY(x + dx, z) + s * 0.4, z], r: [rand() * 3, rand() * 3, 0], s: [s * 1.3, s * 0.8, s] })
  }

  /* ---- vegetation ---- */
  for (let i = 0, placed = 0; i < 900 && placed < 150; i++) {
    const a = rand() * Math.PI * 2, r = Math.sqrt(rand()) * 0.93
    const x = Math.cos(a) * RX * r, z = CZ + Math.sin(a) * RZ * r
    const [s, w] = toSW(x, z)
    // keep the line, the piers and the stream clear
    if (Math.abs(w) < 0.026 || Math.abs(s) < 0.016) continue
    const y = surfaceY(x, z)
    const front = w > 0
    const sz = 0.006 + rand() * 0.006
    if (front && Math.abs(s) > 0.03 && rand() < 0.7) {
      // tea: low clipped bushes
      k.rigidAt(x, z, () => k.ball(1, 0, { c: 'tea', c2: 'foliageLite', t: rand() * 0.5 }, { p: [x, y + sz * 0.3, z], r: [0, rand() * 3, 0], s: [sz * 1.1, sz * 0.5, sz * 1.1] }))
    } else if (y > DECK * 0.9 && rand() < 0.3) {
      // the conifers on the hilltops: two stacked cones
      k.rigidAt(x, z, () => {
        const leaf: Paint = { c: 'foliageDeep', c2: 'foliage', t: rand() * 0.5 }
        k.cone(sz * 0.62, sz * 1.5, 6, leaf, { p: [x, y - 0.001, z] })
        k.cone(sz * 0.44, sz * 1.5, 6, leaf, { p: [x, y - 0.001 + sz * 0.9, z] })
      })
    } else {
      const pick = hash2(x * 173, z * 91)
      tree(k, x, z, sz * 1.15, sz * 2.1, pick > 0.6 ? 'foliageDeep' : pick > 0.25 ? 'foliage' : 'foliageLite', i, y - sz * 0.4)
    }
    placed++
  }
  k.drape = 0

  /* ---- the viaduct ---- */
  const YB = -0.03
  const outline: THREE.Vector2[] = [new THREE.Vector2(-L / 2, YB)]
  for (let i = 0; i < N; i++) {
    const s0 = -L / 2 + ABUT + PIER + i * (SPAN + PIER)
    outline.push(new THREE.Vector2(s0 + 0.0024, YB), new THREE.Vector2(s0, SPRING))
    for (let j = 1; j < 12; j++) {
      const a = Math.PI - (j / 12) * Math.PI
      outline.push(new THREE.Vector2(s0 + RAD + Math.cos(a) * RAD, SPRING + Math.sin(a) * RAD))
    }
    outline.push(new THREE.Vector2(s0 + SPAN, SPRING), new THREE.Vector2(s0 + SPAN - 0.0024, YB))
  }
  outline.push(new THREE.Vector2(L / 2, YB))
  for (let i = 0; i <= 40; i++) outline.push(new THREE.Vector2(L / 2 - (i / 40) * L, DECK - 0.0035))
  const stonePaint = (p: THREE.Vector3, n: THREE.Vector3): Paint => {
    const [s] = toSW(p.x, p.z)
    const stain = fbm(s * 60 + 5, p.y * 38, 3)
    return {
      c: 'viaduct',
      c2: 'viaductDark',
      // soffits and pier flanks sit in shadow; rain streaks run down from the string course
      t: clamp((n.y < -0.25 ? 0.6 : 0) + smooth(0.45, 0.75, stain) * 0.55 + smooth(0.03, -0.02, p.y) * 0.3),
      k: 0.82 + 0.18 * smooth(-0.01, 0.06, p.y),
    }
  }
  k.add(bend(new THREE.ExtrudeGeometry(new THREE.Shape(outline), { depth: WD - 0.003, bevelEnabled: false }).translate(0, 0, -(WD - 0.003) / 2)), stonePaint)
  // arch rings, proud of the spandrel on both faces
  for (let i = 0; i < N; i++) {
    const cx = -L / 2 + ABUT + PIER + i * (SPAN + PIER) + RAD
    const ring = new THREE.Shape()
    ring.absarc(0, 0, RAD + 0.0024, 0, Math.PI, false)
    ring.absarc(0, 0, RAD, Math.PI, 0, true)
    for (const side of [-1, 1])
      k.add(bend(new THREE.ExtrudeGeometry(ring, { depth: 0.0012, bevelEnabled: false, curveSegments: 9 }).translate(cx, SPRING, side * ((WD - 0.003) / 2 + 0.0004) - 0.0006)), { c: 'viaduct', c2: 'white', t: 0.25 })
    // pier impost
    if (i < N - 1) {
      const px = cx + RAD + PIER / 2
      k.add(bend(new THREE.BoxGeometry(PIER + 0.0022, 0.0022, WD - 0.001).translate(px, SPRING - 0.0011, 0)), { c: 'viaduct', c2: 'white', t: 0.2 })
    }
  }
  // string course, deck, parapets
  k.add(bend(new THREE.BoxGeometry(L, 0.0024, WD, 44).translate(0, DECK - 0.0035 + 0.0012, 0)), { c: 'viaduct', c2: 'white', t: 0.3 })
  for (const side of [-1, 1]) k.add(bend(new THREE.BoxGeometry(L, 0.0048, 0.0026, 44).translate(0, DECK - 0.0011 + 0.0024, side * (WD / 2 - 0.0016))), stonePaint)
  // ballast and rails, carried on into both tunnels
  const TRACK = L + 2 * (HOOD + 0.012)
  k.add(bend(new THREE.BoxGeometry(TRACK, 0.0014, WD - 0.0064, 60).translate(0, DECK - 0.0011 + 0.0007, 0)), { c: 'ballast' })
  for (const side of [-1, 1]) k.add(bend(new THREE.BoxGeometry(TRACK, 0.0009, 0.0011, 60).translate(0, DECK + 0.0008, side * 0.0036)), { c: 'iron' })

  /* ---- tunnel mouths ---- */
  for (const side of [-1, 1]) {
    const c = side * (HOOD_AT + HOOD / 2)
    const hw = 0.0105, hh = 0.019
    const stone: Paint = { c: 'viaductDark', c2: 'viaduct', t: 0.35 }
    // walls and crown, open toward the bridge
    k.add(bend(new THREE.BoxGeometry(HOOD, hh, 0.0034, 6).translate(c, DECK - 0.002 + hh / 2, hw + 0.0017)), stone)
    k.add(bend(new THREE.BoxGeometry(HOOD, hh, 0.0034, 6).translate(c, DECK - 0.002 + hh / 2, -hw - 0.0017)), stone)
    k.add(bend(new THREE.BoxGeometry(HOOD, 0.0036, hw * 2 + 0.0068, 6).translate(c, DECK - 0.002 + hh + 0.0018, 0)), stone)
    // portal face: dressed stone arch with the dark of the bore behind it
    const face = new THREE.Shape([new THREE.Vector2(-hw - 0.0052, 0), new THREE.Vector2(hw + 0.0052, 0), new THREE.Vector2(hw + 0.0052, hh + 0.0062), new THREE.Vector2(-hw - 0.0052, hh + 0.0062)])
    const bore = new THREE.Path()
    bore.moveTo(-hw + 0.0017, 0.0006)
    bore.lineTo(-hw + 0.0017, 0.0085)
    bore.absarc(0, 0.0085, hw - 0.0017, Math.PI, 0, true)
    bore.lineTo(hw - 0.0017, 0.0006)
    face.holes.push(bore)
    const fg = new THREE.ExtrudeGeometry(face, { depth: 0.003, bevelEnabled: false, curveSegments: 8 }).rotateY(Math.PI / 2).translate(side * HOOD_AT - (side > 0 ? 0.003 : 0), DECK - 0.002, 0)
    k.add(bend(fg), { c: 'viaduct', c2: 'white', t: 0.2 })
    // the far end is blind: nothing shows through
    k.add(bend(new THREE.BoxGeometry(0.002, hh, hw * 2).translate(side * (HOOD_AT + HOOD), DECK - 0.002 + hh / 2, 0)), { c: 'iron' })
  }

  /* ---- the S12, one mesh per car ---- */
  const CARS = 5, PITCH = 0.031
  const TRAIN = CARS * PITCH
  const SPEED = 0.046
  const TRAVEL = 2 * HOOD_AT + TRAIN + HOOD + 0.03
  const PERIOD = TRAVEL / SPEED + 3.5
  const movers: Mover[] = []
  for (let i = 0; i < CARS; i++) {
    const geometry = carGeometry(i === 0 ? 1 : i === CARS - 1 ? -1 : 0)
    geometry.translate(0, 0, R) // spin about the centre of the curve
    movers.push({
      geometry,
      update: (obj, t) => {
        // head starts buried in the left tunnel and runs to the right one, then waits out the rest of the period
        const head = -(HOOD_AT + HOOD + 0.01) + Math.min((t % PERIOD) * SPEED, TRAVEL)
        const s = head - PITCH / 2 - i * PITCH
        obj.position.set(0, 0, -R)
        obj.rotation.y = s / R
        const inside = Math.abs(s) - PITCH / 2
        obj.visible = inside < HOOD_AT + 0.004
      },
    })
  }

  return { body: k.build(), movers }
}
