// Night ↔ day blending for the scene's own shaders. Every theme colour is parsed once;
// the frame loop only ever blends two parsed colours into a uniform that already exists.
import * as THREE from 'three'

const parsed = new Map<string, THREE.Color>()
export const C = (hex: string) => {
  let c = parsed.get(hex)
  if (!c) parsed.set(hex, (c = new THREE.Color(hex)))
  return c
}
/** out = night → day at t */
export const tween = (out: THREE.Color, night: string, day: string, t: number) => out.lerpColors(C(night), C(day), t)
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t
export const tweenDir = (out: THREE.Vector3, night: readonly number[], day: readonly number[], t: number) =>
  out.set(lerp(night[0], day[0], t), lerp(night[1], day[1], t), lerp(night[2], day[2], t)).normalize()
