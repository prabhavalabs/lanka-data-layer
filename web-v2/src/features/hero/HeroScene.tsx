// The real-time island. Lazy-loaded: three.js never touches the first paint.
import { Line } from '@react-three/drei'
import { Canvas, useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo, useRef, useState, type MutableRefObject, type ReactNode } from 'react'
import * as THREE from 'three'
import type { Landmark } from '@/api/types'
import { geo, type Place, type Polyline } from '@/lib/geo'
import { Landmarks } from './Landmarks'
import { Maritime } from './Maritime'
import type { Port } from './maritimeData'
import { buildTerrain, K, ORIGIN, toXZ, type Terrain } from './terrain'

export type Mood = 'dusk' | 'day'
export type HeroLayers = { landmarks: boolean; maritime: boolean; population: boolean; rail: boolean }

type Palette = {
  zenith: string; horizon: string; glow: string; sunColor: string; sunIntensity: number; sunDir: [number, number, number]
  hemiSky: string; hemiGround: string; hemiIntensity: number
  deep: string; mid: string; shallow: string; foam: string; skyReflect: string
  cloud: string; cloudOpacity: number; exposure: number; grid: number
}
const PALETTES: Record<Mood, Palette> = {
  dusk: {
    zenith: '#0b0e12', horizon: '#2e2744', glow: '#ec7f5e', sunColor: '#ffb07a', sunIntensity: 3.2, sunDir: [-0.8, 0.4, 0.12],
    hemiSky: '#6a78b8', hemiGround: '#3a2a36', hemiIntensity: 1.25,
    deep: '#060d18', mid: '#0c2a44', shallow: '#2b9aa0', foam: '#ffe6d6', skyReflect: '#6d5a86',
    cloud: '#f2a58c', cloudOpacity: 0.34, exposure: 1.05, grid: 0.028,
  },
  day: {
    zenith: '#a9d2f2', horizon: '#eef6fb', glow: '#fff6e0', sunColor: '#fff3df', sunIntensity: 3.0, sunDir: [-0.42, 0.78, 0.38],
    hemiSky: '#cfe6ff', hemiGround: '#6d7a5a', hemiIntensity: 1.25,
    deep: '#0d5f93', mid: '#1a86b8', shallow: '#5fd6cf', foam: '#ffffff', skyReflect: '#dff0fb',
    cloud: '#ffffff', cloudOpacity: 0.7, exposure: 1.0, grid: 0.07,
  },
}

const col = (hex: string) => new THREE.Color(hex)
const mixCol = (out: THREE.Color, a: string, b: string, t: number) => out.set(a).lerp(_c.set(b), t)
const _c = new THREE.Color()
const _v = new THREE.Vector3()
const lerp = (a: number, b: number, t: number) => a + (b - a) * t
const sunDirAt = (t: number, out: THREE.Vector3) => out.set(lerp(PALETTES.dusk.sunDir[0], PALETTES.day.sunDir[0], t), lerp(PALETTES.dusk.sunDir[1], PALETTES.day.sunDir[1], t), lerp(PALETTES.dusk.sunDir[2], PALETTES.day.sunDir[2], t)).normalize()

type MixRef = MutableRefObject<number>

