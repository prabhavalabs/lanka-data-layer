// The real-time island. Lazy-loaded: three.js never touches the first paint.
import { Line } from '@react-three/drei'
import { Canvas, useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo, useRef, useState, type MutableRefObject, type ReactNode } from 'react'
import * as THREE from 'three'
import type { Landmark } from '@/api/types'
import { geo, type Place, type Polyline } from '@/lib/geo'
import { DRIFT, isWide, leanFor, LOOK, orbit, PARALLAX, poseCamera, RESPONSE, REST, springTo, type Focus } from './camera'
import { hazeRate } from './haze'
import { Landmarks } from './Landmarks'
import { Maritime } from './Maritime'
import type { Port } from './maritimeData'
import { Population } from './Population'
import { Rivers } from './Rivers'
import { SCENE_THEMES, SceneThemeContext } from './sceneTheme'
import { StageContext, useStage, type Anchor, type CardPlacement, type SafeRect, type Stage } from './stage'
import { BEVEL, buildTerrain, K, ORIGIN, PLINTH, SUMMIT, toXZ, type Terrain } from './terrain'
import { lerp, tween, tweenDir } from './tween'

export type Mood = 'night' | 'day'
export type HeroLayers = { landmarks: boolean; maritime: boolean; population: boolean; rail: boolean }

const N = SCENE_THEMES.dark, D = SCENE_THEMES.light

const _v = new THREE.Vector3()
const black = () => new THREE.Color(0, 0, 0)

type MixRef = MutableRefObject<number>

/** A whisper of noise after the colour transform: dark gradients this smooth band on 8-bit panels without it. */
const DITHER = /* glsl */ `
  gl_FragColor.rgb += (fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715)))) - 0.5) / 255.0;`

/* ---------------- sky ---------------- */
function Sky({ mix }: { mix: MixRef }) {
  const mat = useRef<THREE.ShaderMaterial>(null)
  // Initial values only. The reconciler copies each uniform into the material, so the frame
  // loop must write through the material: a number set on this object would never arrive.
  const uniforms = useMemo(() => ({ uZenith: { value: black() }, uHorizon: { value: black() }, uGlow: { value: black() }, uGlowA: { value: 0 }, uGlowDir: { value: new THREE.Vector3(0, 0, -1) } }), [])
  useFrame(() => {
    const u = mat.current?.uniforms
    if (!u) return
    const t = mix.current
    tween(u.uZenith.value, N.sky.zenith, D.sky.zenith, t)
    tween(u.uHorizon.value, N.sky.horizon, D.sky.horizon, t)
    tween(u.uGlow.value, N.sky.glow, D.sky.glow, t)
    u.uGlowA.value = lerp(N.sky.glowAmount, D.sky.glowAmount, t)
    tweenDir(u.uGlowDir.value, N.sky.glowDir, D.sky.glowDir, t)
  })
  return (
    <mesh renderOrder={-10} frustumCulled={false}>
      <sphereGeometry args={[60, 32, 20]} />
      <shaderMaterial
        ref={mat}
        side={THREE.BackSide}
        depthWrite={false}
        depthTest={false}
        fog={false}
        toneMapped={false}
        uniforms={uniforms}
        vertexShader={/* glsl */ `
          varying vec3 vDir;
          void main() {
            vDir = normalize(position);
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
          }`}
        fragmentShader={/* glsl */ `
          uniform vec3 uZenith, uHorizon, uGlow, uGlowDir;
          uniform float uGlowA;
          varying vec3 vDir;
          void main() {
            vec3 d = normalize(vDir);
            float h = clamp(d.y, 0.0, 1.0);
            // leaves the horizon gently, so the sea meets the sky without a drawn line
            vec3 c = mix(uHorizon, uZenith, smoothstep(0.0, 0.4, h));
            // one soft bloom on the horizon, behind the island
            vec2 az = normalize(d.xz + 1e-5);
            float toward = pow(max(dot(az, normalize(uGlowDir.xz)), 0.0), 2.0);
            c = mix(c, uGlow, exp(-h * 8.0) * uGlowA * (0.35 + 0.65 * toward));
            gl_FragColor = vec4(c, 1.0);
            #include <colorspace_fragment>
            ${DITHER}
          }`}
      />
    </mesh>
  )
}

