// Population overlay: one needle per place.
//
// Drawn as a data layer, in the same hand as the shipping lanes: hairlines of constant
// screen width and one hue. Each place carries its number three ways, so none of them has
// to shout: the needle's height and the area of the ring at its foot both scale with the
// residents (√, so a city ten times the size is not ten times the ink), and the tone runs
// along one sequential ramp. The rings overlap into a soft density where towns crowd
// together, and are clipped to the land so they read as painted on it.
//
// Budget: three draw calls, no shadows, and nothing per frame but a handful of uniforms.
// The grow-in is solved in the vertex shader from one number, staggered outward from the
// largest place.
import { useFrame } from '@react-three/fiber'
import { useEffect, useMemo, useRef, type MutableRefObject } from 'react'
import * as THREE from 'three'
import type { Place } from '@/lib/geo'
import { SCENE_THEMES } from './sceneTheme'
import { K, ORIGIN, toXZ, type Terrain } from './terrain'
import { tween } from './tween'

/** the tallest needle, in scene units: about the height of Sri Pada, so the data never out-climbs the island */
const MAX_H = 0.29
const MIN_H = 0.026
/** share of the timeline the stagger takes; each needle grows over the rest */
const STAGGER = 0.5

const SHARED = /* glsl */ `
  attribute vec4 aBase;
  attribute vec3 aInfo;
  uniform float uGrow;
  uniform vec2 uRes;
  uniform float uPx;
  // 0 → 1 for this place: starts after its delay, lands softly
  float grown() {
    float g = clamp((uGrow - aInfo.y) / ${(1 - STAGGER).toFixed(2)}, 0.0, 1.0);
    return 1.0 - pow(1.0 - g, 4.0);
  }`
const RAMP = /* glsl */ `
  uniform vec3 uRamp[4];
  vec3 ramp(float t) {
    float x = clamp(t, 0.0, 1.0) * 3.0;
    return x < 1.0 ? mix(uRamp[0], uRamp[1], x) : x < 2.0 ? mix(uRamp[1], uRamp[2], x - 1.0) : mix(uRamp[2], uRamp[3], x - 2.0);
  }`

const stemVertex = /* glsl */ `
  ${SHARED}
  varying float vEdge;
  varying float vHalf;
  varying float vAlong;
  varying float vT;
  varying float vG;
  void main() {
    float g = grown();
    vec4 c0 = projectionMatrix * viewMatrix * vec4(aBase.xyz, 1.0);
    vec4 c1 = projectionMatrix * viewMatrix * vec4(aBase.xyz + vec3(0.0, max(aBase.w * g, 1e-4), 0.0), 1.0);
    vec2 dir = (c1.xy / max(c1.w, 1e-4) - c0.xy / max(c0.w, 1e-4)) * uRes;
    float l = length(dir);
    dir = l > 1e-6 ? dir / l : vec2(0.0, 1.0);
    vec4 c = position.y < 0.5 ? c0 : c1;
    float width = mix(1.4, 2.4, aInfo.x) * uPx;
    float reach = width * 0.5 + uPx;
    c.xy += vec2(-dir.y, dir.x) * position.x * reach * 2.0 / uRes * c.w;
    vEdge = position.x * reach;
    vHalf = width * 0.5;
    vAlong = position.y;
    vT = aInfo.x;
    vG = g;
    gl_Position = c;
  }`
const stemFragment = /* glsl */ `
  ${RAMP}
  uniform float uFade;
  varying float vEdge;
  varying float vHalf;
  varying float vAlong;
  varying float vT;
  varying float vG;
  void main() {
    float aa = max(fwidth(vEdge), 1e-3);
    float line = 1.0 - smoothstep(vHalf - 0.5 * aa, vHalf + 0.5 * aa, abs(vEdge));
    // a gradient, not a bar: faint where it leaves the ground, full where it carries the value
    float body = mix(0.28, 1.0, pow(vAlong, 1.2));
    gl_FragColor = vec4(ramp(vT), line * body * smoothstep(0.0, 0.2, vG) * uFade);
    #include <colorspace_fragment>
  }`

const headVertex = /* glsl */ `
  ${SHARED}
  varying vec2 vP;
  varying float vR;
  varying float vT;
  varying float vG;
  void main() {
    float g = grown();
    vec4 c = projectionMatrix * viewMatrix * vec4(aBase.xyz + vec3(0.0, aBase.w * g, 0.0), 1.0);
    float r = mix(1.5, 2.7, aInfo.x) * uPx * mix(0.6, 1.0, g);
    float reach = r + 2.6 * uPx;
    c.xy += position.xy * reach * 2.0 / uRes * c.w;
    vP = position.xy * reach;
    vR = r;
    vT = aInfo.x;
    vG = g;
    gl_Position = c;
  }`