/* ---------------- sky ---------------- */
function Sky({ mix }: { mix: MixRef }) {
  const mat = useRef<THREE.ShaderMaterial>(null)
  const uniforms = useMemo(() => ({ uZenith: { value: col('#000') }, uHorizon: { value: col('#000') }, uGlow: { value: col('#000') }, uSunDir: { value: new THREE.Vector3() } }), [])
  useFrame(() => {
    const t = mix.current
    mixCol(uniforms.uZenith.value, PALETTES.dusk.zenith, PALETTES.day.zenith, t)
    mixCol(uniforms.uHorizon.value, PALETTES.dusk.horizon, PALETTES.day.horizon, t)
    mixCol(uniforms.uGlow.value, PALETTES.dusk.glow, PALETTES.day.glow, t)
    sunDirAt(t, uniforms.uSunDir.value)
  })
  return (
    <mesh renderOrder={-10} frustumCulled={false}>
      <sphereGeometry args={[60, 32, 20]} />
      <shaderMaterial
        ref={mat}
        side={THREE.BackSide}
        depthWrite={false}
        fog={false}
        uniforms={uniforms}
        vertexShader={/* glsl */ `
          varying vec3 vDir;
          void main() {
            vDir = normalize(position);
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
          }`}
        fragmentShader={/* glsl */ `
          uniform vec3 uZenith, uHorizon, uGlow, uSunDir;
          varying vec3 vDir;
          void main() {
            vec3 d = normalize(vDir);
            float h = clamp(d.y, 0.0, 1.0);
            vec3 c = mix(uHorizon, uZenith, pow(smoothstep(0.0, 0.42, h), 0.62));
            // warm band hugging the horizon, strongest toward the sun's azimuth
            vec2 az = normalize(d.xz + 1e-5);
            vec2 sunAz = normalize(uSunDir.xz);
            float toward = pow(max(dot(az, sunAz), 0.0), 2.2);
            float band = exp(-h * 7.0);
            c = mix(c, uGlow, band * (0.28 + 0.6 * toward));
            float disc = pow(max(dot(d, normalize(uSunDir)), 0.0), 220.0);
            c += uGlow * disc * 1.4;
            gl_FragColor = vec4(c, 1.0);
            #include <tonemapping_fragment>
            #include <colorspace_fragment>
          }`}
      />
    </mesh>
  )
}