/* ---------------- ocean ---------------- */
function Ocean({ terrain, mix, animate }: { terrain: Terrain; mix: MixRef; animate: boolean }) {
  const mat = useRef<THREE.ShaderMaterial>(null)
  const uniforms = useMemo(() => {
    const b = terrain.meta.bounds
    return {
      uTime: { value: 0 },
      uSdf: { value: terrain.sdfTexture },
      uBounds: { value: new THREE.Vector4(b.w, b.e, b.s, b.n) },
      uRange: { value: terrain.meta.range },
      uK: { value: K },
      uOrigin: { value: new THREE.Vector2(ORIGIN.lon, ORIGIN.lat) },
      uDeep: { value: black() }, uMid: { value: black() }, uShelf: { value: black() }, uLine: { value: black() },
      uReflect: { value: black() }, uHorizon: { value: black() }, uGlow: { value: black() }, uGlint: { value: black() },
      uKeyDir: { value: new THREE.Vector3(0, 1, 0) }, uGlowDir: { value: new THREE.Vector3(0, 0, -1) },
      uGlowA: { value: 0 }, uGlintA: { value: 0 }, uGrid: { value: 0 }, uHaze: { value: 0.05 },
    }
  }, [terrain])
  useFrame(({ clock, camera }) => {
    const u = mat.current?.uniforms
    if (!u) return
    const t = mix.current
    if (animate) u.uTime.value = clock.elapsedTime
    u.uHaze.value = hazeRate(camera)
    tween(u.uDeep.value, N.ocean.deep, D.ocean.deep, t)
    tween(u.uMid.value, N.ocean.mid, D.ocean.mid, t)
    tween(u.uShelf.value, N.ocean.shelf, D.ocean.shelf, t)
    tween(u.uLine.value, N.ocean.shoreline, D.ocean.shoreline, t)
    tween(u.uReflect.value, N.ocean.reflect, D.ocean.reflect, t)
    tween(u.uGlint.value, N.ocean.glint, D.ocean.glint, t)
    tween(u.uHorizon.value, N.sky.horizon, D.sky.horizon, t)
    tween(u.uGlow.value, N.sky.glow, D.sky.glow, t)
    tweenDir(u.uKeyDir.value, N.light.keyDir, D.light.keyDir, t)
    tweenDir(u.uGlowDir.value, N.sky.glowDir, D.sky.glowDir, t)
    u.uGlowA.value = lerp(N.sky.glowAmount, D.sky.glowAmount, t)
    u.uGlintA.value = lerp(N.ocean.glintAmount, D.ocean.glintAmount, t)
    u.uGrid.value = lerp(N.ocean.grid, D.ocean.grid, t)
  })
  // Drawn first and without writing depth: everything else in the scene sits on top of the sea,
  // so the land needs no skirt under the water and nothing on the surface can z-fight with it.
  return (
    <mesh rotation={[-Math.PI / 2, 0, 0]} frustumCulled={false} renderOrder={-5}>
      <planeGeometry args={[140, 140, 1, 1]} />
      <shaderMaterial
        ref={mat}
        uniforms={uniforms}
        depthWrite={false}
        toneMapped={false}
        vertexShader={/* glsl */ `
          varying vec3 vWorld;
          void main() {
            vec4 w = modelMatrix * vec4(position, 1.0);
            vWorld = w.xyz;
            gl_Position = projectionMatrix * viewMatrix * w;
          }`}
        fragmentShader={/* glsl */ `
          precision highp float;
          uniform float uTime, uRange, uK, uGrid, uGlowA, uGlintA, uHaze;
          uniform sampler2D uSdf;
          uniform vec4 uBounds;
          uniform vec2 uOrigin;
          uniform vec3 uDeep, uMid, uShelf, uLine, uReflect, uHorizon, uGlow, uGlint, uKeyDir, uGlowDir;
          varying vec3 vWorld;

          // signed distance to the coast in degrees, positive at sea
          float seaDist(vec2 ll) {
            vec2 uv = vec2((ll.x - uBounds.x) / (uBounds.y - uBounds.x), (uBounds.w - ll.y) / (uBounds.w - uBounds.z));
            float s = (texture2D(uSdf, clamp(uv, 0.0, 1.0)).r - 0.5) * 2.0;
            vec2 o = max(vec2(0.0), max(-uv, uv - 1.0));
            return -sign(s) * s * s * uRange + length(o) * 4.0;
          }
          // A few long swells crossing at shallow angles. The slope is analytic: no noise to
          // shimmer, nothing to alias, and a fraction of the cost of sampling a height field.
          vec2 swell(vec2 p, float t) {
            vec2 g = vec2(0.0);
            vec2 d;
            float ph;
            d = vec2(0.80, -0.60); ph = dot(p, d) * 9.5 + t * 0.50 + sin(dot(p, d.yx) * 3.1 + t * 0.07) * 0.6; g += d * cos(ph) * 0.011;
            d = vec2(0.26, -0.966); ph = dot(p, d) * 15.1 - t * 0.66 + sin(dot(p, d.yx) * 4.3 - t * 0.05) * 0.6 + 1.7; g += d * cos(ph) * 0.011;
            d = vec2(0.966, -0.26); ph = dot(p, d) * 24.7 + t * 0.92 + sin(dot(p, d.yx) * 6.1) * 0.7 + 4.1; g += d * cos(ph) * 0.011;
            d = vec2(-0.5, -0.866); ph = dot(p, d) * 39.0 + t * 1.2 + 2.3; g += d * cos(ph) * 0.007;
            return g;
          }

          void main() {
            vec2 p = vWorld.xz;
            vec2 ll = vec2(p.x / uK + uOrigin.x, -p.y + uOrigin.y);
            float d = seaDist(ll);
            vec3 toCam = cameraPosition - vWorld;
            float dist = length(toCam);
            vec3 v = toCam / dist;

            // depth: a pale shelf hugging the coast, open water, then the deep
            vec3 water = mix(uMid, uDeep, smoothstep(0.1, 0.85, d));
            float shelf = 1.0 - smoothstep(0.0, 0.13, d);
            water = mix(water, uShelf, shelf * shelf * 0.9);

            // the far water is flattened so it reads as a calm sheet and cannot moiré
            vec2 g = swell(p, uTime) * exp(-dist * uHaze * 1.7);
            vec3 n = normalize(vec3(-g.x, 1.0, -g.y));
            float fres = pow(1.0 - max(dot(n, v), 0.0), 3.0);
            vec3 c = mix(water, uReflect, fres * 0.5);
            // the key light on the water: one broad, soft path, no glitter
            c += uGlint * pow(max(dot(reflect(-v, n), normalize(uKeyDir)), 0.0), 16.0) * uGlintA;

            // survey graticule every half degree. It thins out as its cells get small on screen,
            // so it is a quiet texture near the island and never a mesh in the distance or on a phone.
            vec2 gw = max(fwidth(ll * 2.0), vec2(1e-6));
            vec2 gq = abs(fract(ll * 2.0 - 0.5) - 0.5) / gw;
            float grid = 1.0 - min(min(gq.x, gq.y), 1.0);
            float roomy = smoothstep(12.0, 42.0, 1.0 / max(gw.x, gw.y));
            c = mix(c, uLine, grid * uGrid * roomy * smoothstep(0.06, 0.16, d));

            // distance haze: the sea dissolves into exactly the colour the sky paints at the horizon
            float haze = 1.0 - exp(-pow(dist * uHaze, 2.2));
            vec2 az = normalize(vWorld.xz - cameraPosition.xz);
            float toward = pow(max(dot(az, normalize(uGlowDir.xz)), 0.0), 2.0);
            c = mix(c, mix(uHorizon, uGlow, uGlowA * (0.35 + 0.65 * toward)), clamp(haze, 0.0, 1.0));

            gl_FragColor = vec4(c, 1.0);
            #include <colorspace_fragment>
            ${DITHER}
          }`}
      />
    </mesh>
  )
}

