// Close-up review harness for the landmark miniatures (dev only).
// Each model stands alone on a turntable, lit exactly as the hero lights it for
// the chosen theme, with the reference notes it has to answer to beside it.
import { OrbitControls } from '@react-three/drei'
import { Canvas, useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo, useRef, useState } from 'react'
import * as THREE from 'three'
import { LANDMARKS } from '@/mocks/db'
import { Landmarks } from '../../Landmarks'
import { SCENE_THEMES, SceneThemeContext, type SceneTheme, type SceneThemeName } from '../../sceneTheme'
import { buildTerrain, toXZ, type Terrain } from '../../terrain'
import { getBuilt, LandmarkMesh, useLandmarkClock } from '../LandmarkMesh'
import { MODELS } from '../models'

const q = new URLSearchParams(window.location.search)
const num = (k: string, d: number) => (q.has(k) && Number.isFinite(Number(q.get(k))) ? Number(q.get(k)) : d)
const THEME: SceneThemeName = q.get('theme') === 'light' ? 'light' : 'dark'
const SEA = -0.024 // terrain.ts PLINTH: the plate stands this far above the sea
/** the hero camera sits ~25° above the horizon */
const HERO_ELEV = 25

function Stage({ id, theme, rot, elev, zoom, spin }: { id: string; theme: SceneTheme; rot: number; elev: number; zoom: number; spin: boolean }) {
  const def = MODELS[id]
  const built = useMemo(() => getBuilt(id, SEA), [id])
  const turntable = useRef<THREE.Group>(null)
  const { camera, gl } = useThree()
  useLandmarkClock(theme.name)

  const bounds = useMemo(() => {
    const box = built.body.boundingBox!.clone()
    box.min.y = Math.max(box.min.y, SEA)
    const c = box.getCenter(new THREE.Vector3())
    const r = box.getSize(new THREE.Vector3()).length() / 2
    return { c, r }
  }, [built])

  useEffect(() => {
    gl.toneMapping = THREE.NeutralToneMapping
    gl.toneMappingExposure = theme.light.exposure
    const cam = camera as THREE.PerspectiveCamera
    // hero view: the scene turns the model by `yaw`, so from the model's frame the camera is at −yaw
    const az = (def.view !== undefined ? (def.view * Math.PI) / 180 : -(def.yaw ?? 0)) + (rot * Math.PI) / 180
    const el = (elev * Math.PI) / 180
    const d = (bounds.r / Math.sin((cam.fov * Math.PI) / 360)) * 0.78 / zoom
    cam.position.set(bounds.c.x + Math.sin(az) * Math.cos(el) * d, bounds.c.y + Math.sin(el) * d, bounds.c.z + Math.cos(az) * Math.cos(el) * d)
    cam.near = d / 50
    cam.far = d * 20
    cam.lookAt(bounds.c)
    cam.updateProjectionMatrix()
  }, [camera, gl, def, bounds, rot, elev, zoom, theme])

  useFrame((_, dt) => {
    if (spin && turntable.current) turntable.current.rotation.y += dt * 0.35
  })

  const L = theme.light
  const coastal = def.shore !== undefined
  return (
    <>
      <color attach="background" args={[theme.sky.horizon]} />
      <hemisphereLight args={[L.hemiSky, L.hemiGround, L.hemiIntensity]} />
      <directionalLight
        position={[L.keyDir[0] * 4, L.keyDir[1] * 4, L.keyDir[2] * 4]}
        color={L.keyColor}
        intensity={L.keyIntensity}
        castShadow
        shadow-mapSize={[2048, 2048]}
        shadow-bias={-0.0004}
        shadow-normalBias={0.004}
        shadow-camera-left={-0.6}
        shadow-camera-right={0.6}
        shadow-camera-top={0.6}
        shadow-camera-bottom={-0.6}
        shadow-camera-near={0.5}
        shadow-camera-far={9}
      />
      <group ref={turntable}>
        <LandmarkMesh id={id} sea={SEA} />
        {/* the ground it would stand on: the plate for inland models, plate + sea for coastal ones */}
        {coastal ? (
          <>
            <mesh position={[0, SEA / 2, def.shore! - 1.5]} receiveShadow>
              <boxGeometry args={[3, -SEA, 3]} />
              <meshStandardMaterial color={theme.land.low} roughness={0.86} />
            </mesh>
            <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, SEA, 0]} receiveShadow>
              <circleGeometry args={[4, 48]} />
              <meshStandardMaterial color={theme.ocean.shelf} roughness={0.3} />
            </mesh>
          </>
        ) : (
          <mesh rotation={[-Math.PI / 2, 0, 0]} receiveShadow>
            <circleGeometry args={[4, 48]} />
            <meshStandardMaterial color={theme.land.plain} roughness={0.86} />
          </mesh>
        )}
      </group>
      <OrbitControls makeDefault target={bounds.c} enableDamping={false} />
    </>
  )
}