/* ---------------- ocean ---------------- */
function Ocean({ terrain, mix, animate }: { terrain: Terrain; mix: MixRef; animate: boolean }) {
  const uniforms = useMemo(() => {
    const b = terrain.meta.bounds
    return {
      uTime: { value: 0 },
      uSdf: { value: terrain.sdfTexture },
      uBounds: { value: new THREE.Vector4(b.w, b.e, b.s, b.n) },
      uRange: { value: terrain.meta.range },
      uK: { value: K },
      uOrigin: { value: new THREE.Vector2(ORIGIN.lon, ORIGIN.lat) },
      uDeep: { value: col('#000') }, uMid: { value: col('#000') }, uShallow: { value: col('#000') }, uFoam: { value: col('#fff') },
      uSky: { value: col('#000') }, uHorizon: { value: col('#000') }, uGlow: { value: col('#000') }, uSunColor: { value: col('#fff') },
      uSunDir: { value: new THREE.Vector3(0, 1, 0) },
      uGrid: { value: 0.05 },
    }
  }, [terrain])
  useFrame(({ clock }) => {
    const t = mix.current
    if (animate) uniforms.uTime.value = clock.elapsedTime
    mixCol(uniforms.uDeep.value, PALETTES.dusk.deep, PALETTES.day.deep, t)
    mixCol(uniforms.uMid.value, PALETTES.dusk.mid, PALETTES.day.mid, t)
    mixCol(uniforms.uShallow.value, PALETTES.dusk.shallow, PALETTES.day.shallow, t)
    mixCol(uniforms.uFoam.value, PALETTES.dusk.foam, PALETTES.day.foam, t)
    mixCol(uniforms.uSky.value, PALETTES.dusk.skyReflect, PALETTES.day.skyReflect, t)
    mixCol(uniforms.uHorizon.value, PALETTES.dusk.horizon, PALETTES.day.horizon, t)
    mixCol(uniforms.uSunColor.value, PALETTES.dusk.glow, PALETTES.day.sunColor, t)
    mixCol(uniforms.uGlow.value, PALETTES.dusk.glow, PALETTES.day.glow, t)
    sunDirAt(t, uniforms.uSunDir.value)
    uniforms.uGrid.value = lerp(PALETTES.dusk.grid, PALETTES.day.grid, t)
  })
  return (
    <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0, 0]} frustumCulled={false}>
      <planeGeometry args={[140, 140, 1, 1]} />
      <shaderMaterial
        uniforms={uniforms}
        vertexShader={/* glsl */ `
          varying vec3 vWorld;
          void main() {
            vec4 w = modelMatrix * vec4(position, 1.0);
            vWorld = w.xyz;
            gl_Position = projectionMatrix * viewMatrix * w;
          }`}
        fragmentShader={/* glsl */ `
          precision highp float;
          uniform float uTime, uRange, uK, uGrid;
          uniform sampler2D uSdf;
          uniform vec4 uBounds;
          uniform vec2 uOrigin;
          uniform vec3 uDeep, uMid, uShallow, uFoam, uSky, uHorizon, uGlow, uSunColor, uSunDir;
          varying vec3 vWorld;

          float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
          float noise(vec2 p) {
            vec2 i = floor(p), f = fract(p);
            vec2 u = f * f * (3.0 - 2.0 * f);
            return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
          }
          float swell(vec2 p, float t) {
            float n = noise(p * 2.6 + vec2(t * 0.11, t * 0.07)) * 0.6;
            n += noise(p * 6.3 - vec2(t * 0.09, t * 0.14)) * 0.28;
            n += noise(p * 15.0 + vec2(t * 0.2, -t * 0.12)) * 0.12;
            return n;
          }
          // signed distance to the coast in degrees, positive at sea
          float seaDist(vec2 ll) {
            vec2 uv = vec2((ll.x - uBounds.x) / (uBounds.y - uBounds.x), (uBounds.w - ll.y) / (uBounds.w - uBounds.z));
            float v = texture2D(uSdf, clamp(uv, 0.0, 1.0)).r;
            float s = (v - 0.5) * 2.0;
            float d = -sign(s) * s * s * uRange;
            vec2 o = max(vec2(0.0), max(-uv, uv - 1.0));
            return d + length(o) * 4.0;
          }
          float caustic(vec2 p, float t) {
            vec2 q = p;
            float c = 0.0;
            for (int i = 0; i < 3; i++) {
              q += vec2(sin(q.y * 1.7 + t * 0.6 + float(i)), cos(q.x * 1.5 - t * 0.5 + float(i) * 1.7)) * 0.42;
              c += 1.0 / (abs(sin(q.x + q.y)) * 9.0 + 0.6);
            }
            return clamp(c * 0.22 - 0.16, 0.0, 1.0);
          }

          void main() {
            vec2 p = vWorld.xz;
            vec2 ll = vec2(p.x / uK + uOrigin.x, -p.y + uOrigin.y);
            float d = seaDist(ll);
            float t = uTime;

            // depth: turquoise shelf → mid → deep
            float shelf = smoothstep(0.0, 0.11, d);
            float deepness = smoothstep(0.06, 0.48, d);
            vec3 water = mix(uShallow, uMid, shelf);
            water = mix(water, uDeep, deepness);

            // surface normal from a small swell field
            float e = 0.012;
            float h0 = swell(p, t);
            float hx = swell(p + vec2(e, 0.0), t);
            float hz = swell(p + vec2(0.0, e), t);
            float amp = mix(0.035, 0.11, shelf);
            vec3 n = normalize(vec3(-(hx - h0) / e * amp, 1.0, -(hz - h0) / e * amp));
            vec3 v = normalize(cameraPosition - vWorld);

            float fres = pow(1.0 - max(dot(n, v), 0.0), 3.2);
            vec3 c = mix(water, uSky, fres * 0.62);

            // sun glitter
            vec3 hv = normalize(normalize(uSunDir) + v);
            float spec = pow(max(dot(n, hv), 0.0), 140.0);
            c += uSunColor * spec * 0.9;
            // long soft reflection path toward the sun
            float path = pow(max(dot(reflect(-v, n), normalize(uSunDir)), 0.0), 9.0);
            c += uSunColor * path * 0.2;

            // caustic light in the shallows
            float sh = (1.0 - smoothstep(0.0, 0.09, d));
            c += uShallow * caustic(p * 26.0, t) * sh * 0.55;

            // foam: lines that roll toward shore, plus a soft rim at the waterline
            float br = noise(p * 11.0 + t * 0.05);
            float roll = sin(d * 250.0 + t * 1.15 + br * 5.0);
            float lines = smoothstep(0.72, 0.98, roll) * (1.0 - smoothstep(0.004, 0.05, d)) * smoothstep(0.25, 0.7, br + 0.25);
            float rim = 1.0 - smoothstep(0.0, 0.0065 + 0.003 * sin(t * 0.8 + br * 6.0), d);
            c = mix(c, uFoam, clamp(lines * 0.5 + rim * 0.9, 0.0, 1.0));

            // survey graticule every half degree
            vec2 g = abs(fract(ll * 2.0 - 0.5) - 0.5) / fwidth(ll * 2.0);
            float grid = 1.0 - min(min(g.x, g.y), 1.0);
            c = mix(c, uFoam, grid * uGrid * smoothstep(0.02, 0.08, d));

            // distance haze into the horizon
            float dist = length(cameraPosition - vWorld);
            float haze = 1.0 - exp(-pow(dist * 0.05, 2.2));
            // same horizon colour the sky dome paints, so the sea dissolves into it
            vec2 az = normalize(vWorld.xz - cameraPosition.xz);
            float toward = pow(max(dot(az, normalize(uSunDir.xz)), 0.0), 2.2);
            vec3 hzc = mix(uHorizon, uGlow, 0.28 + 0.6 * toward);
            c = mix(c, hzc, clamp(haze, 0.0, 1.0));

            gl_FragColor = vec4(c, 1.0);
            #include <tonemapping_fragment>
            #include <colorspace_fragment>
          }`}
      />
    </mesh>
  )
}

