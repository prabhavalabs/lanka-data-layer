// The landmark layer: nine miniatures at their true coordinates.
//
// The models themselves live in ./landmarks (one module per landmark, authored in
// code and baked to a single geometry each). This file only places them on the
// island, lights them with the scene theme, and handles hover / tap.
//
// Draw calls: one per landmark body (9), one per pick ring (9), plus the few parts
// that move (Nine Arch's train cars, Trincomalee's ship, Galle's light). The hit
// targets are invisible meshes and are never drawn.
import { Html } from '@react-three/drei'
import { useFrame, useThree, type ThreeEvent } from '@react-three/fiber'
import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import * as THREE from 'three'
import type { Landmark } from '@/api/types'
import type { Focus } from './camera'
import { getBuilt, LandmarkMesh, useLandmarkClock } from './landmarks/LandmarkMesh'
import { MODELS } from './landmarks/models'
import type { ModelDef } from './landmarks/types'
import { useSceneTheme } from './sceneTheme'
import { CARD_ABOVE, placeCard, useStage, type CardPlacement } from './stage'
import { BEVEL, K, ORIGIN, PLINTH, toXZ, type Terrain } from './terrain'

/** rose accent of the page, used for the active ring */
const ACCENT = '#f0668a'
const RING_SEG = 72

type Placement = {
  x: number
  z: number
  /** world height of the model's ground plane */
  y: number
  yaw: number
  scale: number
  /** −1 when the model is mirrored left-to-right */
  flip: 1 | -1
  /** sea level in model units (relative to the model's ground) */
  sea: number
  /** body geometry with its ground vertices settled onto the terrain; null when nothing drapes */
  body: THREE.BufferGeometry | null
  ring: THREE.BufferGeometry
  hitR: number
  /** radius of what the eye takes for the landmark, in world units: most of its footprint, apron included */
  mass: number
  /** model height in world units */
  top: number
}

const lonAt = (x: number) => x / K + ORIGIN.lon
const latAt = (z: number) => ORIGIN.lat - z

/** the land surface under a world point; a little below the sea when the point is offshore */
function groundAt(terrain: Terrain, x: number, z: number) {
  const lon = lonAt(x), lat = latAt(z)
  return terrain.coastDistance(lon, lat) > 0 ? terrain.surfaceAt(lon, lat) : -0.02
}