const headFragment = /* glsl */ `
  ${RAMP}
  uniform float uFade;
  uniform float uPx;
  uniform vec3 uHalo;
  varying vec2 vP;
  varying float vR;
  varying float vT;
  varying float vG;
  void main() {
    float d = length(vP);
    float aa = max(fwidth(d), 1e-3);
    float disc = 1.0 - smoothstep(vR - 0.5 * aa, vR + 0.5 * aa, d);
    // a hairline of the page's own ground colour, so heads stay separate where needles crowd
    float halo = (1.0 - smoothstep(vR + 1.0 * uPx - 0.5 * aa, vR + 1.0 * uPx + 0.5 * aa, d)) * 0.6;
    float alpha = max(disc, halo);
    vec3 c = mix(uHalo, ramp(vT), clamp(disc / max(alpha, 1e-4), 0.0, 1.0));
    gl_FragColor = vec4(c, alpha * smoothstep(0.05, 0.4, vG) * uFade);
    #include <colorspace_fragment>
  }`

const ringVertex = /* glsl */ `
  ${SHARED}
  attribute vec3 aNrm;
  varying vec2 vUv;
  varying float vT;
  varying float vG;
  varying vec3 vWorld;
  void main() {
    float g = grown();
    // laid on the ground it stands on, so a hill town's ring tilts with the slope
    vec3 n = normalize(aNrm);
    vec3 tx = normalize(cross(vec3(0.0, 0.0, 1.0), n));
    vec3 tz = cross(n, tx);
    vec3 w = aBase.xyz + n * 0.0045 + (tx * position.x + tz * position.z) * aInfo.z * mix(0.5, 1.0, g);
    vUv = position.xz;
    vT = aInfo.x;
    vG = g;
    vWorld = w;
    gl_Position = projectionMatrix * viewMatrix * vec4(w, 1.0);
  }`
const ringFragment = /* glsl */ `
  ${RAMP}
  uniform float uFade, uRange, uK;
  uniform sampler2D uSdf;
  uniform vec4 uBounds;
  uniform vec2 uOrigin;
  varying vec2 vUv;
  varying float vT;
  varying float vG;
  varying vec3 vWorld;
  // signed distance to the coast in degrees, positive on land
  float landDist(vec2 p) {
    vec2 ll = vec2(p.x / uK + uOrigin.x, -p.y + uOrigin.y);
    vec2 uv = vec2((ll.x - uBounds.x) / (uBounds.y - uBounds.x), (uBounds.w - ll.y) / (uBounds.w - uBounds.z));
    float s = (texture2D(uSdf, clamp(uv, 0.0, 1.0)).r - 0.5) * 2.0;
    return sign(s) * s * s * uRange;
  }
  void main() {
    float d = length(vUv);
    float px = max(fwidth(d), 1e-4);
    float edge = 1.0 - 1.5 * px;
    float stroke = 1.0 - smoothstep(0.0, 1.0, abs(d - edge) / px - 0.25);
    float fill = (1.0 - smoothstep(edge - px, edge + px, d)) * mix(0.3, 0.12, d);
    float alpha = max(stroke * 0.62, fill) * smoothstep(0.0, 0.5, vG) * uFade * smoothstep(0.004, 0.02, landDist(vWorld.xz));
    gl_FragColor = vec4(ramp(vT), alpha);
    #include <colorspace_fragment>
  }`

const N = SCENE_THEMES.dark.population, D = SCENE_THEMES.light.population
const _size = new THREE.Vector2()
/** the uniforms all three materials share */
function write(m: THREE.ShaderMaterial | null, grow: number, fade: number, px: number, t: number) {
  const u = m?.uniforms
  if (!u) return
  u.uGrow.value = grow
  u.uFade.value = fade
  u.uPx.value = px
  u.uRes.value.copy(_size)
  for (let i = 0; i < 4; i++) tween(u.uRamp.value[i], N.ramp[i], D.ramp[i], t)
}

