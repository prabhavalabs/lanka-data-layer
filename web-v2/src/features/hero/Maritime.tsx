// Maritime overlay for the island: shipping lanes on the water with light pulses
// running along them, a small fleet under way, and a beacon at every port.
//
// Budget: one ribbon mesh for every lane, one instanced mesh per vessel type, one
// for all wakes, three for the beacons. The frame loop allocates nothing.
import { Html } from '@react-three/drei'
import { useFrame, type ThreeEvent } from '@react-three/fiber'
import { useEffect, useLayoutEffect, useMemo, useRef, type MutableRefObject, type ReactNode } from 'react'
import * as THREE from 'three'
import { FLEET, LANES, PORTS, SPEED, type LaneKind, type Port, type VesselKind } from './maritimeData'
import { K, ORIGIN, toXZ, type Terrain } from './terrain'

type XZ = [number, number]
type Lane = {
  id: string
  kind: LaneKind
  n: number
  x: Float32Array
  z: Float32Array
  /** unit tangent per sample */
  tx: Float32Array
  tz: Float32Array
  /** arc length at each sample */
  cum: Float32Array
  total: number
  fade: [number, number]
}

const KIND_INDEX: Record<LaneKind, number> = { trunk: 0, feeder: 1, coastal: 2 }
const HALF_WIDTH: Record<LaneKind, number> = { trunk: 0.034, feeder: 0.026, coastal: 0.022 }
const SEA_Y = 0.0034

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)))
  return t * t * (3 - 2 * t)
}

/* ---------------- lane geometry ---------------- */

/** Uniform Catmull-Rom through the waypoints, sampled roughly every `step` scene units. */
function spline(pts: XZ[], step: number): XZ[] {
  const out: XZ[] = []
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)]
    const n = Math.max(2, Math.ceil(Math.hypot(p2[0] - p1[0], p2[1] - p1[1]) / step))
    for (let s = 0; s < n; s++) {
      const t = s / n, t2 = t * t, t3 = t2 * t
      out.push([
        0.5 * (2 * p1[0] + (p2[0] - p0[0]) * t + (2 * p0[0] - 5 * p1[0] + 4 * p2[0] - p3[0]) * t2 + (3 * p1[0] - p0[0] - 3 * p2[0] + p3[0]) * t3),
        0.5 * (2 * p1[1] + (p2[1] - p0[1]) * t + (2 * p0[1] - 5 * p1[1] + 4 * p2[1] - p3[1]) * t2 + (3 * p1[1] - p0[1] - 3 * p2[1] + p3[1]) * t3),
      ])
    }
  }
  out.push(pts[pts.length - 1])
  return out
}

/** Walk any sample that sits on land, or closer to shore than `margin` degrees, down the distance field into open water. */
function keepAtSea(terrain: Terrain, pts: XZ[], margin: number) {
  const B = terrain.meta.bounds
  const E = 0.012
  for (const p of pts) {
    for (let it = 0; it < 8; it++) {
      const lon = p[0] / K + ORIGIN.lon, lat = ORIGIN.lat - p[1]
      if (lon < B.w + 0.03 || lon > B.e - 0.03 || lat < B.s + 0.03 || lat > B.n - 0.03) break
      const d = terrain.coastDistance(lon, lat) // positive inland
      if (d <= -margin) break
      const gx = (terrain.coastDistance(lon + E, lat) - terrain.coastDistance(lon - E, lat)) / K
      const gz = -(terrain.coastDistance(lon, lat + E) - terrain.coastDistance(lon, lat - E))
      const l = Math.hypot(gx, gz) || 1
      const move = (d + margin) * 1.05 + 0.004
      p[0] -= (gx / l) * move
      p[1] -= (gz / l) * move
    }
  }
}