/* ---------------- island ---------------- */
const f = (n: number) => n.toFixed(5)
function Island({ terrain, mix }: { terrain: Terrain; mix: MixRef }) {
  const uniforms = useMemo(
    () => ({
      uLow: { value: black() }, uPlain: { value: black() }, uHill: { value: black() }, uHigh: { value: black() }, uSummit: { value: black() },
      uRim: { value: black() }, uContour: { value: black() }, uContourA: { value: 0 },
    }),
    [],
  )
  const material = useMemo(() => {
    // matte, like a milled relief model: colour comes from elevation alone, form from the light
    const m = new THREE.MeshStandardMaterial({ roughness: 0.86, metalness: 0 })
    m.onBeforeCompile = (shader) => {
      Object.assign(shader.uniforms, uniforms)
      shader.vertexShader = shader.vertexShader
        .replace('#include <common>', '#include <common>\nattribute vec3 aShade;\nvarying float vElev;\nvarying float vSlope;\nvarying vec3 vShade;')
        .replace('#include <begin_vertex>', '#include <begin_vertex>\nvElev = position.y;\nvSlope = 1.0 - normal.y;\nvShade = aShade;')
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', '#include <common>\nuniform vec3 uLow, uPlain, uHill, uHigh, uSummit, uRim, uContour;\nuniform float uContourA;\nvarying float vElev;\nvarying float vSlope;\nvarying vec3 vShade;')
        .replace(
          '#include <color_fragment>',
          /* glsl */ `#include <color_fragment>
           float e = clamp((vElev - ${f(PLINTH)}) / ${f(SUMMIT - PLINTH)}, 0.0, 1.0);
           vec3 land = mix(uLow, uPlain, smoothstep(0.0, 0.045, e));
           land = mix(land, uHill, smoothstep(0.035, 0.34, e));
           land = mix(land, uHigh, smoothstep(0.3, 0.68, e));
           land = mix(land, uSummit, smoothstep(0.64, 1.0, e));
           // dry zone a shade paler and warmer than the wet south-west
           land *= 1.0 + vShade.y * vec3(0.07, 0.045, -0.02);
           // the foot of the plate's rounded edge takes its own pale tone: that band is the shoreline.
           // Keyed on distance from the water, not on height, so an islet too small for the full rim still has a green top.
           land = mix(uRim, land, smoothstep(0.1, 0.27, vShade.z));
           // hairline contours, only where the ground actually slopes
           float ce = (vElev - ${f(PLINTH)}) * ${f(1 / 0.0235)};
           float cl = abs(fract(ce - 0.5) - 0.5) / max(fwidth(ce), 1e-5);
           float contour = (1.0 - smoothstep(0.25, 1.1, cl)) * smoothstep(0.004, 0.022, vSlope) * smoothstep(0.012, 0.03, vElev - ${f(PLINTH)});
           land = mix(land, uContour, contour * uContourA);
           diffuseColor.rgb = land * vShade.x;`,
        )
    }
    return m
  }, [uniforms])
  useEffect(() => () => material.dispose(), [material])
  useFrame(() => {
    const t = mix.current
    tween(uniforms.uLow.value, N.land.low, D.land.low, t)
    tween(uniforms.uPlain.value, N.land.plain, D.land.plain, t)
    tween(uniforms.uHill.value, N.land.hill, D.land.hill, t)
    tween(uniforms.uHigh.value, N.land.high, D.land.high, t)
    tween(uniforms.uSummit.value, N.land.summit, D.land.summit, t)
    tween(uniforms.uRim.value, N.land.rim, D.land.rim, t)
    tween(uniforms.uContour.value, N.land.contour, D.land.contour, t)
    uniforms.uContourA.value = lerp(N.land.contourOpacity, D.land.contourOpacity, t)
  })
  return <mesh geometry={terrain.geometry} material={material} castShadow receiveShadow />
}