/* ---------------- island ---------------- */
function Island({ terrain }: { terrain: Terrain }) {
  const material = useMemo(() => {
    const m = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.92, metalness: 0 })
    // faint contour lines: the terrain reads as a survey sheet, not an illustration
    m.onBeforeCompile = (shader) => {
      shader.vertexShader = shader.vertexShader.replace('#include <common>', '#include <common>\nvarying float vElev;').replace('#include <begin_vertex>', '#include <begin_vertex>\nvElev = position.y;')
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', '#include <common>\nvarying float vElev;')
        .replace(
          '#include <color_fragment>',
          `#include <color_fragment>
           float ce = vElev * 42.0;
           float cl = abs(fract(ce - 0.5) - 0.5) / max(fwidth(ce), 1e-4);
           float contour = (1.0 - smoothstep(0.0, 1.3, cl)) * smoothstep(0.02, 0.05, vElev);
           diffuseColor.rgb *= 1.0 - contour * 0.2;`,
        )
    }
    return m
  }, [])
  useEffect(() => () => material.dispose(), [material])
  return <mesh geometry={terrain.geometry} material={material} castShadow receiveShadow />
}

/* ---------------- draped lines (rivers, rail) ---------------- */
function Draped({ lines, terrain, color, width, opacity, lift = 0.006 }: { lines: Polyline[]; terrain: Terrain; color: string; width: number; opacity: number; lift?: number }) {
  const points = useMemo(() => {
    const out: [number, number, number][] = []
    for (const l of lines)
      for (let i = 0; i < l.length - 1; i++) {
        const a = l[i], b = l[i + 1]
        // drop stray segments that leave the island (ferry links, mainland stubs)
        if (terrain.coastDistance(a[0], a[1]) < -0.004 || terrain.coastDistance(b[0], b[1]) < -0.004) continue
        const n = Math.max(1, Math.ceil(Math.hypot((b[0] - a[0]) * K, b[1] - a[1]) / 0.03))
        for (let s = 0; s < n; s++) {
          const p0 = [a[0] + ((b[0] - a[0]) * s) / n, a[1] + ((b[1] - a[1]) * s) / n]
          const p1 = [a[0] + ((b[0] - a[0]) * (s + 1)) / n, a[1] + ((b[1] - a[1]) * (s + 1)) / n]
          const [x0, z0] = toXZ(p0[0], p0[1])
          const [x1, z1] = toXZ(p1[0], p1[1])
          out.push([x0, terrain.heightAt(p0[0], p0[1]) + lift, z0], [x1, terrain.heightAt(p1[0], p1[1]) + lift, z1])
        }
      }
    return out
  }, [lines, terrain, lift])
  if (!points.length) return null
  return <Line points={points} segments color={color} lineWidth={width} transparent opacity={opacity} toneMapped={false} depthWrite={false} />
}