function makeLane(terrain: Terrain, id: string, kind: LaneKind, xz: XZ[], step: number, margin: number, fade: [number, number] = [0, 0]): Lane {
  const pts = spline(xz, step)
  keepAtSea(terrain, pts, margin)
  // relax the kinks the push can leave behind, then make sure relaxing did not cut a corner over land
  for (let k = 0; k < 4; k++) {
    for (let i = 1; i < pts.length - 1; i++) {
      pts[i][0] = (pts[i - 1][0] + 2 * pts[i][0] + pts[i + 1][0]) / 4
      pts[i][1] = (pts[i - 1][1] + 2 * pts[i][1] + pts[i + 1][1]) / 4
    }
    keepAtSea(terrain, pts, margin)
  }
  const n = pts.length
  const x = new Float32Array(n), z = new Float32Array(n), tx = new Float32Array(n), tz = new Float32Array(n), cum = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    x[i] = pts[i][0]
    z[i] = pts[i][1]
    if (i) cum[i] = cum[i - 1] + Math.hypot(x[i] - x[i - 1], z[i] - z[i - 1])
  }
  for (let i = 0; i < n; i++) {
    const a = Math.max(0, i - 1), b = Math.min(n - 1, i + 1)
    const dx = x[b] - x[a], dz = z[b] - z[a], l = Math.hypot(dx, dz) || 1
    tx[i] = dx / l
    tz[i] = dz / l
  }
  return { id, kind, n, x, z, tx, tz, cum, total: cum[n - 1], fade }
}

type Vessel = { lane: number; kind: number; slot: number; s: number; speed: number; dir: 1 | -1; off: number; size: number; seg: number; phase: number }
type Beacon = { port: Port; x: number; z: number }

const VESSEL_KINDS: VesselKind[] = ['container', 'tanker', 'fishing']
const VESSEL_SIZE: Record<VesselKind, number> = { container: 0.09, tanker: 0.1, fishing: 0.034 }
/** the light each type shows at dusk */
const VESSEL_TINT: Record<VesselKind, string> = { container: '#fff1d6', tanker: '#ffb37a', fishing: '#9dffc4' }

function buildMaritime(terrain: Terrain, lowPower: boolean) {
  const lanes: Lane[] = LANES.map((l) =>
    makeLane(terrain, l.id, l.kind, l.pts.map(([lon, lat]) => toXZ(lon, lat)), l.kind === 'trunk' ? 0.06 : 0.03, l.kind === 'trunk' ? 0.12 : 0.055, l.fade),
  )
  const coastal = lanes.find((l) => l.id === 'coastal')!

  // beacons stand in the harbour mouth; a short spur ties each one to the coasting route
  const beacons: Beacon[] = PORTS.map((port) => {
    const p: XZ[] = [toXZ(port.lon, port.lat)]
    keepAtSea(terrain, p, 0.016)
    return { port, x: p[0][0], z: p[0][1] }
  })
  for (const b of beacons) {
    let best = 0, bd = Infinity
    for (let i = 0; i < coastal.n; i++) {
      const d = Math.hypot(coastal.x[i] - b.x, coastal.z[i] - b.z)
      if (d < bd) { bd = d; best = i }
    }
    if (bd > 0.02) lanes.push(makeLane(terrain, `spur-${b.port.id}`, 'coastal', [[coastal.x[best], coastal.z[best]], [b.x, b.z]], 0.02, 0.014))
  }

  /* ---- one ribbon for every lane ---- */
  let verts = 0
  for (const l of lanes) verts += l.n * 2
  const position = new Float32Array(verts * 3), uv = new Float32Array(verts * 2), info = new Float32Array(verts * 3)
  const index: number[] = []
  let v = 0
  lanes.forEach((l, li) => {
    const hw = HALF_WIDTH[l.kind]
    for (let i = 0; i < l.n; i++) {
      const s = l.cum[i]
      const fade = (l.fade[0] ? smooth(0, l.fade[0], s) : 1) * (l.fade[1] ? 1 - smooth(l.total - l.fade[1], l.total, s) : 1)
      for (const side of [-1, 1]) {
        position[v * 3] = l.x[i] - l.tz[i] * hw * side
        position[v * 3 + 1] = SEA_Y
        position[v * 3 + 2] = l.z[i] + l.tx[i] * hw * side
        uv[v * 2] = s
        uv[v * 2 + 1] = side
        info[v * 3] = KIND_INDEX[l.kind]
        info[v * 3 + 1] = fade
        info[v * 3 + 2] = (li * 0.37) % 1
        if (side === 1 && i < l.n - 1) index.push(v - 1, v, v + 1, v, v + 2, v + 1)
        v++
      }
    }
  })
  const ribbon = new THREE.BufferGeometry()
  ribbon.setAttribute('position', new THREE.BufferAttribute(position, 3))
  ribbon.setAttribute('aUv', new THREE.BufferAttribute(uv, 2))
  ribbon.setAttribute('aInfo', new THREE.BufferAttribute(info, 3))
  ribbon.setIndex(index)

  /* ---- the fleet, seeded so every visit sees the same sea ---- */
  let seed = 20261118
  const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296)
  const vessels: Vessel[] = []
  const counts = [0, 0, 0]
  for (const f of FLEET) {
    const li = lanes.findIndex((l) => l.id === f.lane)
    const lane = lanes[li]
    const kind = VESSEL_KINDS.indexOf(f.kind)
    const count = lowPower ? Math.ceil(f.count / 2) : f.count
    for (let i = 0; i < count; i++) {
      const fishing = f.kind === 'fishing'
      vessels.push({
        lane: li,
        kind,
        slot: counts[kind]++,
        s: ((i + 0.15 + rnd() * 0.7) / count) * lane.total,
        speed: SPEED[f.kind] * (0.82 + rnd() * 0.36),
        dir: f.bothWays && rnd() > 0.5 ? -1 : 1,
        // fishing boats work the water to seaward of the route; ships hold close to it
        off: fishing ? 0.012 + rnd() * 0.05 : (rnd() - 0.5) * (lane.kind === 'trunk' ? 0.03 : 0.014),
        size: VESSEL_SIZE[f.kind] * (lane.kind === 'coastal' && !fishing ? 0.72 : 0.9 + rnd() * 0.24),
        seg: 0,
        phase: rnd() * 6.283,
      })
    }
  }
  const tint = new Float32Array(vessels.length * 3)
  const c = new THREE.Color()
  vessels.forEach((ve, i) => c.set(VESSEL_TINT[VESSEL_KINDS[ve.kind]]).toArray(tint, i * 3))

  return { lanes, beacons, ribbon, vessels, counts, tint }
}