/* ---------------- draped lines (rail) ---------------- */
function Draped({ lines, terrain, color, width, opacity, lift = 0.006 }: { lines: Polyline[]; terrain: Terrain; color: string; width: number; opacity: number; lift?: number }) {
  const points = useMemo(() => {
    const out: [number, number, number][] = []
    const edge = BEVEL * 1.05
    // a point on the rim, moved up the distance field until it stands on the plain
    const inland = (lon: number, lat: number): [number, number] => {
      for (let it = 0; it < 4; it++) {
        const d = terrain.coastDistance(lon, lat)
        if (d >= edge) break
        const e = 0.012
        const gx = terrain.coastDistance(lon + e, lat) - terrain.coastDistance(lon - e, lat)
        const gy = terrain.coastDistance(lon, lat + e) - terrain.coastDistance(lon, lat - e)
        const l = Math.hypot(gx, gy) || 1
        lon += (gx / l) * (edge - d + 0.002)
        lat += (gy / l) * (edge - d + 0.002)
      }
      return [lon, lat]
    }
    for (const l of lines)
      for (let i = 0; i < l.length - 1; i++) {
        const a = l[i], b = l[i + 1]
        // drop stray segments that leave the island (ferry links, mainland stubs)
        if (terrain.coastDistance(a[0], a[1]) < -0.004 || terrain.coastDistance(b[0], b[1]) < -0.004) continue
        const n = Math.max(1, Math.ceil(Math.hypot((b[0] - a[0]) * K, b[1] - a[1]) / 0.03))
        for (let s = 0; s < n; s++) {
          // a line that runs along the shore is walked back onto the plain rather than down the rim
          const p0 = inland(a[0] + ((b[0] - a[0]) * s) / n, a[1] + ((b[1] - a[1]) * s) / n)
          const p1 = inland(a[0] + ((b[0] - a[0]) * (s + 1)) / n, a[1] + ((b[1] - a[1]) * (s + 1)) / n)
          const [x0, z0] = toXZ(p0[0], p0[1])
          const [x1, z1] = toXZ(p1[0], p1[1])
          out.push([x0, terrain.surfaceAt(p0[0], p0[1]) + lift, z0], [x1, terrain.surfaceAt(p1[0], p1[1]) + lift, z1])
        }
      }
    return out
  }, [lines, terrain, lift])
  if (!points.length) return null
  return <Line points={points} segments color={color} lineWidth={width} transparent opacity={opacity} toneMapped={false} depthWrite={false} />
}

