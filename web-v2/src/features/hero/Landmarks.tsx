// Procedurally modelled, low-poly landmarks. No downloaded assets: every
// building is a handful of primitives, placed at its true longitude/latitude.
import { Html } from '@react-three/drei'
import { useFrame, type ThreeEvent } from '@react-three/fiber'
import { useMemo, useRef, type ReactNode } from 'react'
import * as THREE from 'three'
import type { Landmark } from '@/api/types'
import { toXZ, type Terrain } from './terrain'

type P = { glow: number }

const flat = { flatShading: true, roughness: 0.85, metalness: 0 } as const
function Mat({ c, e, ei = 0, ...rest }: { c: string; e?: string; ei?: number } & Partial<THREE.MeshStandardMaterialParameters>) {
  return <meshStandardMaterial color={c} emissive={e ?? '#000000'} emissiveIntensity={ei} {...flat} {...rest} />
}

/* ---------------- models (author units = scene units) ---------------- */

function Sigiriya() {
  return (
    <group>
      <mesh position={[0, 0.012, 0]} receiveShadow castShadow>
        <cylinderGeometry args={[0.085, 0.104, 0.024, 9]} />
        <Mat c="#4f9450" />
      </mesh>
      <mesh position={[0, 0.095, 0]} castShadow receiveShadow rotation={[0, 0.4, 0]}>
        <cylinderGeometry args={[0.043, 0.06, 0.15, 7]} />
        <Mat c="#cf8a4e" />
      </mesh>
      <mesh position={[0.012, 0.05, 0.02]} castShadow rotation={[0, 1.1, 0.1]}>
        <cylinderGeometry args={[0.03, 0.05, 0.07, 6]} />
        <Mat c="#b9753f" />
      </mesh>
      <mesh position={[0, 0.173, 0]} rotation={[0, 0.4, 0]}>
        <cylinderGeometry args={[0.044, 0.044, 0.007, 7]} />
        <Mat c="#6aa554" />
      </mesh>
      <mesh position={[-0.006, 0.181, 0.004]} castShadow>
        <boxGeometry args={[0.03, 0.009, 0.022]} />
        <Mat c="#e2d3b0" />
      </mesh>
      {[-0.014, 0.014].map((x) => (
        <mesh key={x} position={[x, 0.03, 0.062]} castShadow>
          <boxGeometry args={[0.014, 0.012, 0.022]} />
          <Mat c="#d9b98a" />
        </mesh>
      ))}
    </group>
  )
}

function SriPada({ glow }: P) {
  return (
    <group>
      <mesh position={[0, 0.03, 0]} castShadow receiveShadow rotation={[0, 0.3, 0]}>
        <coneGeometry args={[0.062, 0.13, 6]} />
        <Mat c="#55785a" />
      </mesh>
      <mesh position={[0, 0.072, 0]} castShadow rotation={[0, 0.3, 0]}>
        <coneGeometry args={[0.026, 0.056, 6]} />
        <Mat c="#8d9484" />
      </mesh>
      <mesh position={[0, 0.101, 0]} castShadow>
        <boxGeometry args={[0.013, 0.008, 0.013]} />
        <Mat c="#f4efe2" e="#ffd9a0" ei={glow * 1.6} />
      </mesh>
      <mesh position={[0, 0.109, 0]}>
        <coneGeometry args={[0.008, 0.008, 4]} />
        <Mat c="#b5553c" />
      </mesh>
      {/* the pilgrim path, lit through the season's nights */}
      {Array.from({ length: 9 }, (_, i) => {
        const t = i / 9
        const a = t * 4.2 + 0.6
        const r = 0.058 * (1 - t) + 0.006
        return (
          <mesh key={i} position={[Math.cos(a) * r, 0.03 - 0.065 + t * 0.13 + 0.004, Math.sin(a) * r]}>
            <sphereGeometry args={[0.0028, 6, 4]} />
            <meshBasicMaterial color="#ffd38a" transparent opacity={0.25 + glow * 0.75} toneMapped={false} />
          </mesh>
        )
      })}
    </group>
  )
}