/**
 * `&scene=1`: the model where it really stands. The real island (flat-coloured),
 * the real <Landmarks> layer with its placement, draping and coast snapping, and
 * a camera on the hero's bearing but close in.
 */
function InScene({ id, theme, rot, elev, zoom }: { id: string; theme: SceneTheme; rot: number; elev: number; zoom: number }) {
  const [terrain, setTerrain] = useState<Terrain | null>(null)
  const { camera, gl } = useThree()
  useEffect(() => {
    let alive = true
    buildTerrain('high').then((t) => alive && setTerrain(t))
    return () => {
      alive = false
    }
  }, [])
  const lm = LANDMARKS.find((l) => l.id === id)!
  useEffect(() => {
    gl.toneMapping = THREE.NeutralToneMapping
    const [x, z] = toXZ(lm.lon, lm.lat)
    const az = 0.5 + (rot * Math.PI) / 180, el = (elev * Math.PI) / 180
    const d = 1.5 / zoom
    const target = new THREE.Vector3(x, 0.08, z)
    camera.position.set(x - Math.sin(az) * Math.cos(el) * d, 0.08 + Math.sin(el) * d, z + Math.cos(az) * Math.cos(el) * d)
    camera.near = 0.02
    camera.far = 60
    camera.lookAt(target)
    camera.updateProjectionMatrix()
  }, [camera, gl, lm, rot, elev, zoom])
  const L = theme.light
  const [tx, tz] = toXZ(lm.lon, lm.lat)
  return (
    <SceneThemeContext.Provider value={theme}>
      <color attach="background" args={[theme.sky.horizon]} />
      <hemisphereLight args={[L.hemiSky, L.hemiGround, L.hemiIntensity]} />
      <directionalLight
        position={[tx + L.keyDir[0] * 6, L.keyDir[1] * 6, tz + L.keyDir[2] * 6]}
        target-position={[tx, 0, tz]}
        color={L.keyColor}
        intensity={L.keyIntensity}
        castShadow
        shadow-mapSize={[2048, 2048]}
        shadow-bias={-0.0005}
        shadow-normalBias={0.006}
        shadow-camera-left={-0.9}
        shadow-camera-right={0.9}
        shadow-camera-top={0.9}
        shadow-camera-bottom={-0.9}
        shadow-camera-near={1}
        shadow-camera-far={14}
      />
      <mesh rotation={[-Math.PI / 2, 0, 0]}>
        <planeGeometry args={[60, 60]} />
        <meshStandardMaterial color={theme.ocean.mid} roughness={0.35} />
      </mesh>
      {terrain && (
        <>
          <mesh geometry={terrain.geometry} receiveShadow castShadow>
            <meshStandardMaterial color={theme.land.plain} roughness={0.86} />
          </mesh>
          {q.get('bare') !== '1' && <Landmarks items={LANDMARKS} terrain={terrain} active={q.get('active') === '1' ? id : null} glow={theme.landmark.glow} onActive={() => {}} onSelect={() => {}} renderCard={() => null} />}
        </>
      )}
      <OrbitControls makeDefault target={[tx, 0.08, tz]} enableDamping={false} />
      <Stats />
    </SceneThemeContext.Provider>
  )
}