/* ---------------- population columns ---------------- */
function PopulationColumns({ places, terrain, show }: { places: Place[]; terrain: Terrain; show: boolean }) {
  const ref = useRef<THREE.InstancedMesh>(null)
  const grow = useRef(0)
  const data = useMemo(
    () =>
      places.slice(0, 70).map((p, i) => {
        const [x, z] = toXZ(p.lon, p.lat)
        return { x, z, y: terrain.heightAt(p.lon, p.lat), h: 0.035 + Math.sqrt(p.pop) / 1900, order: i / 70, pop: p.pop }
      }),
    [places, terrain],
  )
  const dummy = useMemo(() => new THREE.Object3D(), [])
  const gl = useThree((s) => s.gl)
  useEffect(() => {
    const m = ref.current
    if (!m) return
    const ramp = ['#6650cf', '#8870e6', '#ad9bf2', '#d9d0fb']
    data.forEach((d, i) => m.setColorAt(i, _c.set(ramp[d.pop > 300000 ? 3 : d.pop > 100000 ? 2 : d.pop > 40000 ? 1 : 0])))
    if (m.instanceColor) m.instanceColor.needsUpdate = true
  }, [data])
  useFrame((_, dt) => {
    const m = ref.current
    if (!m) return
    const target = show ? 1 : 0
    if (Math.abs(grow.current - target) < 0.001 && m.userData.settled === target) return
    grow.current += (target - grow.current) * Math.min(1, dt * (show ? 2.6 : 7))
    if (Math.abs(grow.current - target) < 0.001) {
      grow.current = target
      m.userData.settled = target
    } else m.userData.settled = -1
    data.forEach((d, i) => {
      // tallest first, the rest follow: a short stagger rather than everything at once
      const k = THREE.MathUtils.clamp((grow.current - d.order * 0.45) / 0.55, 0, 1)
      const e = 1 - Math.pow(1 - k, 3)
      dummy.position.set(d.x, d.y + (d.h * e) / 2, d.z)
      dummy.scale.set(1, Math.max(0.0001, d.h * e), 1)
      dummy.updateMatrix()
      m.setMatrixAt(i, dummy.matrix)
    })
    m.instanceMatrix.needsUpdate = true
    m.visible = grow.current > 0.002
    gl.shadowMap.needsUpdate = true
  })
  return (
    <instancedMesh ref={ref} args={[undefined, undefined, data.length]} visible={false} castShadow>
      <cylinderGeometry args={[0.0105, 0.0105, 1, 6]} />
      <meshStandardMaterial roughness={0.4} emissive="#6046c0" emissiveIntensity={0.55} transparent opacity={0.92} />
    </instancedMesh>
  )
}