/* ---------------- lights ---------------- */
// Three lights, the way a product is lit: one soft-shadowed key, a sky/ground fill,
// and a shadowless rim from behind that separates the far slopes from the sea.
function Lights({ mix, shadows }: { mix: MixRef; shadows: boolean }) {
  const key = useRef<THREE.DirectionalLight>(null)
  const rim = useRef<THREE.DirectionalLight>(null)
  const hemi = useRef<THREE.HemisphereLight>(null)
  const gl = useThree((s) => s.gl)
  const last = useRef(-1)
  useFrame(() => {
    const t = mix.current
    if (key.current) {
      key.current.position.copy(tweenDir(_v, N.light.keyDir, D.light.keyDir, t)).multiplyScalar(9)
      tween(key.current.color, N.light.keyColor, D.light.keyColor, t)
      key.current.intensity = lerp(N.light.keyIntensity, D.light.keyIntensity, t)
    }
    if (rim.current) {
      rim.current.position.copy(tweenDir(_v, N.light.rimDir, D.light.rimDir, t)).multiplyScalar(9)
      tween(rim.current.color, N.light.rimColor, D.light.rimColor, t)
      rim.current.intensity = lerp(N.light.rimIntensity, D.light.rimIntensity, t)
    }
    if (hemi.current) {
      tween(hemi.current.color, N.light.hemiSky, D.light.hemiSky, t)
      tween(hemi.current.groundColor, N.light.hemiGround, D.light.hemiGround, t)
      hemi.current.intensity = lerp(N.light.hemiIntensity, D.light.hemiIntensity, t)
    }
    gl.toneMappingExposure = lerp(N.light.exposure, D.light.exposure, t)
    // the scene is static: only re-render the shadow map while the key light is moving
    if (shadows && Math.abs(t - last.current) > 0.0005) {
      gl.shadowMap.needsUpdate = true
      last.current = t
    }
  })
  return (
    <>
      <hemisphereLight ref={hemi} />
      <directionalLight ref={rim} />
      <directionalLight
        ref={key}
        castShadow={shadows}
        shadow-mapSize={[2048, 2048]}
        shadow-radius={3.2}
        shadow-intensity={0.86}
        shadow-bias={-0.0005}
        shadow-normalBias={0.014}
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
/**
 * Design-review switch (dev only): `/?view=lon,lat[,lean[,az,polar]]` parks the camera on a
 * point, leaned in by `lean` (0 … 0.9), optionally from another bearing. No drift, no intro.
 */
const DEV_VIEW = import.meta.env.DEV
  ? (() => {
      const v = new URLSearchParams(window.location.search).get('view')?.split(',').map(Number)
      if (!v || v.length < 2 || v.some((n) => !Number.isFinite(n))) return null
      const [x, z] = toXZ(v[0], v[1])
      return { point: new THREE.Vector3(x, 0.03, z), lean: v[2] ?? 0.6, az: v[3], polar: v[4] }
    })()
  : null
const _rest = new THREE.Vector3()

function Rig({ animate, revealed, focus, onFirstFrame }: { animate: boolean; revealed: boolean; focus: Focus | null; onFirstFrame: () => void }) {
  const { camera, size } = useThree()
  const stage = useStage()
  const pointer = useRef({ x: 0, y: 0 })
  const eased = useRef({ x: 0, y: 0 })
  const first = useRef(false)
  // arrival: the camera starts high and wide, then settles onto its orbit as the scene fades in
  const intro = useRef(animate ? 0 : 1)
  // focus: how far the camera has leaned in, how far a tall screen has slid the subject to its spot, and the point it leans toward.
  // Springs, so a new target bends the move instead of restarting it.
  const lean = useRef({ x: 0, v: 0 })
  const pull = useRef({ x: 0, v: 0 })
  const px = useRef({ x: 0, v: 0 }), py = useRef({ x: 0, v: 0 }), pz = useRef({ x: 0, v: 0 })
  const point = useRef(new THREE.Vector3())

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
    const wide = isWide(size.width, size.height)
    if (wide) cam.setViewOffset(size.width, size.height, -size.width * 0.195, -size.height * 0.085, size.width, size.height)
    else cam.setViewOffset(size.width, size.height, 0, -size.height * 0.25, size.width, size.height)
    cam.fov = REST[wide ? 'wide' : 'tall'].fov
    cam.updateProjectionMatrix()
    return () => cam.clearViewOffset()
  }, [camera, size.width, size.height])

  useFrame(({ clock }, dt) => {
    const cam = camera as THREE.PerspectiveCamera
    const shape = isWide(size.width, size.height) ? 'wide' : 'tall'
    const rest = REST[shape]
    if (!first.current) {
      first.current = true
      onFirstFrame()
    }
    if (DEV_VIEW) {
      orbit(_rest, DEV_VIEW.az ?? rest.az, DEV_VIEW.polar ?? rest.polar, rest.r)
      poseCamera(cam, _rest, LOOK, DEV_VIEW.point, DEV_VIEW.lean, 0)
      return
    }
    const h = Math.min(dt, 0.05)
    const k = Math.min(1, dt * 2.2)
    eased.current.x += (pointer.current.x - eased.current.x) * k
    eased.current.y += (pointer.current.y - eased.current.y) * k
    const t = animate ? clock.elapsedTime : 0
    if (revealed && intro.current < 1) intro.current = Math.min(1, intro.current + h / INTRO_S)
    const away = (1 - intro.current) ** 4 // ease-out quart, measured from the resting pose
    const az = rest.az + (animate ? Math.sin(t * 0.07) * DRIFT.az + eased.current.x * PARALLAX.az : 0) - away * 0.34
    const polar = rest.polar + (animate ? Math.sin(t * 0.05 + 1.3) * DRIFT.polar + eased.current.y * PARALLAX.polar : 0) - away * 0.3
    // azimuth is measured from due south, turning toward the west
    orbit(_rest, az, polar, rest.r * (1 + away * 0.42))

    // lean toward whatever has the focus. Reduced motion renders on demand and never moves the camera.
    const a = animate && focus ? stage?.anchors.get(focus.id) : undefined
    const amount = a && focus ? leanFor(shape, focus.kind, a.h) : 0
    const response = a && focus && amount > 0 ? RESPONSE[focus.kind] : RESPONSE.release
    if (a && amount > 0) {
      const y = a.y + a.h * 0.45
      if (lean.current.x < 0.004) {
        // leaving rest: start from the new subject, not from wherever the last one was
        px.current.x = a.x
        py.current.x = y
        pz.current.x = a.z
        px.current.v = py.current.v = pz.current.v = 0
      } else {
        springTo(px.current, a.x, response, h)
        springTo(py.current, y, response, h)
        springTo(pz.current, a.z, response, h)
      }
    }
    springTo(lean.current, amount, response, h)
    springTo(pull.current, amount > 0 && shape === 'tall' ? 1 : 0, response, h)
    if (lean.current.x > 0.0005) poseCamera(cam, _rest, LOOK, point.current.set(px.current.x, py.current.x, pz.current.x), lean.current.x, pull.current.x)
    else {
      cam.position.copy(_rest)
      cam.lookAt(LOOK)
    }
  })
  return null
}

