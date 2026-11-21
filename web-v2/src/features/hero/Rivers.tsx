// Rivers: hairlines draped on the land that run all the way down to the sea.
//
// The source lines are a fragmented network (one piece per named reach), and some of
// them stop a little short of the coast. Here they are joined up logically: an end that
// nothing continues from, close to the shore and lower than the rest of its line, is a
// mouth. It is carried down the distance field to the waterline and a short way out,
// where the line widens a touch and dissolves into the water. Springs fade in.
//
// One draw call. The ribbon is expanded in screen space, so a river is the same hairline
// on a phone as on a desktop, and its colour cross-fades with the rest of the scene.
import { useFrame } from '@react-three/fiber'
import { useEffect, useMemo, useRef, type MutableRefObject } from 'react'
import * as THREE from 'three'
import type { Polyline } from '@/lib/geo'
import { SCENE_THEMES } from './sceneTheme'
import { K, toXZ, type Terrain } from './terrain'
import { lerp, tween } from './tween'

const LIFT = 0.0035
/** an end this close to the coast (degrees) is taken to be heading for it */
const MOUTH_REACH = 0.1
/** how far past the waterline a river is drawn before it has dissolved */
const PLUME = 0.036
/** stride of the stretch added below a mouth, in degrees */
const STEP = 0.006

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)))
  return t * t * (3 - 2 * t)
}

type Sample = { lon: number; lat: number; /** 0 at a spring, where the line fades in */ source: number; /** 1 … 0 along the stretch drawn past the waterline */ plume?: number }

function buildRivers(lines: Polyline[], terrain: Terrain) {
  const d = terrain.coastDistance
  const key = (p: [number, number]) => `${p[0].toFixed(3)},${p[1].toFixed(3)}`

  // who touches each node: line ends, and lines passing through
  const nodes = new Map<string, { ends: { line: number; first: boolean }[]; through: number }>()
  const node = (p: [number, number]) => {
    const k = key(p)
    let n = nodes.get(k)
    if (!n) nodes.set(k, (n = { ends: [], through: 0 }))
    return n
  }
  lines.forEach((l, i) => {
    if (l.length < 2) return
    node(l[0]).ends.push({ line: i, first: true })
    node(l[l.length - 1]).ends.push({ line: i, first: false })
    for (let j = 1; j < l.length - 1; j++) node(l[j]).through++
  })

  /** the way to the sea from a point, as a unit step in (lon·K, lat) space. Sampled wide, so a creek or a sandbar does not turn it. */
  const seaward = (lon: number, lat: number): [number, number] => {
    const e = 0.03
    const gx = (d(lon + e, lat) - d(lon - e, lat)) / K, gy = d(lon, lat + e) - d(lon, lat - e)
    const l = Math.hypot(gx, gy) || 1
    return [-gx / l, -gy / l]
  }

  const paths: Sample[][] = []
  lines.forEach((l, i) => {
    if (l.length < 2) return
    const pts: Sample[] = l.map(([lon, lat]) => ({ lon, lat, source: 1 }))
    for (const first of [true, false]) {
      const end = first ? l[0] : l[l.length - 1], other = first ? l[l.length - 1] : l[0], next = first ? l[1] : l[l.length - 2]
      const n = nodes.get(key(end))!
      if (n.through > 0) continue
      const here = d(end[0], end[1])
      const lower = here < d(other[0], other[1]) - 0.004
      if (n.ends.length === 1 && !lower) {
        // a spring: nothing feeds it and it is the high end of its line
        const s = pts[first ? 0 : pts.length - 1]
        s.source = 0
        continue
      }
      // a mouth: every line that meets here ends here, and this is the first of them to claim it
      if (!lower || here > MOUTH_REACH || n.ends[0].line !== i || n.ends[0].first !== first) continue
      // Carry it down to the water. It leaves on the river's own heading and bends toward the shore a little
      // at a time; once it is afloat it holds its course and dissolves over a short, fixed distance.
      let lon = end[0], lat = end[1]
      let hx = (end[0] - next[0]) * K, hy = end[1] - next[1]
      const hl = Math.hypot(hx, hy) || 1
      hx /= hl
      hy /= hl
      const tail: Sample[] = []
      let afloat = here < 0 ? 0.004 : 0
      for (let step = 0; step < 40 && afloat < PLUME; step++) {
        if (afloat === 0) {
          const [gx, gy] = seaward(lon, lat)
          hx = hx * 0.62 + gx * 0.38
          hy = hy * 0.62 + gy * 0.38
          const l2 = Math.hypot(hx, hy) || 1
          hx /= l2
          hy /= l2
        }
        lon += (hx * STEP) / K
        lat += hy * STEP
        if (afloat > 0 || d(lon, lat) < 0) afloat += STEP
        tail.push({ lon, lat, source: 1, plume: 1 - smooth(0, PLUME, afloat) })
      }
      if (first) pts.unshift(...tail.reverse())
      else pts.push(...tail)
    }
    paths.push(pts)
  })

  /* ---- sample each path: coarse inland, fine where it crosses the rim ---- */
  const position: number[] = [], tangent: number[] = [], info: number[] = [], index: number[] = []
  const xs: number[] = [], ys: number[] = [], zs: number[] = [], al: number[] = [], wd: number[] = []
  for (const pts of paths) {
    xs.length = ys.length = zs.length = al.length = wd.length = 0
    let run = 0
    const total = pts.reduce((s, p, j) => (j ? s + Math.hypot((p.lon - pts[j - 1].lon) * K, p.lat - pts[j - 1].lat) : 0), 0)
    const springAtStart = pts[0].source === 0, springAtEnd = pts[pts.length - 1].source === 0
    const push = (lon: number, lat: number, at: number, plume: number | undefined) => {
      const dist = d(lon, lat)
      const [x, z] = toXZ(lon, lat)
      xs.push(x)
      ys.push((dist > 0 ? terrain.surfaceAt(lon, lat) : 0) + LIFT)
      zs.push(z)
      // a spring fades in over its first stretch
      const spring = (springAtStart ? smooth(0, 0.05, at) : 1) * (springAtEnd ? smooth(0, 0.05, total - at) : 1)
      // below a mouth the fade is laid out along the line; anywhere else a river over water (a lagoon it crosses) thins with the depth
      al.push((plume ?? smooth(-PLUME, -0.004, dist)) * spring)
      wd.push(1 + 0.5 * smooth(0.07, -0.01, dist))
    }
    for (let j = 0; j < pts.length - 1; j++) {
      const a = pts[j], b = pts[j + 1]
      const len = Math.hypot((b.lon - a.lon) * K, b.lat - a.lat)
      const near = Math.min(d(a.lon, a.lat), d(b.lon, b.lat)) < 0.04
      const n = Math.max(1, Math.ceil(len / (near ? 0.006 : 0.022)))
      const tail = a.plume !== undefined || b.plume !== undefined
      for (let s = 0; s < n; s++) {
        const u = s / n
        push(a.lon + (b.lon - a.lon) * u, a.lat + (b.lat - a.lat) * u, run + len * u, tail ? (a.plume ?? 1) + ((b.plume ?? 1) - (a.plume ?? 1)) * u : undefined)
      }
      run += len
      if (j === pts.length - 2) push(b.lon, b.lat, run, tail ? (b.plume ?? 1) : undefined)
    }
    const m = xs.length
    if (m < 2) continue
    const base = position.length / 3
    for (let j = 0; j < m; j++) {
      const p = Math.max(0, j - 1), q = Math.min(m - 1, j + 1)
      let tx = xs[q] - xs[p], ty = ys[q] - ys[p], tz = zs[q] - zs[p]
      const tl = Math.hypot(tx, ty, tz) || 1
      tx /= tl
      ty /= tl
      tz /= tl
      for (const side of [-1, 1]) {
        position.push(xs[j], ys[j], zs[j])
        tangent.push(tx, ty, tz)
        info.push(side, al[j], wd[j])
      }
      if (j < m - 1) {
        const v = base + j * 2
        index.push(v, v + 1, v + 2, v + 1, v + 3, v + 2)
      }
    }
  }
  const g = new THREE.BufferGeometry()
  g.setAttribute('position', new THREE.Float32BufferAttribute(position, 3))
  g.setAttribute('aTan', new THREE.Float32BufferAttribute(tangent, 3))
  g.setAttribute('aInfo', new THREE.Float32BufferAttribute(info, 3))
  g.setIndex(index)
  return g
}