/* ---------------- clouds ---------------- */
function Clouds({ mix, count, animate }: { mix: MixRef; count: number; animate: boolean }) {
  const group = useRef<THREE.Group>(null)
  const tex = useMemo(() => {
    const c = document.createElement('canvas')
    c.width = c.height = 256
    const g = c.getContext('2d')!
    for (let i = 0; i < 14; i++) {
      const x = 60 + Math.random() * 136, y = 100 + Math.random() * 56, r = 26 + Math.random() * 46
      const gr = g.createRadialGradient(x, y, 0, x, y, r)
      gr.addColorStop(0, 'rgba(255,255,255,0.5)')
      gr.addColorStop(1, 'rgba(255,255,255,0)')
      g.fillStyle = gr
      g.fillRect(0, 0, 256, 256)
    }
    const t = new THREE.CanvasTexture(c)
    t.colorSpace = THREE.SRGBColorSpace
    return t
  }, [])
  const material = useMemo(() => new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false }), [tex])
  const seeds = useMemo(
    () => Array.from({ length: count }, (_, i) => ({ x: -11 + ((i * 5.37) % 22), y: 1.5 + ((i * 0.77) % 2.2), z: -9 - ((i * 2.93) % 9), s: 2.6 + ((i * 0.91) % 2.4), v: 0.03 + ((i * 0.011) % 0.03) })),
    [count],
  )
  useFrame((_, dt) => {
    const t = mix.current
    material.color.set(PALETTES.dusk.cloud).lerp(_c.set(PALETTES.day.cloud), t)
    material.opacity = lerp(PALETTES.dusk.cloudOpacity, PALETTES.day.cloudOpacity, t)
    if (!animate || !group.current) return
    for (const s of group.current.children) {
      s.position.x += (s.userData.v as number) * dt
      if (s.position.x > 13) s.position.x = -13
    }
  })
  return (
    <group ref={group}>
      {seeds.map((s, i) => (
        <sprite key={i} material={material} position={[s.x, s.y, s.z]} scale={[s.s * 2.1, s.s * 0.62, 1]} userData={{ v: s.v }} />
      ))}
    </group>
  )
}

/* ---------------- lights ---------------- */
function Lights({ mix, shadows }: { mix: MixRef; shadows: boolean }) {
  const sun = useRef<THREE.DirectionalLight>(null)
  const hemi = useRef<THREE.HemisphereLight>(null)
  const gl = useThree((s) => s.gl)
  const last = useRef(-1)
  useFrame(() => {
    const t = mix.current
    if (sun.current) {
      sunDirAt(t, _v)
      sun.current.position.copy(_v).multiplyScalar(9)
      sun.current.color.set(PALETTES.dusk.sunColor).lerp(_c.set(PALETTES.day.sunColor), t)
      sun.current.intensity = lerp(PALETTES.dusk.sunIntensity, PALETTES.day.sunIntensity, t)
    }
    if (hemi.current) {
      hemi.current.color.set(PALETTES.dusk.hemiSky).lerp(_c.set(PALETTES.day.hemiSky), t)
      hemi.current.groundColor.set(PALETTES.dusk.hemiGround).lerp(_c.set(PALETTES.day.hemiGround), t)
      hemi.current.intensity = lerp(PALETTES.dusk.hemiIntensity, PALETTES.day.hemiIntensity, t)
    }
    gl.toneMappingExposure = lerp(PALETTES.dusk.exposure, PALETTES.day.exposure, t)
    // the scene is static: only re-render the shadow map while the sun is moving
    if (shadows && Math.abs(t - last.current) > 0.0005) {
      gl.shadowMap.needsUpdate = true
      last.current = t
    }
  })
  return (
    <>
      <hemisphereLight ref={hemi} />
      <directionalLight
        ref={sun}
        castShadow={shadows}
        shadow-mapSize={[2048, 2048]}
        shadow-bias={-0.0006}
        shadow-normalBias={0.012}
        shadow-camera-left={-2.6}
        shadow-camera-right={2.6}
        shadow-camera-top={2.9}
        shadow-camera-bottom={-2.9}
        shadow-camera-near={1}
        shadow-camera-far={20}
      />
    </>
  )
}