/* ---------------- scene ---------------- */
function Scene({
  terrain, landmarks, places, rail, rivers, mood, layers, active, focus, getSafe, onActive, onSelect, renderCard, port, onPort, renderPortCard, animate, lowPower, revealed, onFirstFrame,
}: {
  revealed: boolean
  port: string | null; onPort: (id: string | null) => void; renderPortCard: (port: Port, at: CardPlacement) => ReactNode
  terrain: Terrain; landmarks: Landmark[]; places: Place[]; rail: Polyline[]; rivers: Polyline[]
  mood: Mood; layers: HeroLayers; active: string | null; focus: Focus | null; getSafe: () => SafeRect | null
  onActive: (id: string | null) => void; onSelect: (id: string) => void
  renderCard: (lm: Landmark, at: CardPlacement) => ReactNode; animate: boolean; lowPower: boolean; onFirstFrame: () => void
}) {
  const mix = useRef(mood === 'day' ? 1 : 0)
  const theme = SCENE_THEMES[mood === 'day' ? 'light' : 'dark']
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

  // one registry of everything that stands on the scene, shared by the rig (what to lean toward) and the cards (what not to cover)
  const anchors = useMemo(() => new Map<string, Anchor>(), [])
  const stage = useMemo<Stage>(() => ({ anchors, safe: getSafe }), [anchors, getSafe])

  return (
    <SceneThemeContext.Provider value={theme}>
      <StageContext.Provider value={stage}>
        <Sky mix={mix} />
        <Lights mix={mix} shadows={!lowPower} />
        <Ocean terrain={terrain} mix={mix} animate={animate} />
        <Island terrain={terrain} mix={mix} />
        <Rivers lines={rivers} terrain={terrain} mix={mix} />
        {layers.rail && <Draped lines={rail} terrain={terrain} color={theme.land.rail} width={1.6} opacity={0.95} lift={0.009} />}
        <Maritime terrain={terrain} mix={mix} show={layers.maritime} animate={animate} lowPower={lowPower} activePort={port} focus={focus} onPort={onPort} renderPortCard={renderPortCard} />
        <Population places={places} terrain={terrain} mix={mix} show={layers.population} animate={animate} />
        {layers.landmarks && <Landmarks items={landmarks} terrain={terrain} active={active} focus={focus} animate={animate} glow={theme.landmark.glow} onActive={onActive} onSelect={onSelect} renderCard={renderCard} />}
        <Rig animate={animate} revealed={revealed} focus={focus} onFirstFrame={onFirstFrame} />
      </StageContext.Provider>
    </SceneThemeContext.Provider>
  )
}

