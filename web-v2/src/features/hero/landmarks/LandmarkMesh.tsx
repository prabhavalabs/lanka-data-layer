// Draws one built landmark: the baked body plus its few moving parts.
import { useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo, useRef } from 'react'
import * as THREE from 'three'
import { SCENE_THEMES, type SceneThemeName } from '../sceneTheme'
import { landmarkMaterial, landmarkUniforms } from './material'
import { MODELS } from './models'
import type { Built } from './types'

const cache = new Map<string, Built>()
/** Build (once) and return a landmark's geometry. Coastal models bake their waterline, so `sea` is part of their key. */
export function getBuilt(id: string, sea: number): Built {
  const key = MODELS[id].shore !== undefined ? `${id}@${sea.toFixed(4)}` : id
  let b = cache.get(key)
  if (!b) {
    b = MODELS[id].build({ sea })
    cache.set(key, b)
  }
  return b
}

let beam: THREE.MeshBasicMaterial | null = null
function beamMaterial() {
  beam ??= new THREE.MeshBasicMaterial({ color: '#ffe9b0', vertexColors: true, transparent: true, opacity: 0, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true, toneMapped: false, blending: THREE.AdditiveBlending })
  return beam
}

/**
 * Drives the shared uniforms. Mount exactly once per canvas.
 * The day/night cross-fade runs on the same 1.1 s the scene uses for its own palette.
 */
export function useLandmarkClock(theme: SceneThemeName) {
  const frameloop = useThree((s) => s.frameloop)
  const first = useRef(true)
  useEffect(() => {
    landmarkUniforms.uEmissive.value.set(SCENE_THEMES[theme].landmark.emissive)
  }, [theme])
  useFrame(({ clock }, dt) => {
    const u = landmarkUniforms
    const target = theme === 'light' ? 1 : 0
    if (first.current || frameloop !== 'always') u.uDay.value = target
    else if (u.uDay.value !== target) {
      const step = Math.min(dt, 0.05) / 1.1
      u.uDay.value = target > u.uDay.value ? Math.min(target, u.uDay.value + step) : Math.max(target, u.uDay.value - step)
    }
    first.current = false
    const night = SCENE_THEMES.dark.landmark.glow, day = SCENE_THEMES.light.landmark.glow
    u.uGlow.value = night + (day - night) * u.uDay.value
    u.uTime.value = clock.elapsedTime
    if (beam) beam.opacity = 0.09 * u.uGlow.value
  })
}

export function LandmarkMesh({ id, sea, body, shadows = true }: { id: string; sea: number; /** the body settled onto the terrain, when the scene has one */ body?: THREE.BufferGeometry | null; shadows?: boolean }) {
  const built = useMemo(() => getBuilt(id, sea), [id, sea])
  const movers = useRef<(THREE.Mesh | null)[]>([])
  const list = built.movers
  useFrame(({ clock }) => {
    if (!list) return
    for (let i = 0; i < list.length; i++) {
      const obj = movers.current[i]
      if (!obj) continue
      list[i].update(obj, clock.elapsedTime)
      if (list[i].kind === 'beam') obj.visible = landmarkUniforms.uGlow.value > 0.03
    }
  })
  return (
    <>
      <mesh geometry={body ?? built.body} material={landmarkMaterial()} castShadow={shadows} receiveShadow={shadows} />
      {list?.map((mv, i) => (
        <mesh
          key={i}
          ref={(o) => {
            movers.current[i] = o
          }}
          geometry={mv.geometry}
          material={mv.kind === 'beam' ? beamMaterial() : landmarkMaterial()}
          // the shadow map is only redrawn on demand, so anything that moves stays out of it
          castShadow={false}
          receiveShadow={shadows && mv.kind !== 'beam'}
          renderOrder={mv.kind === 'beam' ? 5 : 0}
        />
      ))}
    </>
  )
}