function LotusTower({ glow }: P) {
  const bud = useMemo(() => {
    const pts = [
      [0.0, 0.0], [0.012, 0.0], [0.02, 0.006], [0.0235, 0.016], [0.022, 0.028], [0.016, 0.04], [0.009, 0.049], [0.004, 0.054], [0.0, 0.056],
    ].map(([x, y]) => new THREE.Vector2(x, y))
    return new THREE.LatheGeometry(pts, 10)
  }, [])
  const skyline: [number, number, number, number][] = [
    [-0.045, 0.02, 0.034, 0.016], [-0.03, -0.022, 0.052, 0.014], [0.034, 0.03, 0.028, 0.018], [0.05, -0.012, 0.044, 0.013], [-0.012, 0.046, 0.022, 0.02], [0.022, -0.04, 0.036, 0.015], [-0.06, -0.012, 0.02, 0.016],
  ]
  return (
    <group>
      {skyline.map(([x, z, h, w], i) => (
        <mesh key={i} position={[x, h / 2, z]} castShadow receiveShadow>
          <boxGeometry args={[w, h, w]} />
          <Mat c={i % 2 ? '#aebccb' : '#c6d0da'} e="#ffcf8a" ei={glow * (0.25 + (i % 3) * 0.12)} />
        </mesh>
      ))}
      <mesh position={[0, 0.006, 0]} castShadow>
        <cylinderGeometry args={[0.026, 0.03, 0.012, 12]} />
        <Mat c="#dfe5df" />
      </mesh>
      <mesh position={[0, 0.105, 0]} castShadow>
        <cylinderGeometry args={[0.0062, 0.0125, 0.19, 10]} />
        <Mat c="#9fd2b4" e="#7dffc0" ei={glow * 0.35} />
      </mesh>
      <mesh position={[0, 0.196, 0]} castShadow>
        <cylinderGeometry args={[0.017, 0.008, 0.012, 10]} />
        <Mat c="#f0a6dc" e="#ff8ad8" ei={glow * 0.5} />
      </mesh>
      <mesh position={[0, 0.2, 0]} geometry={bud} castShadow>
        <Mat c="#b24fc4" e="#e25cff" ei={glow * 0.9} />
      </mesh>
      <mesh position={[0, 0.284, 0]}>
        <cylinderGeometry args={[0.001, 0.0028, 0.06, 6]} />
        <Mat c="#e8eef0" />
      </mesh>
    </group>
  )
}

function Ruwanwelisaya({ glow }: P) {
  return (
    <group>
      <mesh position={[0, 0.005, 0]} castShadow receiveShadow>
        <boxGeometry args={[0.13, 0.01, 0.13]} />
        <Mat c="#d8cfb8" />
      </mesh>
      {[0.056, 0.051, 0.046].map((r, i) => (
        <mesh key={r} position={[0, 0.014 + i * 0.008, 0]} castShadow receiveShadow>
          <cylinderGeometry args={[r, r + 0.002, 0.008, 20]} />
          <Mat c="#f1ede2" flatShading={false} />
        </mesh>
      ))}
      <mesh position={[0, 0.034, 0]} castShadow scale={[1, 0.94, 1]}>
        <sphereGeometry args={[0.042, 22, 12, 0, Math.PI * 2, 0, Math.PI / 2]} />
        <Mat c="#f8f5ec" flatShading={false} e="#ffe2b8" ei={glow * 0.22} roughness={0.6} />
      </mesh>
      <mesh position={[0, 0.079, 0]} castShadow>
        <boxGeometry args={[0.019, 0.012, 0.019]} />
        <Mat c="#f1ede2" />
      </mesh>
      <mesh position={[0, 0.107, 0]} castShadow>
        <coneGeometry args={[0.0085, 0.046, 10]} />
        <Mat c="#f1ede2" flatShading={false} />
      </mesh>
      <mesh position={[0, 0.133, 0]}>
        <sphereGeometry args={[0.0036, 8, 6]} />
        <Mat c="#e6b84f" e="#ffcf6a" ei={0.3 + glow} metalness={0.4} roughness={0.4} />
      </mesh>
    </group>
  )
}