function place(lm: Landmark, def: ModelDef, terrain: Terrain, heroYaw: number): Placement {
  const scale = def.scale ?? 1
  const flip = def.mirror ? -1 : 1
  let [x, z] = toXZ(...(def.site ?? [lm.lon, lm.lat]))
  let yaw = heroYaw + (def.yaw ?? 0)

  if (def.shore !== undefined) {
    const d = (dx: number, dz: number) => terrain.coastDistance(lonAt(x + dx), latAt(z + dz))
    let sx: number, sz: number
    if (def.bearing !== undefined) {
      // a headland whose direction is a matter of record: +z points down that compass bearing (north is −z, east is +x)
      sx = Math.sin((def.bearing * Math.PI) / 180)
      sz = -Math.cos((def.bearing * Math.PI) / 180)
    } else {
      // face the nearest open water: −∇(distance inland), sampled wide enough to ignore creeks
      const e = 0.07
      sx = -(d(e, 0) - d(-e, 0))
      sz = -(d(0, e) - d(0, -e))
      const len = Math.hypot(sx, sz) || 1
      sx /= len
      sz /= len
    }
    yaw = Math.atan2(sx, sz) + (def.yaw ?? 0)
    // slide along that bearing until the model's waterline lies on the real one
    for (let i = 0; i < 6; i++) {
      const off = d(0, 0) - def.shore * scale
      if (Math.abs(off) < 0.0015) break
      x += sx * off
      z += sz * off
    }
  } else if (def.inland) {
    // a site that would hang over the water walks uphill on the distance field until it fits (or the land runs out)
    const need = def.inland * scale, e = 0.03
    const d = (dx: number, dz: number) => terrain.coastDistance(lonAt(x + dx), latAt(z + dz))
    for (let i = 0; i < 24; i++) {
      const here = d(0, 0)
      if (here >= need) break
      const gx = d(e, 0) - d(-e, 0), gz = d(0, e) - d(0, -e)
      const len = Math.hypot(gx, gz)
      if (len < 1e-4) break
      const step = Math.min(need - here, 0.012)
      x += (gx / len) * step
      z += (gz / len) * step
    }
  }

  const built = getBuilt(lm.id, -PLINTH / scale) // bounds only: the footprint does not depend on sea level
  const box = built.body.boundingBox!
  const reach = Math.max(Math.abs(box.min.x), box.max.x, Math.abs(box.min.z), box.max.z) * scale

  let y: number
  if (def.seat === 'rim') {
    y = 0
    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2
      y += groundAt(terrain, x + Math.cos(a) * reach * 0.95, z + Math.sin(a) * reach * 0.95) / 16
    }
  } else {
    y = Math.max(0.003, terrain.heightAt(lonAt(x), latAt(z)))
    if (def.seat === 'high' && def.core) {
      for (const f of [0.5, 1])
        for (let i = 0; i < 10; i++) {
          const a = (i / 10) * Math.PI * 2 + f
          y = Math.max(y, groundAt(terrain, x + Math.cos(a) * def.core * scale * f, z + Math.sin(a) * def.core * scale * f))
        }
    }
  }
  const sea = -y / scale

  // settle ground-hugging vertices onto the real terrain
  const src = getBuilt(lm.id, sea).body
  const drape = src.getAttribute('drape')
  let body: THREE.BufferGeometry | null = null
  if (drape) {
    body = src.clone()
    const pos = body.getAttribute('position') as THREE.BufferAttribute
    const c = Math.cos(yaw), s = Math.sin(yaw)
    for (let i = 0; i < pos.count; i++) {
      const w = drape.getX(i)
      if (w <= 0) continue
      // sample under the vertex, or under the foot of the object it belongs to
      const ax = drape.getY(i), az = drape.getZ(i)
      const free = drape.getW(i) > 0
      const px = ax * scale * flip, pz = az * scale
      let wx = x + px * c + pz * s, wz = z - px * s + pz * c
      let g = groundAt(terrain, wx, wz)
      // the plate's rounded rim belongs to the shoreline: site ground stops at the top of it
      const edge = BEVEL + 0.004
      let d0 = terrain.coastDistance(lonAt(wx), latAt(wz))
      if (d0 < edge) {
        if (free) {
          // a tree or a hut whose foot is in the sea has no business being there. The sea does not write depth,
          // so sinking it would not hide it: fold the whole object into a point instead.
          if (d0 < 0.006) {
            pos.setXYZ(i, ax, 0, az)
            continue
          }
        } else if (w > 0.5) {
          // ground that runs off the land is drawn back to the edge of the plate, so an apron ends where the coast does
          for (let it = 0; it < 4 && d0 < edge; it++) {
            const e = 0.012
            const gx = terrain.coastDistance(lonAt(wx + e), latAt(wz)) - terrain.coastDistance(lonAt(wx - e), latAt(wz))
            const gz = terrain.coastDistance(lonAt(wx), latAt(wz + e)) - terrain.coastDistance(lonAt(wx), latAt(wz - e))
            const len = Math.hypot(gx, gz)
            if (len < 1e-5) break
            wx += (gx / len) * (edge + 0.002 - d0)
            wz += (gz / len) * (edge + 0.002 - d0)
            d0 = terrain.coastDistance(lonAt(wx), latAt(wz))
          }
          const qx = (wx - x) * c - (wz - z) * s, qz = (wx - x) * s + (wz - z) * c
          pos.setX(i, qx / (scale * flip))
          pos.setZ(i, qz / scale)
          g = groundAt(terrain, wx, wz)
        }
      }
      const my = pos.getY(i)
      const yw = y + my * scale + w * (g - y)
      // never let the land poke through something that is meant to cover it (buried skirts stay buried)
      pos.setY(i, (Math.max(yw, w > 0.02 && my >= 0 && g >= 0 ? g + 0.0012 : -1) - y) / scale)
    }
    pos.needsUpdate = true
    body.computeBoundingBox()
    body.computeBoundingSphere()
  }

  // pick ring: an ellipse just outside the model's footprint, laid on the land (or the sea) it crosses
  const c = Math.cos(yaw), s = Math.sin(yaw)
  const ex = def.ring ?? ((box.max.x - box.min.x) / 2) * 1.03, ez = def.ring ?? ((box.max.z - box.min.z) / 2) * 1.03
  const ecx = def.ring ? 0 : (box.min.x + box.max.x) / 2, ecz = def.ring ? 0 : (box.min.z + box.max.z) / 2
  const hitR = Math.min(ex, ez) * scale
  const ringPos = new Float32Array(RING_SEG * 6 * 3)
  const at = (a: number, grow: number, o: number) => {
    const mx = (ecx + Math.cos(a) * (ex + grow)) * scale * flip, mz = (ecz + Math.sin(a) * (ez + grow)) * scale
    const rx = mx * c + mz * s, rz = -mx * s + mz * c
    ringPos.set([rx, Math.max(0, groundAt(terrain, x + rx, z + rz)) + 0.0045, rz], o)
  }
  const band = 0.0052 / scale
  for (let i = 0; i < RING_SEG; i++) {
    const a0 = (i / RING_SEG) * Math.PI * 2, a1 = ((i + 1) / RING_SEG) * Math.PI * 2
    const o = i * 18
    at(a0, 0, o)
    at(a1, band, o + 3)
    at(a0, band, o + 6)
    at(a0, 0, o + 9)
    at(a1, 0, o + 12)
    at(a1, band, o + 15)
  }
  const ring = new THREE.BufferGeometry()
  ring.setAttribute('position', new THREE.BufferAttribute(ringPos, 3))

  return { x, z, y, yaw, scale, flip, sea, body, ring, hitR, mass: Math.max(hitR, reach * 0.62), top: box.max.y * scale }
}