/* ---------------- vessel models: a few flat-shaded solids each ---------------- */
function solid() {
  const pos: number[] = [], col: number[] = []
  const c = new THREE.Color()
  const tri = (a: number[], b: number[], d: number[]) => {
    pos.push(...a, ...b, ...d)
    col.push(c.r, c.g, c.b, c.r, c.g, c.b, c.r, c.g, c.b)
  }
  const api = {
    /** vertical prism over a convex plan given counter-clockwise in (x, z) */
    prism(plan: XZ[], y0: number, y1: number, color: string) {
      c.set(color)
      for (let i = 1; i < plan.length - 1; i++) tri([plan[0][0], y1, plan[0][1]], [plan[i + 1][0], y1, plan[i + 1][1]], [plan[i][0], y1, plan[i][1]])
      for (let i = 0; i < plan.length; i++) {
        const a = plan[i], b = plan[(i + 1) % plan.length]
        tri([a[0], y0, a[1]], [a[0], y1, a[1]], [b[0], y1, b[1]])
        tri([a[0], y0, a[1]], [b[0], y1, b[1]], [b[0], y0, b[1]])
      }
      return api
    },
    box(x0: number, x1: number, y0: number, y1: number, hw: number, color: string) {
      return api.prism([[x0, -hw], [x0, hw], [x1, hw], [x1, -hw]], y0, y1, color)
    },
    done() {
      const g = new THREE.BufferGeometry()
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3))
      g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3))
      g.computeVertexNormals()
      return g
    },
  }
  return api
}
const hullPlan = (stern: number, shoulder: number, bow: number, hw: number): XZ[] => [[stern, -hw * 0.82], [stern, hw * 0.82], [shoulder, hw], [bow, 0], [shoulder, -hw]]

function vesselGeometries() {
  // bow points down +x; one unit is the ship's length
  const container = solid()
    .prism(hullPlan(-0.5, 0.3, 0.5, 0.085), 0, 0.075, '#22344a')
    .box(-0.44, -0.33, 0.075, 0.215, 0.07, '#f3efe4')
    .box(-0.415, -0.385, 0.215, 0.26, 0.02, '#d8446b')
    .box(-0.28, -0.1, 0.075, 0.15, 0.072, '#d8446b')
    .box(-0.085, 0.095, 0.075, 0.165, 0.072, '#2b9aa0')
    .box(0.11, 0.29, 0.075, 0.135, 0.072, '#e8b84a')
    .done()
  const tanker = solid()
    .prism(hullPlan(-0.5, 0.34, 0.5, 0.092), 0, 0.06, '#6b2f27')
    .box(-0.32, 0.36, 0.06, 0.072, 0.078, '#8a949e')
    .box(-0.3, 0.34, 0.072, 0.086, 0.012, '#d9dee2')
    .box(-0.46, -0.35, 0.06, 0.19, 0.07, '#f3efe4')
    .box(-0.43, -0.4, 0.19, 0.235, 0.022, '#22344a')
    .done()
  const fishing = solid()
    .prism([[-0.5, 0], [-0.26, 0.17], [0.16, 0.17], [0.5, 0], [0.16, -0.17], [-0.26, -0.17]], 0, 0.11, '#2f7fbf')
    .box(-0.22, 0.04, 0.11, 0.25, 0.1, '#f3efe4')
    .box(-0.2, 0.02, 0.25, 0.275, 0.115, '#e2733c')
    .done()
  return [container, tanker, fishing]
}