function TempleOfTheTooth({ glow }: P) {
  return (
    <group rotation={[0, -0.5, 0]}>
      {/* Kandy Lake */}
      <mesh position={[0.02, 0.0015, 0.062]} rotation={[-Math.PI / 2, 0, 0]} scale={[1.5, 0.75, 1]}>
        <circleGeometry args={[0.045, 24]} />
        <meshStandardMaterial color="#4aa7cf" roughness={0.25} metalness={0.1} />
      </mesh>
      <mesh position={[0, 0.006, 0.022]} castShadow receiveShadow>
        <boxGeometry args={[0.13, 0.012, 0.012]} />
        <Mat c="#f3efe4" />
      </mesh>
      <mesh position={[0.01, 0.022, -0.012]} castShadow receiveShadow>
        <boxGeometry args={[0.088, 0.032, 0.05]} />
        <Mat c="#f3efe4" e="#ffdca8" ei={glow * 0.25} />
      </mesh>
      <mesh position={[0.01, 0.05, -0.012]} rotation={[0, Math.PI / 4, 0]} scale={[1.42, 1, 0.86]} castShadow>
        <coneGeometry args={[0.05, 0.026, 4]} />
        <Mat c="#a9533b" />
      </mesh>
      <mesh position={[0.01, 0.066, -0.012]} castShadow>
        <boxGeometry args={[0.034, 0.005, 0.022]} />
        <Mat c="#e6b84f" e="#ffcf6a" ei={0.25 + glow * 0.9} metalness={0.5} roughness={0.35} />
      </mesh>
      {/* Paththirippuwa, the octagon */}
      <mesh position={[-0.05, 0.024, 0.012]} castShadow>
        <cylinderGeometry args={[0.019, 0.021, 0.048, 8]} />
        <Mat c="#f8f5ec" e="#ffdca8" ei={glow * 0.3} />
      </mesh>
      <mesh position={[-0.05, 0.059, 0.012]} castShadow>
        <coneGeometry args={[0.029, 0.024, 8]} />
        <Mat c="#a9533b" />
      </mesh>
    </group>
  )
}

function GalleFort({ glow }: P) {
  const beam = useRef<THREE.Group>(null)
  useFrame((_, dt) => {
    if (beam.current) beam.current.rotation.y += dt * 0.7
  })
  return (
    <group>
      <mesh position={[0, 0.009, 0]} castShadow receiveShadow rotation={[0, 0.3, 0]}>
        <cylinderGeometry args={[0.066, 0.074, 0.018, 6]} />
        <Mat c="#8e897c" />
      </mesh>
      <mesh position={[0, 0.0195, 0]} rotation={[0, 0.3, 0]}>
        <cylinderGeometry args={[0.058, 0.058, 0.004, 6]} />
        <Mat c="#6a9c58" />
      </mesh>
      {[[-0.02, 0.01], [0.012, -0.02], [-0.008, -0.024], [0.02, 0.016], [-0.03, -0.006]].map(([x, z], i) => (
        <group key={i} position={[x, 0.021, z]}>
          <mesh position={[0, 0.005, 0]} castShadow>
            <boxGeometry args={[0.016, 0.01, 0.012]} />
            <Mat c="#f0eadb" />
          </mesh>
          <mesh position={[0, 0.0135, 0]} rotation={[0, Math.PI / 4, 0]} scale={[1.3, 1, 1]}>
            <coneGeometry args={[0.0095, 0.007, 4]} />
            <Mat c="#b5603f" />
          </mesh>
        </group>
      ))}
      <group position={[0.034, 0.02, 0.034]}>
        <mesh position={[0, 0.042, 0]} castShadow>
          <cylinderGeometry args={[0.0062, 0.0105, 0.084, 10]} />
          <Mat c="#fbf9f2" flatShading={false} />
        </mesh>
        <mesh position={[0, 0.086, 0]}>
          <cylinderGeometry args={[0.0115, 0.0115, 0.0035, 10]} />
          <Mat c="#3a3f47" />
        </mesh>
        <mesh position={[0, 0.094, 0]}>
          <cylinderGeometry args={[0.006, 0.006, 0.012, 8]} />
          <meshBasicMaterial color="#fff0b8" toneMapped={false} />
        </mesh>
        <mesh position={[0, 0.105, 0]}>
          <coneGeometry args={[0.0085, 0.01, 8]} />
          <Mat c="#b5553c" />
        </mesh>
        <group ref={beam} position={[0, 0.094, 0]} visible={glow > 0.05}>
          <mesh position={[0.11, 0, 0]} rotation={[0, 0, Math.PI / 2]}>
            <coneGeometry args={[0.022, 0.22, 12, 1, true]} />
            <meshBasicMaterial color="#ffe9a8" transparent opacity={0.045 * glow} depthWrite={false} side={THREE.DoubleSide} toneMapped={false} blending={THREE.AdditiveBlending} />
          </mesh>
        </group>
      </group>
    </group>
  )
}