function Pin({ lm, at, active, minHit, onActive, onSelect }: { lm: Landmark; at: Placement; active: boolean; /** smallest radius of the hit target, world units */ minHit: number; onActive: (id: string | null, touch?: boolean) => void; onSelect: (id: string, touch?: boolean) => void }) {
  const def = MODELS[lm.id]
  const group = useRef<THREE.Group>(null)
  const ring = useRef<THREE.Mesh>(null)
  const lift = useRef(0)
  // a click does not say what made it everywhere: remember the pointer that went down
  const touch = useRef(false)
  const gl = useThree((s) => s.gl)
  const frameloop = useThree((s) => s.frameloop)
  const pop = def.pop ?? 0.1

  // lift on focus; eases from the live value, so a fast tour never snaps
  useFrame((_, dt) => {
    const target = active ? 1 : 0
    const l0 = lift.current
    if (l0 !== target) {
      // reduced motion renders on demand: there is no next frame to ease into, so the state lands at once
      const l = frameloop === 'always' ? l0 + (target - l0) * (1 - Math.exp(-Math.min(dt, 0.05) * 9)) : target
      lift.current = Math.abs(target - l) < 0.002 ? target : l
      // the shadow map is redrawn on demand: keep it honest while the model moves
      if (pop > 0) gl.shadowMap.needsUpdate = true
    }
    const l = lift.current
    if (group.current) {
      group.current.position.y = at.y + l * pop * 0.12
      const k = at.scale * (1 + l * pop)
      group.current.scale.set(k * at.flip, k, k)
    }
    if (ring.current) {
      const m = ring.current.material as THREE.MeshBasicMaterial
      m.opacity = 0.1 + l * 0.8
    }
  })

  // a finger has no hover: on touch the tap alone decides, so these two only report it
  const over = (e: ThreeEvent<PointerEvent>) => {
    e.stopPropagation()
    onActive(lm.id, e.pointerType === 'touch')
    document.body.style.cursor = 'pointer'
  }
  const out = (e: ThreeEvent<PointerEvent>) => {
    onActive(null, e.pointerType === 'touch')
    document.body.style.cursor = ''
  }
  useEffect(() => () => void (document.body.style.cursor = ''), [])

  const hitH = at.top + 0.05
  return (
    <group position={[at.x, 0, at.z]}>
      <mesh ref={ring} geometry={at.ring} renderOrder={2}>
        <meshBasicMaterial color={active ? ACCENT : '#ffffff'} transparent opacity={0.1} depthWrite={false} toneMapped={false} side={THREE.DoubleSide} forceSinglePass />
      </mesh>
      <group ref={group} position={[0, at.y, 0]} rotation={[0, at.yaw, 0]} scale={[at.scale * at.flip, at.scale, at.scale]}>
        <LandmarkMesh id={lm.id} sea={at.sea} body={at.body} />
      </group>
      {/* hit target: never drawn, still raycast */}
      <mesh position={[0, at.y + hitH / 2 - 0.01, 0]} visible={false} onPointerOver={over} onPointerOut={out} onPointerDown={(e) => void (touch.current = e.pointerType === 'touch')} onClick={(e) => (e.stopPropagation(), onSelect(lm.id, touch.current))}>
        <cylinderGeometry args={[Math.max(at.hitR * 0.78, minHit), Math.max(at.hitR * 0.9, minHit), hitH, 12]} />
      </mesh>
    </group>
  )
}