export type HeroSceneProps = {
  landmarks: Landmark[]
  mood: Mood
  layers: HeroLayers
  active: string | null
  /** what the camera leans toward: a landmark id or `port:<id>`. Null: the camera rests. */
  focus: Focus | null
  /** the part of the canvas a card may open into, read each time a card opens */
  getSafe: () => SafeRect | null
  onActive: (id: string | null) => void
  onSelect: (id: string) => void
  renderCard: (lm: Landmark, at: CardPlacement) => ReactNode
  port: string | null
  onPort: (id: string | null) => void
  renderPortCard: (port: Port, at: CardPlacement) => ReactNode
  /** a click or tap that landed on nothing */
  onMiss: () => void
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

/**
 * Design-review switch (dev only): `/?hero=glfail` lets the availability probe in the shell pass and then
 * refuses the renderer its context, which is how a blocklisted or exhausted GPU actually fails.
 */
if (import.meta.env.DEV && new URLSearchParams(window.location.search).get('hero') === 'glfail') {
  const real = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, type: string, ...rest: unknown[]) {
    return /webgl/.test(type) ? null : (real as (...a: unknown[]) => unknown).call(this, type, ...rest)
  } as typeof real
}

/**
 * Phones start at a sharper pixel ratio than they used to and give it back if they cannot hold
 * the frame rate: two steps of a quarter, measured over fifty frames each, never while paused.
 */
