// Maritime overlay for the island: shipping lanes on the water, a small fleet under
// way, and a marker at every port. Drawn as a data layer, not as scenery: hairlines
// of constant screen width, one hue, and rank carried by weight.
//
// Budget: four draw calls (lanes, vessels, port rings, port pins). The frame loop
// allocates nothing: vessels are two instanced attributes rewritten in place.
import { Html } from '@react-three/drei'
import { useFrame, type ThreeEvent } from '@react-three/fiber'
import { useEffect, useLayoutEffect, useMemo, useRef, type MutableRefObject, type ReactNode } from 'react'
import * as THREE from 'three'
import { FLEET, LANES, PORTS, SPEED, type LaneKind, type Port, type VesselKind } from './maritimeData'
import { hazeRate } from './haze'
import { SCENE_THEMES } from './sceneTheme'
import { K, ORIGIN, toXZ, type Terrain } from './terrain'

type XZ = [number, number]
/** what a line is drawn as: the three ranks of route, plus the short ties into each harbour */
type Rank = LaneKind | 'spur'
type Lane = {
  id: string
  kind: Rank
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

const RANK_INDEX: Record<Rank, number> = { trunk: 0, feeder: 1, coastal: 2, spur: 3 }
const SEA_Y = 0.003

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

function makeLane(terrain: Terrain, id: string, kind: Rank, xz: XZ[], step: number, margin: number, fade: [number, number] = [0, 0]): Lane {
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

type Vessel = { lane: number; s: number; speed: number; dir: 1 | -1; off: number; size: number; trail: number; seg: number }
type Beacon = { port: Port; x: number; z: number }

/** hull length in scene units: one glyph for every vessel, the class only changes its size */
const VESSEL_SIZE: Record<VesselKind, number> = { container: 0.058, tanker: 0.064, fishing: 0.026 }
/** wake length, in hull lengths */
const VESSEL_TRAIL: Record<VesselKind, number> = { container: 2.6, tanker: 2.6, fishing: 0 }
const MAX_TRAIL = 2.6

function buildMaritime(terrain: Terrain, lowPower: boolean) {
  const lanes: Lane[] = LANES.map((l) =>
    makeLane(terrain, l.id, l.kind, l.pts.map(([lon, lat]) => toXZ(lon, lat)), l.kind === 'trunk' ? 0.06 : 0.03, l.kind === 'trunk' ? 0.12 : 0.06, l.fade),
  )
  const coastal = lanes.find((l) => l.id === 'coastal')!

  // markers stand in the harbour mouth; a short spur ties each one to the coasting route
  const beacons: Beacon[] = PORTS.map((port) => {
    const p: XZ[] = [toXZ(port.lon, port.lat)]
    keepAtSea(terrain, p, 0.02)
    return { port, x: p[0][0], z: p[0][1] }
  })
  for (const b of beacons) {
    let best = 0, bd = Infinity
    for (let i = 0; i < coastal.n; i++) {
      const d = Math.hypot(coastal.x[i] - b.x, coastal.z[i] - b.z)
      if (d < bd) { bd = d; best = i }
    }
    if (bd > 0.02) lanes.push(makeLane(terrain, `spur-${b.port.id}`, 'spur', [[coastal.x[best], coastal.z[best]], [b.x, b.z]], 0.02, 0.016))
  }

  /* ---- one strip for every lane: two vertices per sample, pushed apart in screen space by the shader ---- */
  let verts = 0
  for (const l of lanes) verts += l.n * 2
  const position = new Float32Array(verts * 3), line = new Float32Array(verts * 4), info = new Float32Array(verts * 3)
  const index: number[] = []
  let v = 0
  lanes.forEach((l, li) => {
    for (let i = 0; i < l.n; i++) {
      const s = l.cum[i]
      const fade = (l.fade[0] ? smooth(0, l.fade[0], s) : 1) * (l.fade[1] ? 1 - smooth(l.total - l.fade[1], l.total, s) : 1)
      for (const side of [-1, 1]) {
        position[v * 3] = l.x[i]
        position[v * 3 + 1] = SEA_Y
        position[v * 3 + 2] = l.z[i]
        line[v * 4] = l.tx[i]
        line[v * 4 + 1] = l.tz[i]
        line[v * 4 + 2] = side
        line[v * 4 + 3] = s
        info[v * 3] = RANK_INDEX[l.kind]
        info[v * 3 + 1] = fade
        // where along its cycle this lane's pulse starts, so no two lanes fire together
        info[v * 3 + 2] = (li * 0.381966) % 1
        if (side === 1 && i < l.n - 1) index.push(v - 1, v, v + 1, v, v + 2, v + 1)
        v++
      }
    }
  })
  const strip = new THREE.BufferGeometry()
  strip.setAttribute('position', new THREE.BufferAttribute(position, 3))
  strip.setAttribute('aLine', new THREE.BufferAttribute(line, 4))
  strip.setAttribute('aInfo', new THREE.BufferAttribute(info, 3))
  strip.setIndex(index)

  /* ---- the fleet, seeded so every visit sees the same sea ---- */
  let seed = 20261118
  const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296)
  const vessels: Vessel[] = []
  for (const f of FLEET) {
    const li = lanes.findIndex((l) => l.id === f.lane)
    const lane = lanes[li]
    const count = lowPower ? Math.ceil(f.count / 2) : f.count
    for (let i = 0; i < count; i++) {
      const fishing = f.kind === 'fishing'
      vessels.push({
        lane: li,
        s: ((i + 0.2 + rnd() * 0.6) / count) * lane.total,
        speed: SPEED[f.kind] * (0.88 + rnd() * 0.24),
        dir: f.bothWays && rnd() > 0.5 ? -1 : 1,
        // fishing boats work the water to seaward of the route; ships hold to the line
        off: fishing ? 0.014 + rnd() * 0.05 : 0,
        size: VESSEL_SIZE[f.kind] * (lane.kind === 'coastal' && !fishing ? 0.78 : 1),
        trail: VESSEL_TRAIL[f.kind],
        seg: 0,
      })
    }
  }

  /* ---- vessels: one quad, instanced; the shader turns it to the heading and draws the glyph ---- */
  const glyph = new THREE.InstancedBufferGeometry()
  // in hull lengths: x runs astern → ahead with the bow at +0.5, y across
  const x0 = -0.5 - MAX_TRAIL - 0.1, x1 = 0.66, y = 0.34
  glyph.setAttribute('position', new THREE.Float32BufferAttribute([x0, -y, 0, x1, -y, 0, x1, y, 0, x0, y, 0], 3))
  glyph.setIndex([0, 1, 2, 0, 2, 3])
  /** per vessel: x, z, heading x, heading z */
  const ship = new THREE.InstancedBufferAttribute(new Float32Array(vessels.length * 4), 4)
  /** per vessel: hull length (0 hides it), wake length in hull lengths */
  const meta = new THREE.InstancedBufferAttribute(new Float32Array(vessels.length * 2), 2)
  ship.setUsage(THREE.DynamicDrawUsage)
  meta.setUsage(THREE.DynamicDrawUsage)
  glyph.setAttribute('aShip', ship)
  glyph.setAttribute('aMeta', meta)
  glyph.instanceCount = vessels.length

  return { lanes, beacons, strip, vessels, glyph, ship, meta }
}

/* ---------------- shaders ---------------- */
const HAZE = /* glsl */ `
  uniform float uHaze;
  float seaHaze(vec3 world) {
    float dist = length(cameraPosition - world);
    return exp(-pow(dist * uHaze, 2.2));
  }`

// Lanes are expanded in screen space: every line keeps its width in pixels from the
// foreground to the horizon, so nothing thins to a shimmer or fattens near the camera.
const laneVertex = /* glsl */ `
  attribute vec4 aLine;
  attribute vec3 aInfo;
  uniform vec2 uRes;
  uniform float uPx;
  varying float vEdge;
  varying float vHalf;
  varying float vS;
  varying vec3 vInfo;
  varying vec3 vWorld;
  void main() {
    vec4 w = modelMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    vec4 c = projectionMatrix * viewMatrix * w;
    vec4 a = projectionMatrix * viewMatrix * (w + vec4(aLine.x, 0.0, aLine.y, 0.0) * 0.02);
    vec2 dir = (a.xy / max(a.w, 1e-4) - c.xy / max(c.w, 1e-4)) * uRes;
    float l = length(dir);
    dir = l > 1e-6 ? dir / l : vec2(1.0, 0.0);
    // trunk 1.7 px, feeder 1.25 px, coasting route and spurs 1 px
    float width = (aInfo.x < 0.5 ? 1.7 : aInfo.x < 1.5 ? 1.25 : 1.0) * uPx;
    // room either side for the edge to soften and for a pulse to swell the line
    float reach = width * 0.5 + 1.8 * uPx;
    c.xy += vec2(-dir.y, dir.x) * aLine.z * reach * 2.0 / uRes * c.w;
    vEdge = aLine.z * reach;
    vHalf = width * 0.5;
    vS = aLine.w;
    vInfo = aInfo;
    gl_Position = c;
  }`
const laneFragment = /* glsl */ `
  uniform float uTime, uShow, uPx, uPulseA;
  uniform vec3 uColor, uPulse;
  uniform vec4 uRank;
  varying float vEdge;
  varying float vHalf;
  varying float vS;
  varying vec3 vInfo;
  varying vec3 vWorld;
  ${HAZE}
  void main() {
    float k = vInfo.x;
    float rank = k < 0.5 ? uRank.x : k < 1.5 ? uRank.y : k < 2.5 ? uRank.z : uRank.w;
    float coastal = step(1.5, k) * (1.0 - step(2.5, k));

    // Pulses: every lane runs them at the same speed; busier lanes simply carry them closer together.
    // A soft head with a long tail, and nothing on the harbour spurs.
    float gap = k < 0.5 ? 6.5 : k < 1.5 ? 4.5 : 9.0;
    float q = fract((uTime * 0.16 - vS) / gap + vInfo.z);
    float pulse = (exp(-q * gap / 0.42) + exp(-(1.0 - q) * gap / 0.03)) * (1.0 - step(2.5, k)) * uPulseA;
    pulse *= mix(1.0, 0.7, coastal);

    float halfW = vHalf + pulse * 0.55 * uPx;
    float aa = max(fwidth(vEdge), 1e-3);
    float line = 1.0 - smoothstep(halfW - 0.5 * aa, halfW + 0.5 * aa, abs(vEdge));
    // coasting routes are dashed, the way a chart draws a recommended track
    float fs = fwidth(vS);
    float dp = abs(fract(vS / 0.09) - 0.5) * 0.09;
    float dash = mix(1.0, 1.0 - smoothstep(0.026 - fs, 0.026 + fs, dp), coastal);

    float alpha = line * dash * mix(rank, 1.0, pulse * 0.9);
    vec3 c = mix(uColor, uPulse, pulse * 0.85);
    gl_FragColor = vec4(c, alpha * vInfo.y * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

// Vessels are map symbols: a hull plan turned to its heading on screen, at the size the
// ship would have at that depth. Unlike a decal on the water, it never foreshortens into a blob.
const vesselVertex = /* glsl */ `
  attribute vec4 aShip;
  attribute vec2 aMeta;
  uniform vec2 uRes;
  uniform float uPx;
  varying vec2 vP;
  varying float vLen;
  varying float vTrail;
  varying vec3 vWorld;
  void main() {
    vec3 centre = vec3(aShip.x, ${SEA_Y.toFixed(4)}, aShip.y);
    vWorld = centre;
    vec4 c = projectionMatrix * viewMatrix * vec4(centre, 1.0);
    vec4 a = projectionMatrix * viewMatrix * vec4(centre + vec3(aShip.z, 0.0, aShip.w) * 0.04, 1.0);
    vec2 dir = (a.xy / max(a.w, 1e-4) - c.xy / max(c.w, 1e-4)) * uRes;
    float l = length(dir);
    dir = l > 1e-6 ? dir / l : vec2(1.0, 0.0);
    float len = max(aMeta.x * projectionMatrix[1][1] / max(c.w, 1e-4) * uRes.y * 0.5, 5.0 * uPx) * step(1e-5, aMeta.x);
    c.xy += (dir * position.x + vec2(-dir.y, dir.x) * position.y) * len * 2.0 / uRes * c.w;
    vP = position.xy;
    vLen = len;
    vTrail = aMeta.y;
    gl_Position = c;
  }`
const vesselFragment = /* glsl */ `
  uniform vec3 uColor, uEdge;
  uniform float uShow;
  varying vec2 vP;
  varying float vLen;
  varying float vTrail;
  varying vec3 vWorld;
  ${HAZE}
  void main() {
    float px = 1.0 / max(vLen, 1.0); // one pixel, in hull lengths
    float x = vP.x, y = abs(vP.y);
    // hull plan: full amidships, a long fine entry to the bow, a slightly drawn-in stern
    float bow = clamp((0.5 - x) * 2.0, 0.0, 1.0);
    float halfBeam = 0.125 * pow(bow, 0.6) * (0.74 + 0.26 * smoothstep(-0.5, -0.4, x));
    float d = max(y - halfBeam, max(-0.5 - x, x - 0.5));
    float hull = 1.0 - smoothstep(-0.55 * px, 0.55 * px, d);
    // a hairline of the water's own dark around it, so a white hull holds on a pale shelf
    float keyline = (1.0 - smoothstep(0.5 * px, 2.4 * px, d)) * 0.4;
    // wake: a single hairline astern, fading out
    float t = clamp((-0.5 - x) / max(vTrail, 1e-3), 0.0, 1.0);
    float wake = (1.0 - smoothstep(0.25 * px, 1.05 * px, y)) * (1.0 - t) * (1.0 - t) * step(x, -0.5) * step(1e-3, vTrail) * 0.46;
    float ink = max(hull, wake);
    float alpha = max(ink, keyline);
    vec3 c = mix(uEdge, uColor, clamp(ink / max(alpha, 1e-4), 0.0, 1.0));
    gl_FragColor = vec4(c, alpha * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

// Port marker on the water: a core, one fine ring, and a single slow ping. Flat on the
// sea, so perspective turns it into the ellipse that says "this is on the surface".
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
  uniform float uTime, uShow, uActive, uPing;
  uniform vec3 uColor, uCore, uHot;
  varying vec2 vUv;
  varying float vIdx;
  varying vec3 vWorld;
  ${HAZE}
  float stroke(float x, float px, float w) { return 1.0 - smoothstep(0.0, 1.0, abs(x) / px - w); }
  void main() {
    float d = length(vUv - 0.5) * 2.0;
    float px = max(fwidth(d), 1e-4);
    float act = 1.0 - step(0.5, abs(vIdx - uActive));
    const float R = 0.36;
    float ring = stroke(d - R, px, 0.25 + 0.35 * act);
    // one ping every five seconds, staggered port to port, gone before it has travelled far
    float ph = fract(uTime * 0.2 + vIdx * 0.381966);
    float ping = stroke(d - mix(R, 0.95, ph), px, 0.2) * (1.0 - ph) * (1.0 - ph) * 0.5 * uPing;
    float core = 1.0 - smoothstep(0.11 - px, 0.11 + px, d);
    float fill = (1.0 - smoothstep(R - px, R + px, d)) * (0.1 + 0.2 * act);
    float alpha = max(max(ring * (0.8 + 0.2 * act), core), max(ping, fill));
    vec3 c = mix(mix(uColor, uHot, act), uCore, core);
    gl_FragColor = vec4(c, alpha * uShow * seaHaze(vWorld));
    #include <colorspace_fragment>
  }`

const N = SCENE_THEMES.dark.maritime, D = SCENE_THEMES.light.maritime
const parsed = new Map<string, THREE.Color>()
const C = (hex: string) => {
  let c = parsed.get(hex)
  if (!c) parsed.set(hex, (c = new THREE.Color(hex)))
  return c
}
/** out = night → day at t; both ends are parsed once, never per frame */
const tween = (out: THREE.Color, night: string, day: string, t: number) => out.lerpColors(C(night), C(day), t)
const lerp = (a: number, b: number, t: number) => a + (b - a) * t
const _size = new THREE.Vector2()

export function Maritime({
  terrain, mix, show, animate, lowPower, activePort, onPort, renderPortCard,
}: {
  terrain: Terrain
  /** 0 = night, 1 = day */
  mix: MutableRefObject<number>
  show: boolean
  animate: boolean
  lowPower: boolean
  activePort: string | null
  onPort: (id: string | null) => void
  renderPortCard: (port: Port) => ReactNode
}) {
  const built = useMemo(() => buildMaritime(terrain, lowPower), [terrain, lowPower])
  const ringGeo = useMemo(() => {
    const g = new THREE.PlaneGeometry(0.26, 0.26)
    g.rotateX(-Math.PI / 2)
    g.setAttribute('aIdx', new THREE.InstancedBufferAttribute(Float32Array.from(built.beacons, (_, i) => i), 1))
    return g
  }, [built])
  // the pin: a hairline stem with a small head, turned as one profile
  const pinGeo = useMemo(() => {
    const pts: THREE.Vector2[] = [new THREE.Vector2(0.0015, 0), new THREE.Vector2(0.0011, 0.04)]
    for (let i = 0; i <= 8; i++) {
      const a = -1.15 + (i / 8) * (Math.PI / 2 + 1.15)
      pts.push(new THREE.Vector2(Math.cos(a) * 0.0062, 0.0462 + Math.sin(a) * 0.0062))
    }
    return new THREE.LatheGeometry(pts, 10)
  }, [])
  useEffect(
    () => () => {
      built.strip.dispose()
      built.glyph.dispose()
      ringGeo.dispose()
    },
    [built, ringGeo],
  )
  useEffect(() => () => pinGeo.dispose(), [pinGeo])

  // Initial uniform values. The frame loop writes through the material refs, never through these
  // objects: under StrictMode the memo can be re-run after the material has taken the first copy.
  const laneU = useMemo(() => ({ uHaze: { value: 0.05 }, uTime: { value: 0 }, uShow: { value: 0 }, uPx: { value: 1 }, uPulseA: { value: 1 }, uRes: { value: new THREE.Vector2(1, 1) }, uRank: { value: new THREE.Vector4(1, 1, 1, 1) }, uColor: { value: new THREE.Color() }, uPulse: { value: new THREE.Color() } }), [])
  const vesselU = useMemo(() => ({ uHaze: { value: 0.05 }, uShow: { value: 0 }, uPx: { value: 1 }, uRes: { value: new THREE.Vector2(1, 1) }, uColor: { value: new THREE.Color() }, uEdge: { value: new THREE.Color() } }), [])
  const ringU = useMemo(() => ({ uHaze: { value: 0.05 }, uTime: { value: 0 }, uShow: { value: 0 }, uActive: { value: -1 }, uPing: { value: 1 }, uColor: { value: new THREE.Color() }, uCore: { value: new THREE.Color() }, uHot: { value: new THREE.Color() } }), [])

  const group = useRef<THREE.Group>(null)
  const pins = useRef<THREE.InstancedMesh>(null)
  const rings = useRef<THREE.InstancedMesh>(null)
  const pinMat = useRef<THREE.MeshBasicMaterial>(null)
  const laneMat = useRef<THREE.ShaderMaterial>(null)
  const vesselMat = useRef<THREE.ShaderMaterial>(null)
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
      pins.current?.setMatrixAt(i, dummy.matrix)
      dummy.position.y = SEA_Y + 0.0004
      dummy.updateMatrix()
      rings.current?.setMatrixAt(i, dummy.matrix)
    })
    for (const m of [pins.current, rings.current]) if (m) m.instanceMatrix.needsUpdate = true
    placed.current = false
  }, [built, dummy])

  const activeIndex = built.beacons.findIndex((b) => b.port.id === activePort)

  useFrame(({ clock, gl, camera }, dt) => {
    const g = group.current
    const laneU = laneMat.current?.uniforms, vesselU = vesselMat.current?.uniforms, ringU = ringMat.current?.uniforms
    if (!g || !laneU || !vesselU || !ringU) return
    // fade the whole layer rather than popping it
    const target = show ? 1 : 0
    reveal.current = animate ? reveal.current + (target - reveal.current) * Math.min(1, dt * 5) : target
    if (Math.abs(reveal.current - target) < 0.002) reveal.current = target
    const r = reveal.current
    g.visible = r > 0.002
    if (!g.visible) return

    const t = mix.current
    const time = animate ? clock.elapsedTime : 0
    const px = gl.getPixelRatio()
    gl.getDrawingBufferSize(_size)
    laneU.uHaze.value = vesselU.uHaze.value = ringU.uHaze.value = hazeRate(camera)
    laneU.uTime.value = time
    laneU.uShow.value = r
    laneU.uPx.value = px
    laneU.uPulseA.value = animate ? 1 : 0
    laneU.uRes.value.copy(_size)
    laneU.uRank.value.set(lerp(N.trunk, D.trunk, t), lerp(N.feeder, D.feeder, t), lerp(N.coastal, D.coastal, t), lerp(N.spur, D.spur, t))
    tween(laneU.uColor.value, N.lane, D.lane, t)
    tween(laneU.uPulse.value, N.pulse, D.pulse, t)
    vesselU.uShow.value = r
    vesselU.uPx.value = px
    vesselU.uRes.value.copy(_size)
    tween(vesselU.uColor.value, N.vessel, D.vessel, t)
    tween(vesselU.uEdge.value, N.vesselEdge, D.vesselEdge, t)
    ringU.uTime.value = time
    ringU.uShow.value = r
    ringU.uActive.value = activeIndex
    ringU.uPing.value = animate ? 1 : 0
    tween(ringU.uColor.value, N.port, D.port, t)
    tween(ringU.uCore.value, N.portCore, D.portCore, t)
    tween(ringU.uHot.value, N.portActive, D.portActive, t)
    if (pinMat.current) {
      tween(pinMat.current.color, N.port, D.port, t)
      pinMat.current.opacity = r
    }

    // vessels: advance along the lane, heading from the lane's tangent
    if (!animate && placed.current) return
    placed.current = true
    const step = animate ? Math.min(dt, 0.1) : 0
    const { vessels, lanes } = built
    const ship = built.ship.array as Float32Array, meta = built.meta.array as Float32Array
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
      // fishing boats sit to seaward of the coasting route
      const side = v.off * v.dir
      // vessels ease in and out at the ends of a lane instead of popping
      const edge = l.kind === 'trunk' ? 1.2 : 0.14
      const grow = smooth(0, edge, v.s) * (1 - smooth(l.total - edge, l.total, v.s))
      ship[i * 4] = l.x[k] + (l.x[k + 1] - l.x[k]) * u - hz * side
      ship[i * 4 + 1] = l.z[k] + (l.z[k + 1] - l.z[k]) * u + hx * side
      ship[i * 4 + 2] = hx
      ship[i * 4 + 3] = hz
      meta[i * 2] = v.size * grow
      meta[i * 2 + 1] = v.trail
    }
    built.ship.needsUpdate = true
    built.meta.needsUpdate = true
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
      <mesh geometry={built.strip} frustumCulled={false} renderOrder={2}>
        <shaderMaterial ref={laneMat} uniforms={laneU} vertexShader={laneVertex} fragmentShader={laneFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </mesh>

      <instancedMesh ref={rings} args={[ringGeo, undefined, built.beacons.length]} frustumCulled={false} renderOrder={3}>
        <shaderMaterial ref={ringMat} uniforms={ringU} vertexShader={ringVertex} fragmentShader={ringFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </instancedMesh>
      <instancedMesh ref={pins} args={[pinGeo, undefined, built.beacons.length]} frustumCulled={false}>
        <meshBasicMaterial ref={pinMat} toneMapped={false} transparent />
      </instancedMesh>

      <mesh geometry={built.glyph} frustumCulled={false} renderOrder={4}>
        <shaderMaterial ref={vesselMat} uniforms={vesselU} vertexShader={vesselVertex} fragmentShader={vesselFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </mesh>

      {show &&
        built.beacons.map((b) => (
          <mesh
            key={b.port.id}
            position={[b.x, 0.03, b.z]}
            onPointerOver={over(b.port.id)}
            onPointerOut={out}
            onClick={(e) => (e.stopPropagation(), onPort(b.port.id))}
          >
            <sphereGeometry args={[0.075, 10, 8]} />
            <meshBasicMaterial transparent opacity={0} depthWrite={false} />
          </mesh>
        ))}
      {show && active && (
        <Html key={active.port.id} position={[active.x, 0.07, active.z]} zIndexRange={[40, 30]} style={{ pointerEvents: 'none' }}>
          {renderPortCard(active.port)}
        </Html>
      )}
    </group>
  )
}