/* ---------------- shaders ---------------- */
const HAZE = /* glsl */ `
  float seaHaze(vec3 world) {
    float dist = length(cameraPosition - world);
    return exp(-pow(dist * 0.05, 2.2));
  }`

const laneVertex = /* glsl */ `
  attribute vec2 aUv;
  attribute vec3 aInfo;
  varying vec2 vUv;
  varying vec3 vInfo;
  varying vec3 vWorld;
  void main() {
    vUv = aUv;
    vInfo = aInfo;
    vec4 w = modelMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
  }`
const laneFragment = /* glsl */ `
  uniform float uTime, uShow, uOpacity;
  uniform vec3 uColor, uPulse;
  varying vec2 vUv;
  varying vec3 vInfo;
  varying vec3 vWorld;
  ${HAZE}
  void main() {
    float a = abs(vUv.y);
    float fw = fwidth(vUv.y);
    float trunk = 1.0 - step(0.5, vInfo.x);
    float coastal = step(1.5, vInfo.x);
    // a crisp core that never drops below a pixel, inside a soft halo
    float coreW = 0.13;
    float core = 1.0 - smoothstep(max(coreW - fw, 0.0), coreW + fw, a);
    float halo = exp(-a * a * 6.5) * (1.0 - smoothstep(0.75, 1.0, a));
    // coasting routes are dashed, the way a chart draws a recommended track
    float dash = mix(1.0, smoothstep(0.2, 0.26, abs(fract(vUv.x * 9.0) - 0.5)), coastal);
    float base = mix(0.5, 0.78, trunk) * mix(1.0, 0.82, coastal);
    // light pulses: a bright head with a tail trailing behind it, travelling with the traffic
    float speed = mix(0.2, 0.3, trunk);
    float f = fract(vUv.x * mix(0.62, 0.4, trunk) - uTime * speed * mix(0.62, 0.4, trunk) + vInfo.z);
    float pulse = pow(f, 11.0) * (1.0 - smoothstep(0.975, 1.0, f));
    float alpha = core * base * dash + halo * (0.16 + 0.1 * trunk) + pulse * (core * 0.9 + halo * 0.85);
    vec3 c = mix(uColor, uPulse, clamp(pulse * 1.4, 0.0, 1.0));
    gl_FragColor = vec4(c, alpha * vInfo.y * uOpacity * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

const wakeVertex = /* glsl */ `
  attribute vec3 aTint;
  varying vec2 vUv;
  varying vec3 vTint;
  varying vec3 vWorld;
  void main() {
    vUv = uv;
    vTint = aTint;
    vec4 w = modelMatrix * instanceMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
  }`
const wakeFragment = /* glsl */ `
  uniform float uShow, uWake, uGlow;
  uniform vec3 uFoam;
  varying vec2 vUv;
  varying vec3 vTint;
  varying vec3 vWorld;
  ${HAZE}
  void main() {
    // u: 0 far astern → 1 ahead of the bow; the hull sits around u = 0.86
    float along = vUv.x;
    float across = abs(vUv.y - 0.5) * 2.0;
    float astern = clamp(along / 0.8, 0.0, 1.0);
    float width = mix(0.95, 0.2, astern);
    float inside = 1.0 - smoothstep(width * 0.55, width, across);
    float arms = smoothstep(width * 0.38, width * 0.7, across) * inside;
    float wake = (inside * 0.3 + arms * 0.7) * astern * astern * (1.0 - smoothstep(0.78, 0.84, along));
    // the pool of light a ship throws on the water at dusk
    vec2 q = vec2((along - 0.86) * 2.6, (vUv.y - 0.5) * 1.2);
    float glow = exp(-dot(q, q) * 12.0);
    float alpha = wake * uWake + glow * uGlow;
    vec3 c = mix(uFoam, vTint, clamp(glow * uGlow * 1.6, 0.0, 1.0));
    gl_FragColor = vec4(c, alpha * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

const ringVertex = /* glsl */ `
  attribute float aIdx;
  varying vec2 vUv;
  varying float vIdx;
  varying vec3 vWorld;
  void main() {
    vUv = uv;
    vIdx = aIdx;
    vec4 w = modelMatrix * instanceMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
  }`
const ringFragment = /* glsl */ `
  uniform float uTime, uShow, uActive;
  uniform vec3 uColor;
  varying vec2 vUv;
  varying float vIdx;
  varying vec3 vWorld;
  ${HAZE}
  float ring(float d, float ph) {
    float w = 0.05 + fwidth(d);
    return (1.0 - smoothstep(0.0, w, abs(d - ph * 0.92))) * (1.0 - ph) * (1.0 - ph);
  }
  void main() {
    float d = length(vUv - 0.5) * 2.0;
    float ph = fract(uTime * 0.3 + vIdx * 0.37);
    float act = 1.0 - step(0.5, abs(vIdx - uActive));
    float rings = ring(d, ph) + ring(d, fract(ph + 0.5));
    float pool = exp(-d * d * 30.0) * 0.7;
    float alpha = rings * (0.55 + 0.45 * act) + pool + act * exp(-d * d * 5.0) * 0.28;
    gl_FragColor = vec4(uColor, alpha * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

const _c = new THREE.Color()
const mixCol = (out: THREE.Color, a: string, b: string, t: number) => out.set(a).lerp(_c.set(b), t)

export function Maritime({
  terrain, mix, show, animate, lowPower, activePort, onPort, renderPortCard,
}: {
  terrain: Terrain
  /** 0 = dusk, 1 = day */
  mix: MutableRefObject<number>
  show: boolean
  animate: boolean
  lowPower: boolean
  activePort: string | null
  onPort: (id: string | null) => void
  renderPortCard: (port: Port) => ReactNode
}) {
  const built = useMemo(() => buildMaritime(terrain, lowPower), [terrain, lowPower])
  const hulls = useMemo(vesselGeometries, [])
  const wakeGeo = useMemo(() => {
    const g = new THREE.PlaneGeometry(5.2, 1.2)
    g.rotateX(-Math.PI / 2)
    g.translate(-1.87, 0, 0)
    g.setAttribute('aTint', new THREE.InstancedBufferAttribute(built.tint, 3))
    return g
  }, [built])
  const ringGeo = useMemo(() => {
    const g = new THREE.PlaneGeometry(0.3, 0.3)
    g.rotateX(-Math.PI / 2)
    g.setAttribute('aIdx', new THREE.InstancedBufferAttribute(Float32Array.from(built.beacons, (_, i) => i), 1))
    return g
  }, [built])
  useEffect(
    () => () => {
      built.ribbon.dispose()
      wakeGeo.dispose()
      ringGeo.dispose()
    },
    [built, wakeGeo, ringGeo],
  )
  const beaconGeo = useMemo(() => ({ post: new THREE.CylinderGeometry(0.0022, 0.0034, 0.058, 6).translate(0, 0.029, 0), lamp: new THREE.SphereGeometry(0.0085, 10, 8).translate(0, 0.064, 0) }), [])
  useEffect(
    () => () => {
      hulls.forEach((g) => g.dispose())
      beaconGeo.post.dispose()
      beaconGeo.lamp.dispose()
    },
    [hulls, beaconGeo],
  )

  // Initial uniform values. The frame loop writes through the material refs, never through these
  // objects: under StrictMode the memo can be re-run after the material has taken the first copy.
  const laneU = useMemo(() => ({ uTime: { value: 0 }, uShow: { value: 0 }, uOpacity: { value: 1 }, uColor: { value: new THREE.Color() }, uPulse: { value: new THREE.Color() } }), [])
  const wakeU = useMemo(() => ({ uShow: { value: 0 }, uWake: { value: 0.5 }, uGlow: { value: 1 }, uFoam: { value: new THREE.Color() } }), [])
  const ringU = useMemo(() => ({ uTime: { value: 0 }, uShow: { value: 0 }, uActive: { value: -1 }, uColor: { value: new THREE.Color() } }), [])

  const group = useRef<THREE.Group>(null)
  const meshes = useRef<(THREE.InstancedMesh | null)[]>([null, null, null])
  const wake = useRef<THREE.InstancedMesh>(null)
  const posts = useRef<THREE.InstancedMesh>(null)
  const lamps = useRef<THREE.InstancedMesh>(null)
  const rings = useRef<THREE.InstancedMesh>(null)
  const lampMat = useRef<THREE.MeshBasicMaterial>(null)
  const laneMat = useRef<THREE.ShaderMaterial>(null)
  const wakeMat = useRef<THREE.ShaderMaterial>(null)
  const ringMat = useRef<THREE.ShaderMaterial>(null)
  const dummy = useMemo(() => new THREE.Object3D(), [])
  const reveal = useRef(0)
  const placed = useRef(false)

  useLayoutEffect(() => {
    built.beacons.forEach((b, i) => {
      dummy.position.set(b.x, 0, b.z)
      dummy.rotation.set(0, 0, 0)
      dummy.scale.setScalar(1)
      dummy.updateMatrix()
      posts.current?.setMatrixAt(i, dummy.matrix)
      lamps.current?.setMatrixAt(i, dummy.matrix)
      dummy.position.y = SEA_Y + 0.0006
      dummy.updateMatrix()
      rings.current?.setMatrixAt(i, dummy.matrix)
    })
    for (const m of [posts.current, lamps.current, rings.current]) if (m) m.instanceMatrix.needsUpdate = true
    placed.current = false
  }, [built, dummy])

  const activeIndex = built.beacons.findIndex((b) => b.port.id === activePort)

  useFrame(({ clock }, dt) => {
    const g = group.current
    const laneU = laneMat.current?.uniforms, wakeU = wakeMat.current?.uniforms, ringU = ringMat.current?.uniforms
    if (!g || !laneU || !wakeU || !ringU) return
    // fade the whole layer rather than popping it
    const target = show ? 1 : 0
    reveal.current = animate ? reveal.current + (target - reveal.current) * Math.min(1, dt * 5) : target
    if (Math.abs(reveal.current - target) < 0.002) reveal.current = target
    const r = reveal.current
    g.visible = r > 0.002
    if (!g.visible) return

    const t = mix.current
    const time = animate ? clock.elapsedTime : 0
    laneU.uTime.value = time
    laneU.uShow.value = r
    laneU.uOpacity.value = 0.92 - t * 0.14
    mixCol(laneU.uColor.value, '#7fe6f2', '#f4fbff', t)
    mixCol(laneU.uPulse.value, '#ffffff', '#ffffff', t)
    wakeU.uShow.value = r
    wakeU.uWake.value = 0.34 + t * 0.4
    wakeU.uGlow.value = 0.95 - t * 0.8
    mixCol(wakeU.uFoam.value, '#cfeef2', '#ffffff', t)
    ringU.uTime.value = time
    ringU.uShow.value = r
    ringU.uActive.value = activeIndex
    mixCol(ringU.uColor.value, '#ff7d9f', '#e2386a', t)
    if (lampMat.current) {
      mixCol(lampMat.current.color, '#ff8fae', '#e2386a', t)
      lampMat.current.opacity = r
    }

    // vessels: advance along the lane, heading from the lane's tangent
    if (!animate && placed.current) return
    placed.current = true
    const step = animate ? Math.min(dt, 0.1) : 0
    const { vessels, lanes } = built
    for (let i = 0; i < vessels.length; i++) {
      const v = vessels[i]
      const l = lanes[v.lane]
      v.s += v.speed * step
      if (v.s >= l.total) v.s -= l.total
      const s = v.dir > 0 ? v.s : l.total - v.s
      let k = v.seg
      while (k < l.n - 2 && l.cum[k + 1] < s) k++
      while (k > 0 && l.cum[k] > s) k--
      v.seg = k
      const u = (s - l.cum[k]) / (l.cum[k + 1] - l.cum[k] || 1)
      const hx = (l.tx[k] + (l.tx[k + 1] - l.tx[k]) * u) * v.dir
      const hz = (l.tz[k] + (l.tz[k + 1] - l.tz[k]) * u) * v.dir
      // keep to starboard of the track; fishing boats sit to seaward
      const side = v.off * (l.kind === 'coastal' && v.off > 0.011 ? v.dir : 1)
      const edge = l.kind === 'trunk' ? 1.2 : 0.12
      const grow = smooth(0, edge, v.s) * (1 - smooth(l.total - edge, l.total, v.s))
      dummy.position.set(l.x[k] + (l.x[k + 1] - l.x[k]) * u - hz * side, 0.0028 + Math.sin(time * 1.3 + v.phase) * 0.0007, l.z[k] + (l.z[k + 1] - l.z[k]) * u + hx * side)
      dummy.rotation.set(0, Math.atan2(-hz, hx), 0)
      dummy.scale.setScalar(Math.max(0.0001, v.size * grow * (0.6 + 0.4 * r)))
      dummy.updateMatrix()
      meshes.current[v.kind]?.setMatrixAt(v.slot, dummy.matrix)
      wake.current?.setMatrixAt(i, dummy.matrix)
    }
    for (const m of meshes.current) if (m) m.instanceMatrix.needsUpdate = true
    if (wake.current) wake.current.instanceMatrix.needsUpdate = true
  })

  const active = activeIndex >= 0 ? built.beacons[activeIndex] : null
  const over = (id: string) => (e: ThreeEvent<PointerEvent>) => {
    e.stopPropagation()
    onPort(id)
    document.body.style.cursor = 'pointer'
  }
  const out = (e: ThreeEvent<PointerEvent>) => {
    document.body.style.cursor = ''
    // a tap keeps the card up; a mouse leaving puts it away
    if (e.pointerType !== 'touch') onPort(null)
  }

  return (
    <group ref={group} visible={false}>
      <mesh geometry={built.ribbon} frustumCulled={false} renderOrder={2}>
        <shaderMaterial ref={laneMat} uniforms={laneU} vertexShader={laneVertex} fragmentShader={laneFragment} transparent depthWrite={false} side={THREE.DoubleSide} polygonOffset polygonOffsetFactor={-4} polygonOffsetUnits={-4} />
      </mesh>

      <instancedMesh ref={wake} args={[wakeGeo, undefined, built.vessels.length]} frustumCulled={false} renderOrder={3}>
        <shaderMaterial ref={wakeMat} uniforms={wakeU} vertexShader={wakeVertex} fragmentShader={wakeFragment} transparent depthWrite={false} side={THREE.DoubleSide} polygonOffset polygonOffsetFactor={-6} polygonOffsetUnits={-6} />
      </instancedMesh>
      {hulls.map((geo, i) => (
        <instancedMesh
          key={i}
          ref={(m) => {
            meshes.current[i] = m
          }}
          args={[geo, undefined, built.counts[i]]}
          frustumCulled={false}
        >
          <meshStandardMaterial vertexColors flatShading roughness={0.7} metalness={0} />
        </instancedMesh>
      ))}

      <instancedMesh ref={rings} args={[ringGeo, undefined, built.beacons.length]} frustumCulled={false} renderOrder={4}>
        <shaderMaterial ref={ringMat} uniforms={ringU} vertexShader={ringVertex} fragmentShader={ringFragment} transparent depthWrite={false} side={THREE.DoubleSide} polygonOffset polygonOffsetFactor={-8} polygonOffsetUnits={-8} />
      </instancedMesh>
      <instancedMesh ref={posts} args={[undefined, undefined, built.beacons.length]} frustumCulled={false}>
        <primitive object={beaconGeo.post} attach="geometry" />
        <meshStandardMaterial color="#f3efe4" roughness={0.6} />
      </instancedMesh>
      <instancedMesh ref={lamps} args={[undefined, undefined, built.beacons.length]} frustumCulled={false}>
        <primitive object={beaconGeo.lamp} attach="geometry" />
        <meshBasicMaterial ref={lampMat} toneMapped={false} transparent />
      </instancedMesh>

      {show &&
        built.beacons.map((b) => (
          <mesh
            key={b.port.id}
            position={[b.x, 0.035, b.z]}
            onPointerOver={over(b.port.id)}
            onPointerOut={out}
            onClick={(e) => (e.stopPropagation(), onPort(b.port.id))}
          >
            <sphereGeometry args={[0.075, 10, 8]} />
            <meshBasicMaterial transparent opacity={0} depthWrite={false} />
          </mesh>
        ))}
      {show && active && (
        <Html key={active.port.id} position={[active.x, 0.085, active.z]} zIndexRange={[40, 30]} style={{ pointerEvents: 'none' }}>
          {renderPortCard(active.port)}
        </Html>
      )}
    </group>
  )
}