/** the landmark card's box, for placement: its width is fixed, its height is the tallest it gets (a name that wraps). Phones drop the last row. */
const CARD = { w: 236, h: 204 }
const CARD_PHONE = { w: 236, h: 168 }

export function Landmarks({
  items,
  terrain,
  active,
  focus = null,
  animate = true,
  onActive,
  onSelect,
  renderCard,
}: {
  items: Landmark[]
  terrain: Terrain
  active: string | null
  /** what the camera is leaning toward, so a card can be placed for where the camera will be */
  focus?: Focus | null
  /** false: the camera never moves (reduced motion) */
  animate?: boolean
  /** theme.landmark.glow. Kept for the caller's sake: the layer reads the whole scene theme from context and eases the glow itself. */
  glow?: number
  onActive: (id: string | null, touch?: boolean) => void
  onSelect: (id: string, touch?: boolean) => void
  renderCard: (lm: Landmark, at: CardPlacement) => ReactNode
}) {
  useLandmarkClock(useSceneTheme().name)
  const stage = useStage()
  const camera = useThree((s) => s.camera) as THREE.PerspectiveCamera
  const width = useThree((s) => s.size.width), height = useThree((s) => s.size.height)

  // models present their signature side to the camera: the rig sits ~0.5 rad west of south on wide screens, ~0.3 on tall ones
  const wide = width / height > 1.15
  const heroYaw = wide ? -0.5 : -0.3

  const placed = useMemo(() => {
    const out = new Map<string, Placement>()
    for (const lm of items) {
      const def = MODELS[lm.id]
      if (def) out.set(lm.id, place(lm, def, terrain, heroYaw))
    }
    return out
  }, [items, terrain, heroYaw])
  useEffect(
    () => () => {
      for (const p of placed.values()) {
        p.body?.dispose()
        p.ring.dispose()
      }
    },
    [placed],
  )
  // tell the stage where everything stands: the rig leans toward these, and cards keep clear of them
  useEffect(() => {
    if (!stage) return
    for (const [id, p] of placed) stage.anchors.set(id, { x: p.x, y: p.y, z: p.z, r: p.mass, h: p.top })
    return () => {
      for (const id of placed.keys()) stage.anchors.delete(id)
    }
  }, [placed, stage])

  const current = items.find((l) => l.id === active)
  const where = current ? placed.get(current.id) : undefined
  const pop = current ? (MODELS[current.id]?.pop ?? 0.1) : 0
  const currentId = where ? current!.id : null
  // solved once per card, for the pose the camera is heading to
  const at = useMemo(
    () => (currentId && stage ? placeCard(stage, camera, width, height, currentId, focus, animate, width < 640 ? CARD_PHONE : CARD, pop) : CARD_ABOVE),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [currentId, focus?.id, focus?.kind, animate, width, height, stage, placed],
  )
  return (
    <group>
      {items.map((lm) => {
        const at = placed.get(lm.id)
        // on a tall screen the island is small and the pointer is a finger: no landmark is a smaller target than a fingertip
        return at ? <Pin key={lm.id} lm={lm} at={at} active={lm.id === active} minHit={wide ? 0 : 0.21} onActive={onActive} onSelect={onSelect} /> : null
      })}
      {current && where && (
        <Html key={current.id} position={[where.x, where.y + where.top * (1 + pop) + 0.03, where.z]} zIndexRange={[30, 20]} style={{ pointerEvents: 'none' }}>
          {renderCard(current, at)}
        </Html>
      )}
    </group>
  )
}