function Governor() {
  const setDpr = useThree((s) => s.setDpr)
  const m = useRef({ n: 0, t: 0, skip: 45, steps: 0 })
  useFrame((state, dt) => {
    const a = m.current
    if (a.steps >= 2 || state.frameloop !== 'always' || dt > 0.25) return
    if (a.skip > 0) return void a.skip--
    a.n++
    a.t += dt
    if (a.n < 50) return
    const slow = a.t / a.n > 1 / 42
    a.n = a.t = 0
    if (!slow) return
    a.steps++
    a.skip = 30
    setDpr(Math.max(1, state.viewport.dpr - 0.25))
  })
  return null
}

export default function HeroScene(props: HeroSceneProps) {
  const { landmarks, mood, layers, active, focus, getSafe, onActive, onSelect, renderCard, port, onPort, renderPortCard, onMiss, onStage, animate, lowPower, revealed, paused, onReady, onError } = props
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
      shadows={lowPower ? false : 'percentage'}
      dpr={lowPower ? [1, 1.5] : [1, 1.75]}
      frameloop={paused ? 'never' : animate ? 'always' : 'demand'}
      camera={{ fov: 27, near: 0.1, far: 140, position: [-3.6, 4.6, 7.4] }}
      // Neutral tone mapping: albedo reaches the screen close to the hex it was authored as
      gl={{ antialias: true, powerPreference: 'high-performance', alpha: false, stencil: false, toneMapping: THREE.NeutralToneMapping }}
      onCreated={({ gl }) => {
        gl.shadowMap.autoUpdate = false
        gl.shadowMap.needsUpdate = true
        gl.setClearColor('#0b0e12')
        // a lost context (GPU reset, a phone reclaiming memory) hands over to the poster instead of leaving a blank canvas
        gl.domElement.addEventListener('webglcontextlost', onError, { once: true })
      }}
      fallback={null}
      onPointerMissed={onMiss}
      style={{ touchAction: 'pan-y' }}
    >
      <Scene {...assets} landmarks={landmarks} mood={mood} layers={layers} active={active} focus={focus} getSafe={getSafe} onActive={onActive} onSelect={onSelect} renderCard={renderCard} port={port} onPort={onPort} renderPortCard={renderPortCard} animate={animate} lowPower={lowPower} revealed={revealed} onFirstFrame={onReady} />
      {lowPower && <Governor />}
    </Canvas>
  )
}