/** renderer counters for the frame just drawn, written straight to the DOM (no React state per frame) */
function Stats() {
  const gl = useThree((s) => s.gl)
  const acc = useRef({ t: 0, n: 0, worst: 0 })
  useEffect(() => {
    gl.info.autoReset = false
  }, [gl])
  useFrame((_, dt) => {
    const a = acc.current
    a.t += dt
    a.n++
    a.worst = Math.max(a.worst, dt)
    const el = document.getElementById('lm-stats')
    if (el && a.t > 0.5) {
      el.textContent = `${gl.info.render.calls} draw calls · ${gl.info.render.triangles.toLocaleString()} tris · ${(a.n / a.t).toFixed(0)} fps · worst ${(a.worst * 1000).toFixed(1)} ms`
      a.t = 0
      a.n = 0
      a.worst = 0
    }
    // read above, reset here: the counters then hold exactly the frame r3f draws next
    gl.info.reset()
  })
  return null
}

function Card({ id, theme, big }: { id: string; theme: SceneTheme; big: boolean }) {
  const lm = LANDMARKS.find((l) => l.id === id)
  const def = MODELS[id]
  const built = getBuilt(id, SEA)
  const size = built.body.boundingBox!.getSize(new THREE.Vector3())
  const tris = built.body.attributes.position.count / 3 + (built.movers ?? []).reduce((n, m) => n + m.geometry.attributes.position.count / 3, 0)
  const ink = theme.label.ink, muted = theme.label.muted
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-0 p-4" style={{ color: ink }}>
      <div className="font-mono text-[10.5px]" style={{ color: muted }}>
        {lm?.kind} · {theme.name}
      </div>
      <div className={big ? 'text-[22px] leading-tight font-semibold tracking-[-0.02em]' : 'text-[15px] leading-tight font-semibold tracking-[-0.015em]'}>{lm?.name.en ?? id}</div>
      <div className="mt-0.5 flex gap-2 text-[12.5px]" style={{ color: muted }}>
        <span lang="si">{lm?.name.si}</span>
        <span>·</span>
        <span lang="ta">{lm?.name.ta}</span>
      </div>
      {big && (
        <>
          <ul className="mt-2 max-w-[62ch] space-y-0.5 text-[12px] leading-snug" style={{ color: muted }}>
            {def.notes.map((n) => (
              <li key={n}>— {n}</li>
            ))}
          </ul>
          <div className="mt-2 font-mono text-[10.5px]" style={{ color: muted }}>
            {Math.round(tris).toLocaleString()} tris · {1 + (built.movers?.length ?? 0)} draw call{built.movers?.length ? 's' : ''} · {size.x.toFixed(2)} × {size.y.toFixed(2)} × {size.z.toFixed(2)} units
          </div>
        </>
      )}
    </div>
  )
}

function Frame({ id, theme, big }: { id: string; theme: SceneTheme; big: boolean }) {
  return (
    <div className="relative h-full w-full overflow-hidden" style={{ background: theme.sky.horizon }}>
      <Canvas shadows="soft" dpr={[1, 2]} camera={{ fov: 24, near: 0.01, far: 50 }} gl={{ antialias: true }}>
        {big && q.get('scene') === '1' ? (
          <InScene id={id} theme={theme} rot={num('rot', 0)} elev={num('elev', HERO_ELEV)} zoom={num('zoom', 1)} />
        ) : (
          <Stage id={id} theme={theme} rot={num('rot', 0)} elev={num('elev', HERO_ELEV)} zoom={num('zoom', 1)} spin={q.get('spin') === '1'} />
        )}
      </Canvas>
      <Card id={id} theme={theme} big={big} />
      {big && <div id="lm-stats" className="pointer-events-none absolute top-3 right-4 font-mono text-[10.5px]" style={{ color: theme.label.muted }} />}
    </div>
  )
}

export function Review() {
  const theme = SCENE_THEMES[THEME]
  const id = q.get('m')
  useEffect(() => {
    document.documentElement.classList.remove('dark', 'light')
    document.documentElement.classList.add(THEME)
  }, [])
  if (id && MODELS[id]) {
    return (
      <div className="fixed inset-0">
        <Frame id={id} theme={theme} big />
      </div>
    )
  }
  const ids = Object.keys(MODELS)
  return (
    <div className="fixed inset-0 grid grid-cols-3 gap-px" style={{ background: theme.label.halo, gridTemplateRows: `repeat(${Math.ceil(ids.length / 3)}, minmax(0, 1fr))` }}>
      {ids.map((m) => (
        <Frame key={m} id={m} theme={theme} big={false} />
      ))}
    </div>
  )
}