function NineArch() {
  const train = useRef<THREE.Group>(null)
  const geom = useMemo(() => {
    const W = 0.21, H = 0.07, n = 9, pier = 0.0065
    const aw = (W - pier * (n + 1)) / n
    const s = new THREE.Shape()
    s.moveTo(-W / 2, 0)
    s.lineTo(W / 2, 0)
    s.lineTo(W / 2, H)
    s.lineTo(-W / 2, H)
    s.closePath()
    for (let i = 0; i < n; i++) {
      const x0 = -W / 2 + pier + i * (aw + pier)
      const r = aw / 2
      const top = H - 0.016 - r
      const hole = new THREE.Path()
      hole.moveTo(x0, -0.001)
      hole.lineTo(x0, top)
      hole.absarc(x0 + r, top, r, Math.PI, 0, true)
      hole.lineTo(x0 + aw, -0.001)
      hole.closePath()
      s.holes.push(hole)
    }
    const g = new THREE.ExtrudeGeometry(s, { depth: 0.013, bevelEnabled: false, curveSegments: 8 })
    g.translate(0, 0, -0.0065)
    return g
  }, [])
  useFrame(({ clock }) => {
    if (train.current) train.current.position.x = Math.sin(clock.elapsedTime * 0.22) * 0.07
  })
  return (
    <group rotation={[0, 0.5, 0]}>
      <mesh geometry={geom} position={[0, -0.022, 0]} castShadow receiveShadow>
        <Mat c="#c2b093" />
      </mesh>
      <mesh position={[0, 0.0495, 0]}>
        <boxGeometry args={[0.215, 0.003, 0.016]} />
        <Mat c="#8c7f6b" />
      </mesh>
      <group ref={train} position={[0, 0.056, 0]}>
        <mesh position={[0.024, 0, 0]} castShadow>
          <boxGeometry args={[0.02, 0.01, 0.009]} />
          <Mat c="#c8452f" />
        </mesh>
        {[0, -0.022].map((x) => (
          <mesh key={x} position={[x, 0, 0]} castShadow>
            <boxGeometry args={[0.02, 0.009, 0.009]} />
            <Mat c="#2f5f9e" />
          </mesh>
        ))}
      </group>
    </group>
  )
}

function Nallur({ glow }: P) {
  const tiers = 7
  return (
    <group>
      <mesh position={[0, 0.004, 0]} receiveShadow castShadow>
        <boxGeometry args={[0.12, 0.008, 0.09]} />
        <Mat c="#e8dcc0" />
      </mesh>
      {Array.from({ length: 8 }, (_, i) => (
        <mesh key={i} position={[-0.049 + i * 0.014, 0.019, 0.036]} castShadow>
          <boxGeometry args={[0.014, 0.022, 0.004]} />
          <Mat c={i % 2 ? '#f6f1e6' : '#b8402f'} />
        </mesh>
      ))}
      <mesh position={[0, 0.019, 0]} castShadow receiveShadow>
        <boxGeometry args={[0.11, 0.022, 0.07]} />
        <Mat c="#efe6d0" />
      </mesh>
      {Array.from({ length: tiers }, (_, i) => {
        const t = i / (tiers - 1)
        const w = 0.058 - t * 0.03
        return (
          <mesh key={i} position={[0, 0.038 + i * 0.0155, 0.012]} castShadow>
            <boxGeometry args={[w, 0.0145, w * 0.62]} />
            <Mat c={i % 2 ? '#d99a3a' : '#edbb52'} e="#ffb84a" ei={glow * 0.35} />
          </mesh>
        )
      })}
      <mesh position={[0, 0.038 + tiers * 0.0155 + 0.001, 0.012]} rotation={[0, 0, Math.PI / 2]} castShadow>
        <cylinderGeometry args={[0.0095, 0.0095, 0.03, 10, 1, false, 0, Math.PI]} />
        <Mat c="#edbb52" e="#ffb84a" ei={glow * 0.4} />
      </mesh>
      {[-0.009, 0, 0.009].map((x) => (
        <mesh key={x} position={[x, 0.038 + tiers * 0.0155 + 0.016, 0.012]}>
          <coneGeometry args={[0.0022, 0.011, 6]} />
          <Mat c="#f2cf66" e="#ffd76a" ei={0.3 + glow} metalness={0.5} roughness={0.35} />
        </mesh>
      ))}
    </group>
  )
}