const vertex = /* glsl */ `
  attribute vec3 aTan;
  attribute vec3 aInfo;
  uniform vec2 uRes;
  uniform float uPx;
  varying float vEdge;
  varying float vHalf;
  varying float vAlpha;
  void main() {
    vec4 w = modelMatrix * vec4(position, 1.0);
    vec4 c = projectionMatrix * viewMatrix * w;
    vec4 a = projectionMatrix * viewMatrix * (w + vec4(aTan, 0.0) * 0.02);
    vec2 dir = (a.xy / max(a.w, 1e-4) - c.xy / max(c.w, 1e-4)) * uRes;
    float l = length(dir);
    dir = l > 1e-6 ? dir / l : vec2(1.0, 0.0);
    float width = aInfo.z * uPx;
    float reach = width * 0.5 + uPx;
    c.xy += vec2(-dir.y, dir.x) * aInfo.x * reach * 2.0 / uRes * c.w;
    vEdge = aInfo.x * reach;
    vHalf = width * 0.5;
    vAlpha = aInfo.y;
    gl_Position = c;
  }`
const fragment = /* glsl */ `
  uniform vec3 uColor;
  uniform float uOpacity;
  varying float vEdge;
  varying float vHalf;
  varying float vAlpha;
  void main() {
    float aa = max(fwidth(vEdge), 1e-3);
    float line = 1.0 - smoothstep(vHalf - 0.5 * aa, vHalf + 0.5 * aa, abs(vEdge));
    gl_FragColor = vec4(uColor, line * vAlpha * uOpacity);
    #include <colorspace_fragment>
  }`

const N = SCENE_THEMES.dark.land, D = SCENE_THEMES.light.land
const _size = new THREE.Vector2()

export function Rivers({ lines, terrain, mix }: { lines: Polyline[]; terrain: Terrain; /** 0 = night, 1 = day */ mix: MutableRefObject<number> }) {
  const geometry = useMemo(() => buildRivers(lines, terrain), [lines, terrain])
  useEffect(() => () => geometry.dispose(), [geometry])
  const mat = useRef<THREE.ShaderMaterial>(null)
  // initial values only: the frame loop writes through the material
  const uniforms = useMemo(() => ({ uRes: { value: new THREE.Vector2(1, 1) }, uPx: { value: 1 }, uColor: { value: new THREE.Color() }, uOpacity: { value: 0 } }), [])
  useFrame(({ gl }) => {
    const u = mat.current?.uniforms
    if (!u) return
    const t = mix.current
    gl.getDrawingBufferSize(_size)
    u.uRes.value.copy(_size)
    u.uPx.value = gl.getPixelRatio()
    tween(u.uColor.value, N.river, D.river, t)
    u.uOpacity.value = lerp(N.riverOpacity, D.riverOpacity, t)
  })
  return (
    <mesh geometry={geometry} frustumCulled={false} renderOrder={1}>
      <shaderMaterial ref={mat} uniforms={uniforms} vertexShader={vertex} fragmentShader={fragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
    </mesh>
  )
}