/* ---------------- camera rig ---------------- */
const INTRO_S = 3.2
function Rig({ animate, revealed, onFirstFrame }: { animate: boolean; revealed: boolean; onFirstFrame: () => void }) {
  const { camera, size } = useThree()
  const pointer = useRef({ x: 0, y: 0 })
  const eased = useRef({ x: 0, y: 0 })
  const first = useRef(false)
  // arrival: the camera starts high and wide, then settles onto its orbit as the scene fades in
  const intro = useRef(animate ? 0 : 1)

  useEffect(() => {
    const on = (e: PointerEvent) => {
      if (e.pointerType === 'touch') return
      pointer.current.x = (e.clientX / window.innerWidth) * 2 - 1
      pointer.current.y = (e.clientY / window.innerHeight) * 2 - 1
    }
    window.addEventListener('pointermove', on, { passive: true })
    return () => window.removeEventListener('pointermove', on)
  }, [])

  // wide screens: push the island to the right so the headline has clear sky
  useEffect(() => {
    const cam = camera as THREE.PerspectiveCamera
    const wide = size.width / size.height > 1.15
    if (wide) cam.setViewOffset(size.width, size.height, -size.width * 0.195, -size.height * 0.085, size.width, size.height)
    else cam.setViewOffset(size.width, size.height, 0, -size.height * 0.27, size.width, size.height)
    cam.fov = wide ? 44 : 50
    cam.updateProjectionMatrix()
    return () => cam.clearViewOffset()
  }, [camera, size.width, size.height])

  useFrame(({ clock }, dt) => {
    const wide = size.width / size.height > 1.15
    const k = Math.min(1, dt * 2.2)
    eased.current.x += (pointer.current.x - eased.current.x) * k
    eased.current.y += (pointer.current.y - eased.current.y) * k
    const t = animate ? clock.elapsedTime : 0
    if (revealed && intro.current < 1) intro.current = Math.min(1, intro.current + Math.min(dt, 0.05) / INTRO_S)
    const away = (1 - intro.current) ** 4 // ease-out quart, measured from the resting pose
    const az = (wide ? 0.5 : 0.3) + (animate ? Math.sin(t * 0.07) * 0.075 + eased.current.x * 0.05 : 0) - away * 0.34
    const polar = (wide ? 1.135 : 1.02) + (animate ? Math.sin(t * 0.05 + 1.3) * 0.012 + eased.current.y * 0.018 : 0) - away * 0.3
    const r = (wide ? 5.7 : 8.6) * (1 + away * 0.42)
    // azimuth is measured from due south, turning toward the west
    camera.position.set(-Math.sin(az) * Math.sin(polar) * r, Math.cos(polar) * r, Math.cos(az) * Math.sin(polar) * r + 0.1)
    camera.lookAt(0.04, 0.02, 0.1)
    if (!first.current) {
      first.current = true
      onFirstFrame()
    }
  })
  return null
}

/* ---------------- scene ---------------- */
function Scene({
  terrain, landmarks, places, rail, rivers, mood, layers, active, onActive, onSelect, renderCard, port, onPort, renderPortCard, animate, lowPower, revealed, onFirstFrame,
}: {
  revealed: boolean
  port: string | null; onPort: (id: string | null) => void; renderPortCard: (port: Port) => ReactNode
  terrain: Terrain; landmarks: Landmark[]; places: Place[]; rail: Polyline[]; rivers: Polyline[]
  mood: Mood; layers: HeroLayers; active: string | null; onActive: (id: string | null) => void; onSelect: (id: string) => void
  renderCard: (lm: Landmark) => ReactNode; animate: boolean; lowPower: boolean; onFirstFrame: () => void
}) {
  const mix = useRef(mood === 'day' ? 1 : 0)
  const invalidate = useThree((s) => s.invalidate)
  useFrame((_, dt) => {
    const target = mood === 'day' ? 1 : 0
    if (mix.current === target) return
    const step = animate ? dt / 1.1 : 1
    mix.current = target > mix.current ? Math.min(target, mix.current + step) : Math.max(target, mix.current - step)
    invalidate()
  }, -1)
  // static render mode (reduced motion): redraw only when something changes
  const gl = useThree((s) => s.gl)
  useEffect(() => {
    gl.shadowMap.needsUpdate = true
    invalidate()
  }, [mood, layers, active, port, invalidate, gl])

  return (
    <>
      <Sky mix={mix} />
      <Lights mix={mix} shadows={!lowPower} />
      <Ocean terrain={terrain} mix={mix} animate={animate} />
      <Island terrain={terrain} />
      <Draped lines={rivers} terrain={terrain} color={mood === 'day' ? '#3b8fd0' : '#7fd0e8'} width={1} opacity={mood === 'day' ? 0.6 : 0.3} lift={0.004} />
      {layers.rail && <Draped lines={rail} terrain={terrain} color={mood === 'day' ? '#b84a12' : '#ffb766'} width={1.7} opacity={0.95} lift={0.011} />}
      <Maritime terrain={terrain} mix={mix} show={layers.maritime} animate={animate} lowPower={lowPower} activePort={port} onPort={onPort} renderPortCard={renderPortCard} />
      <PopulationColumns places={places} terrain={terrain} show={layers.population} />
      {layers.landmarks && <Landmarks items={landmarks} terrain={terrain} active={active} glow={mood === 'dusk' ? 1 : 0} onActive={onActive} onSelect={onSelect} renderCard={renderCard} />}
      <Clouds mix={mix} count={lowPower ? 4 : 9} animate={animate} />
      <Rig animate={animate} revealed={revealed} onFirstFrame={onFirstFrame} />
    </>
  )
}