function Trincomalee({ glow }: P) {
  const ship = useRef<THREE.Group>(null)
  useFrame(({ clock }) => {
    if (!ship.current) return
    const t = clock.elapsedTime
    ship.current.position.y = 0.004 + Math.sin(t * 1.1) * 0.0012
    ship.current.rotation.z = Math.sin(t * 0.8) * 0.03
  })
  return (
    <group>
      {/* Swami Rock headland */}
      <mesh position={[0.03, 0.016, -0.03]} scale={[1.3, 0.8, 1]} castShadow receiveShadow>
        <dodecahedronGeometry args={[0.03, 0]} />
        <Mat c="#807868" />
      </mesh>
      <mesh position={[0.03, 0.042, -0.03]} castShadow>
        <coneGeometry args={[0.008, 0.016, 4]} />
        <Mat c="#e9c15a" e="#ffcf6a" ei={glow * 0.6} />
      </mesh>
      {/* quay and cranes */}
      <mesh position={[-0.006, 0.005, 0.03]} castShadow receiveShadow>
        <boxGeometry args={[0.085, 0.006, 0.016]} />
        <Mat c="#9aa1a8" />
      </mesh>
      {[-0.03, 0.006].map((x) => (
        <group key={x} position={[x, 0.008, 0.03]}>
          <mesh position={[0, 0.02, 0]} castShadow>
            <boxGeometry args={[0.004, 0.04, 0.004]} />
            <Mat c="#e2733c" />
          </mesh>
          <mesh position={[0, 0.039, 0.012]} castShadow>
            <boxGeometry args={[0.003, 0.003, 0.034]} />
            <Mat c="#e2733c" />
          </mesh>
        </group>
      ))}
      {[[-0.012, '#3f7fbf'], [0.0, '#c8452f'], [0.012, '#e8b84a']].map(([x, c]) => (
        <mesh key={x as number} position={[(x as number) + 0.022, 0.0115, 0.03]} castShadow>
          <boxGeometry args={[0.01, 0.007, 0.007]} />
          <Mat c={c as string} />
        </mesh>
      ))}
      {/* a ship riding at anchor in the bay */}
      <group ref={ship} position={[-0.012, 0.004, 0.058]} rotation={[0, 0.25, 0]}>
        <mesh castShadow>
          <boxGeometry args={[0.062, 0.009, 0.014]} />
          <Mat c="#24364d" />
        </mesh>
        <mesh position={[0.036, 0, 0]} rotation={[0, Math.PI / 4, 0]} scale={[1, 1, 1]}>
          <boxGeometry args={[0.0099, 0.009, 0.0099]} />
          <Mat c="#24364d" />
        </mesh>
        <mesh position={[-0.02, 0.009, 0]} castShadow>
          <boxGeometry args={[0.014, 0.011, 0.011]} />
          <Mat c="#f1ede2" e="#ffdca8" ei={glow * 0.7} />
        </mesh>
        <mesh position={[-0.021, 0.0185, 0]}>
          <cylinderGeometry args={[0.0022, 0.0022, 0.008, 6]} />
          <Mat c="#c8452f" />
        </mesh>
        {[0.0, 0.012].map((x) => (
          <mesh key={x} position={[x + 0.004, 0.0075, 0]}>
            <boxGeometry args={[0.01, 0.006, 0.01]} />
            <Mat c={x ? '#3f7fbf' : '#e2733c'} />
          </mesh>
        ))}
      </group>
    </group>
  )
}