function build(places: Place[], terrain: Terrain) {
  const n = places.length
  const base = new Float32Array(n * 4), info = new Float32Array(n * 3), nrm = new Float32Array(n * 3)
  let max = 1, cx = 0, cz = 0
  for (const p of places) if (p.pop > max) {
    max = p.pop
    ;[cx, cz] = toXZ(p.lon, p.lat)
  }
  let far = 1e-6
  for (const p of places) {
    const [x, z] = toXZ(p.lon, p.lat)
    far = Math.max(far, Math.hypot(x - cx, z - cz))
  }
  places.forEach((p, i) => {
    const [x, z] = toXZ(p.lon, p.lat)
    const t = Math.sqrt(p.pop / max)
    // height runs a little steeper than √, so the cities stand clear of the towns without the towns vanishing
    base.set([x, terrain.heightAt(p.lon, p.lat), z, MIN_H + (MAX_H - MIN_H) * Math.pow(p.pop / max, 0.62)], i * 4)
    // radius ∝ √population: the ring's area is the number
    info.set([t, (Math.hypot(x - cx, z - cz) / far) * STAGGER, 0.011 + 0.05 * t], i * 3)
    const e = 0.02
    const gx = (terrain.surfaceAt(p.lon + e / K, p.lat) - terrain.surfaceAt(p.lon - e / K, p.lat)) / (2 * e)
    const gz = -(terrain.surfaceAt(p.lon, p.lat + e) - terrain.surfaceAt(p.lon, p.lat - e)) / (2 * e)
    // on the coast the difference reaches down the rim: keep the ring level there
    const flat = terrain.coastDistance(p.lon, p.lat) < 0.05
    const l = Math.hypot(gx, 1, gz)
    nrm.set(flat ? [0, 1, 0] : [-gx / l, 1 / l, -gz / l], i * 3)
  })
  const aBase = new THREE.InstancedBufferAttribute(base, 4), aInfo = new THREE.InstancedBufferAttribute(info, 3), aNrm = new THREE.InstancedBufferAttribute(nrm, 3)
  const make = (position: number[], index: number[]) => {
    const g = new THREE.InstancedBufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute(position, 3))
    g.setIndex(index)
    g.setAttribute('aBase', aBase)
    g.setAttribute('aInfo', aInfo)
    g.instanceCount = n
    return g
  }
  // stem: x = side, y = 0 foot … 1 head · head: a screen-space quad · ring: a quad on the ground
  const stem = make([-1, 0, 0, 1, 0, 0, 1, 1, 0, -1, 1, 0], [0, 1, 2, 0, 2, 3])
  const head = make([-1, -1, 0, 1, -1, 0, 1, 1, 0, -1, 1, 0], [0, 1, 2, 0, 2, 3])
  const ring = make([-1, 0, -1, 1, 0, -1, 1, 0, 1, -1, 0, 1], [0, 2, 1, 0, 3, 2])
  ring.setAttribute('aNrm', aNrm)
  return { stem, head, ring }
}

export function Population({ places, terrain, mix, show, animate }: { places: Place[]; terrain: Terrain; /** 0 = night, 1 = day */ mix: MutableRefObject<number>; show: boolean; animate: boolean }) {
  const built = useMemo(() => build(places, terrain), [places, terrain])
  useEffect(
    () => () => {
      built.stem.dispose()
      built.head.dispose()
      built.ring.dispose()
    },
    [built],
  )
  const group = useRef<THREE.Group>(null)
  const stemMat = useRef<THREE.ShaderMaterial>(null), headMat = useRef<THREE.ShaderMaterial>(null), ringMat = useRef<THREE.ShaderMaterial>(null)
  // Initial values only: the frame loop writes through the material refs.
  const uniforms = useMemo(() => {
    const b = terrain.meta.bounds
    const common = () => ({
      uGrow: { value: 0 }, uFade: { value: 0 }, uPx: { value: 1 }, uRes: { value: new THREE.Vector2(1, 1) },
      uRamp: { value: [new THREE.Color(), new THREE.Color(), new THREE.Color(), new THREE.Color()] },
    })
    return {
      stem: common(),
      head: { ...common(), uHalo: { value: new THREE.Color() } },
      ring: { ...common(), uSdf: { value: terrain.sdfTexture }, uBounds: { value: new THREE.Vector4(b.w, b.e, b.s, b.n) }, uRange: { value: terrain.meta.range }, uK: { value: K }, uOrigin: { value: new THREE.Vector2(ORIGIN.lon, ORIGIN.lat) } },
    }
  }, [terrain])
  const grow = useRef(0)
  const fade = useRef(0)

  useFrame(({ gl }, dt) => {
    const g = group.current
    if (!g) return
    const h = Math.min(dt, 0.05)
    if (!animate) {
      grow.current = fade.current = show ? 1 : 0
    } else if (show) {
      // arriving: the stagger plays once, over a little more than a second
      fade.current = 1
      grow.current = Math.min(1, grow.current + h / 1.3)
    } else {
      // leaving: the whole layer goes together, and faster than it came
      fade.current = Math.max(0, fade.current - h / 0.24)
      if (fade.current === 0) grow.current = 0
    }
    g.visible = fade.current > 0 && grow.current > 0
    if (!g.visible) return
    const t = mix.current
    const px = gl.getPixelRatio()
    gl.getDrawingBufferSize(_size)
    write(stemMat.current, grow.current, fade.current, px, t)
    write(headMat.current, grow.current, fade.current, px, t)
    write(ringMat.current, grow.current, fade.current, px, t)
    if (headMat.current) tween(headMat.current.uniforms.uHalo.value, N.halo, D.halo, t)
  })

  return (
    <group ref={group} visible={false}>
      <mesh geometry={built.ring} frustumCulled={false} renderOrder={1}>
        <shaderMaterial ref={ringMat} uniforms={uniforms.ring} vertexShader={ringVertex} fragmentShader={ringFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </mesh>
      <mesh geometry={built.stem} frustumCulled={false} renderOrder={3}>
        <shaderMaterial ref={stemMat} uniforms={uniforms.stem} vertexShader={stemVertex} fragmentShader={stemFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </mesh>
      <mesh geometry={built.head} frustumCulled={false} renderOrder={4}>
        <shaderMaterial ref={headMat} uniforms={uniforms.head} vertexShader={headVertex} fragmentShader={headFragment} transparent depthWrite={false} side={THREE.DoubleSide} toneMapped={false} />
      </mesh>
    </group>
  )
}
