// Camera poses for the hero, kept apart from the rig so the card placement can ask
// "where will the camera be once it has settled?" without waiting for it to get there.
//
// Rest: a slow orbit about the island, measured from due south and turning toward the
// west. Focus: the camera leans toward a landmark along its own line of sight. It moves
// without turning, so on a wide screen the landmark stays exactly where it is and simply
// grows: whatever the pointer is over is still under the pointer when the move ends, and
// a hover can never chase its own target. On a tall screen (a phone) a tap also slides
// the landmark to a fixed spot low in the frame, so its card has room above it.
import * as THREE from 'three'

export type Rest = { az: number; polar: number; r: number; fov: number }
/** wide: the island sits to the right of the headline · tall: it sits under it */
export const REST: Record<'wide' | 'tall', Rest> = {
  wide: { az: 0.5, polar: 1.135, r: 5.7, fov: 44 },
  tall: { az: 0.3, polar: 1.02, r: 8.6, fov: 50 },
}
/** How far the resting camera wanders, in radians: the slow drift of the orbit, and what the pointer adds on top (pointer at the edge of the window). */
export const DRIFT = { az: 0.075, polar: 0.012 }
export const PARALLAX = { az: 0.05, polar: 0.018 }
export const isWide = (width: number, height: number) => width / height > 1.15
/** the point the resting camera looks at */
export const LOOK = new THREE.Vector3(0.04, 0.02, 0.1)

/** camera position on the orbit */
export function orbit(out: THREE.Vector3, az: number, polar: number, r: number) {
  return out.set(-Math.sin(az) * Math.sin(polar) * r, Math.cos(polar) * r, Math.cos(az) * Math.sin(polar) * r + 0.1)
}

/** hover: the pointer is on it · pin: it was clicked or tapped · tour: the guided tour is showing it */
export type FocusKind = 'hover' | 'pin' | 'tour'
export type Focus = { id: string; kind: FocusKind }

/**
 * How far the camera travels toward what it is looking at, as a share of the distance.
 * 0.44 brings a landmark to 1 / (1 − 0.44) ≈ 1.8× its resting size. The tour only leans:
 * the visitor did not ask for that move, so it stays a drift rather than a zoom.
 */
export const LEAN: Record<'wide' | 'tall', Record<FocusKind, number>> = {
  wide: { hover: 0.44, pin: 0.44, tour: 0.13 },
  tall: { hover: 0.5, pin: 0.5, tour: 0 },
}
/**
 * The lean for one subject. On a tall screen a subject that is itself tall (the Lotus Tower, Sri Pada) is approached
 * less: it has to fit between the copy and the layer bar with its card above it. `height` is in scene units.
 */
export const leanFor = (shape: 'wide' | 'tall', kind: FocusKind, height: number) => (shape === 'tall' ? Math.min(LEAN.tall[kind], Math.max(0, 1 - height / 0.62)) : LEAN.wide[kind])
/** Tall screens: where a focused landmark comes to rest, in NDC. Centred, and as low as the layer bar allows: all the room above it is the card's. */
export const TALL_SPOT = { x: 0, y: -0.78 }
/** Spring response in seconds. Critically damped, so this is roughly the time to settle. */
export const RESPONSE: Record<FocusKind | 'release', number> = { hover: 0.85, pin: 0.85, tour: 2.4, release: 1.3 }

/**
 * One step of a critically damped spring, solved implicitly: stable for any frame time,
 * never overshoots, and keeps its velocity when the target moves. That last part is what
 * makes the focus interruptible: a new target bends the motion, it never restarts it.
 */
export function springTo(s: { x: number; v: number }, target: number, response: number, dt: number) {
  const w = (2 * Math.PI) / response
  const f = 1 + 2 * dt * w
  const hw = dt * w * w
  const det = 1 / (f + dt * hw)
  const x = (f * s.x + dt * s.v + dt * hw * target) * det
  s.v = (s.v + hw * (target - s.x)) * det
  s.x = x
}

const _p = new THREE.Vector3()
const _look = new THREE.Vector3()
const _lean = new THREE.Vector3()
const _ndc = new THREE.Vector3()
const _right = new THREE.Vector3()
const _up = new THREE.Vector3()

/**
 * Pose `cam` from a resting position and look-at, leaned `lean` of the way toward `point`,
 * then slid sideways so `point` sits `pull` of the way to the tall-screen spot.
 * Allocates nothing: the rig calls this every frame.
 */
export function poseCamera(cam: THREE.PerspectiveCamera, rest: THREE.Vector3, look: THREE.Vector3, point: THREE.Vector3, lean: number, pull: number) {
  _lean.copy(point).sub(rest).multiplyScalar(lean)
  cam.position.copy(rest).add(_lean)
  cam.lookAt(_look.copy(look).add(_lean))
  if (pull < 0.001) return
  cam.updateMatrixWorld()
  _ndc.copy(point).project(cam)
  const depth = -_lean.copy(point).applyMatrix4(cam.matrixWorldInverse).z
  const e = cam.matrixWorld.elements, p = cam.projectionMatrix.elements
  _right.set(e[0], e[1], e[2])
  _up.set(e[4], e[5], e[6])
  // moving the camera by δ along its own right axis moves the point by −δ·p[0]/depth in NDC
  cam.position.addScaledVector(_right, (-(TALL_SPOT.x - _ndc.x) * pull * depth) / p[0]).addScaledVector(_up, (-(TALL_SPOT.y - _ndc.y) * pull * depth) / p[5])
}

/**
 * Put `cam` where the hero camera comes to rest for a given focus: the middle of its drift, or `daz` / `dpolar`
 * radians away from it. `cam` must already carry the live camera's projection.
 */
export function settledPose(cam: THREE.PerspectiveCamera, wide: boolean, point: THREE.Vector3 | null, kind: FocusKind | null, height = 0, daz = 0, dpolar = 0) {
  const rest = REST[wide ? 'wide' : 'tall']
  orbit(_p, rest.az + daz, rest.polar + dpolar, rest.r)
  const lean = kind ? leanFor(wide ? 'wide' : 'tall', kind, height) : 0
  // the same rule the rig follows: no lean, no slide
  if (point && lean > 0) poseCamera(cam, _p, LOOK, point, lean, wide ? 0 : 1)
  else {
    cam.position.copy(_p)
    cam.lookAt(LOOK)
  }
  cam.updateMatrixWorld()
}