const MODELS: Record<string, { C: (p: P) => ReactNode; top: number; scale: number; hit: number; sea?: boolean }> = {
  sigiriya: { C: Sigiriya, top: 0.19, scale: 1.25, hit: 0.11 },
  'sri-pada': { C: SriPada, top: 0.12, scale: 1.2, hit: 0.1 },
  'lotus-tower': { C: LotusTower, top: 0.315, scale: 1.2, hit: 0.085 },
  ruwanwelisaya: { C: Ruwanwelisaya, top: 0.14, scale: 1.5, hit: 0.1 },
  'temple-tooth': { C: TempleOfTheTooth, top: 0.085, scale: 1.45, hit: 0.1 },
  'galle-fort': { C: GalleFort, top: 0.13, scale: 1.4, hit: 0.1 },
  'nine-arch': { C: NineArch, top: 0.07, scale: 1.3, hit: 0.13 },
  nallur: { C: Nallur, top: 0.17, scale: 1.35, hit: 0.095 },
  trinco: { C: Trincomalee, top: 0.06, scale: 1.5, hit: 0.11, sea: true },
}

function Pin({ lm, terrain, active, glow, onActive, onSelect }: { lm: Landmark; terrain: Terrain; active: boolean; glow: number; onActive: (id: string | null) => void; onSelect: (id: string) => void }) {
  const model = MODELS[lm.id]
  const group = useRef<THREE.Group>(null)
  const ring = useRef<THREE.Mesh>(null)
  const lift = useRef(0)
  const [x, z] = toXZ(lm.lon, lm.lat)
  const y = Math.max(0.003, terrain.heightAt(lm.lon, lm.lat)) - (lm.id === 'sri-pada' ? 0.03 : 0)

  // spring-ish lift on focus; interruptible because it eases from the live value
  useFrame((_, dt) => {
    const target = active ? 1 : 0
    lift.current += (target - lift.current) * Math.min(1, dt * 9)
    const l = lift.current
    if (group.current) {
      group.current.position.y = y + l * 0.012
      group.current.scale.setScalar(model.scale * (1 + l * 0.08))
    }
    if (ring.current) {
      ring.current.scale.setScalar(1 + l * 0.35)
      ;(ring.current.material as THREE.MeshBasicMaterial).opacity = 0.22 + l * 0.7
    }
  })

  const over = (e: ThreeEvent<PointerEvent>) => {
    e.stopPropagation()
    onActive(lm.id)
    document.body.style.cursor = 'pointer'
  }
  const out = () => {
    onActive(null)
    document.body.style.cursor = ''
  }

  return (
    <group position={[x, 0, z]}>
      <mesh ref={ring} position={[0, y + 0.006, 0]} rotation={[-Math.PI / 2, 0, 0]}>
        <ringGeometry args={[model.hit * 0.92, model.hit, 56]} />
        <meshBasicMaterial color={active ? '#f0668a' : '#ffffff'} transparent opacity={0.22} depthWrite={false} toneMapped={false} />
      </mesh>
      <group ref={group} position={[0, y, 0]} scale={model.scale}>
        <model.C glow={glow} />
      </group>
      <mesh position={[0, y + model.top * model.scale * 0.5, 0]} onPointerOver={over} onPointerOut={out} onClick={(e) => (e.stopPropagation(), onSelect(lm.id))}>
        <cylinderGeometry args={[model.hit * 1.05, model.hit * 1.05, model.top * model.scale + 0.04, 10]} />
        <meshBasicMaterial transparent opacity={0} depthWrite={false} />
      </mesh>
    </group>
  )
}

export function Landmarks({
  items,
  terrain,
  active,
  glow,
  onActive,
  onSelect,
  renderCard,
}: {
  items: Landmark[]
  terrain: Terrain
  active: string | null
  glow: number
  onActive: (id: string | null) => void
  onSelect: (id: string) => void
  renderCard: (lm: Landmark) => ReactNode
}) {
  const current = items.find((l) => l.id === active)
  const anchor = useMemo(() => {
    if (!current) return null
    const [x, z] = toXZ(current.lon, current.lat)
    const m = MODELS[current.id]
    return [x, Math.max(0.003, terrain.heightAt(current.lon, current.lat)) + m.top * m.scale + 0.03, z] as [number, number, number]
  }, [current, terrain])
  return (
    <group>
      {items.map((lm) => (MODELS[lm.id] ? <Pin key={lm.id} lm={lm} terrain={terrain} active={lm.id === active} glow={glow} onActive={onActive} onSelect={onSelect} /> : null))}
      {current && anchor && (
        <Html key={current.id} position={anchor} zIndexRange={[30, 20]} style={{ pointerEvents: 'none' }}>
          {renderCard(current)}
        </Html>
      )}
    </group>
  )
}