export type HeroSceneProps = {
  landmarks: Landmark[]
  mood: Mood
  layers: HeroLayers
  active: string | null
  onActive: (id: string | null) => void
  onSelect: (id: string) => void
  renderCard: (lm: Landmark) => ReactNode
  port: string | null
  onPort: (id: string | null) => void
  renderPortCard: (port: Port) => ReactNode
  /** load milestones for the loader's status line: 1 fetching, 2 building, 3 compiling */
  onStage: (stage: number) => void
  /** false → static render (prefers-reduced-motion) */
  animate: boolean
  lowPower: boolean
  /** the loader has handed over: start the camera's arrival move */
  revealed: boolean
  /** pause the render loop entirely (hero scrolled out of view / tab hidden) */
  paused: boolean
  onReady: () => void
  onError: () => void
}

export default function HeroScene(props: HeroSceneProps) {
  const { landmarks, mood, layers, active, onActive, onSelect, renderCard, port, onPort, renderPortCard, onStage, animate, lowPower, revealed, paused, onReady, onError } = props
  const [assets, setAssets] = useState<{ terrain: Terrain; places: Place[]; rail: Polyline[]; rivers: Polyline[] } | null>(null)

  useEffect(() => {
    let alive = true
    onStage(1)
    Promise.all([buildTerrain(lowPower ? 'low' : 'high', () => alive && onStage(2)), geo.places(), geo.rail(), geo.rivers()])
      .then(([terrain, places, rail, rivers]) => {
        if (!alive) return
        onStage(3)
        setAssets({ terrain, places, rail, rivers })
      })
      .catch(() => alive && onError())
    return () => {
      alive = false
    }
  }, [lowPower, onError, onStage])

  useEffect(
    () => () => {
      assets?.terrain.geometry.dispose()
      assets?.terrain.sdfTexture.dispose()
    },
    [assets],
  )

  if (!assets) return null
  return (
    <Canvas
      shadows={lowPower ? false : 'soft'}
      dpr={lowPower ? [1, 1.25] : [1, 1.75]}
      frameloop={paused ? 'never' : animate ? 'always' : 'demand'}
      camera={{ fov: 27, near: 0.1, far: 140, position: [-3.6, 4.6, 7.4] }}
      gl={{ antialias: true, powerPreference: 'high-performance', alpha: false, stencil: false }}
      onCreated={({ gl }) => {
        gl.shadowMap.autoUpdate = false
        gl.shadowMap.needsUpdate = true
        gl.setClearColor('#0b0e12')
      }}
      fallback={null}
      onPointerMissed={() => onPort(null)}
      style={{ touchAction: 'pan-y' }}
    >
      <Scene {...assets} landmarks={landmarks} mood={mood} layers={layers} active={active} onActive={onActive} onSelect={onSelect} renderCard={renderCard} port={port} onPort={onPort} renderPortCard={renderPortCard} animate={animate} lowPower={lowPower} revealed={revealed} onFirstFrame={onReady} />
    </Canvas>
  )
}
