// What the layers of the hero scene know about each other: where every landmark and
// port stands (so the camera can lean toward one, and a card can keep clear of the
// rest), and which part of the canvas is free for a card to open into.
import { createContext, useContext } from 'react'
import * as THREE from 'three'
import { DRIFT, isWide, PARALLAX, settledPose, type Focus } from './camera'

/** Something that stands on the island or the sea: a landmark, a port beacon. World units. */
export type Anchor = {
  x: number
  /** ground level */
  y: number
  z: number
  /** footprint radius */
  r: number
  /** height above the ground */
  h: number
}
/** The part of the canvas a card may cover, in px from the canvas's top-left corner. */
export type SafeRect = { left: number; top: number; right: number; bottom: number }

export type Stage = {
  /** keyed by landmark id, or `port:<id>` */
  anchors: Map<string, Anchor>
  /** read when a card opens, so it always reflects the current layout */
  safe: () => SafeRect | null
}
export const StageContext = createContext<Stage | null>(null)
export const useStage = () => useContext(StageContext)

/**
 * Where a card hangs, relative to the point its <Html> is pinned to (the top of the model).
 * `ax` / `ay` say which point of the card sits there, as a fraction of its own size, so the
 * card never has to be measured: 0.5 / 1 hangs it by the middle of its bottom edge.
 */
export type CardPlacement = { x: number; y: number; ax: number; ay: number; /** where the leader line starts, px from the pin */ lx: number; ly: number }
export const CARD_ABOVE: CardPlacement = { x: 0, y: -28, ax: 0.5, ay: 1, lx: 0, ly: 0 }

type Rect = { l: number; t: number; r: number; b: number }
const overlap = (a: Rect, b: Rect) => Math.max(0, Math.min(a.r, b.r) - Math.max(a.l, b.l)) * Math.max(0, Math.min(a.b, b.b) - Math.max(a.t, b.t))

const _cam = new THREE.PerspectiveCamera()
const _v = new THREE.Vector3()
const _focus = new THREE.Vector3()

/** screen rectangle of an anchor as `cam` sees it, grown by `k` (the active model pops a little) */
function screenRect(a: Anchor, cam: THREE.PerspectiveCamera, w: number, h: number, k = 1): Rect & { cx: number; top: number; base: number; rw: number } {
  _v.set(a.x, a.y, a.z).project(cam)
  const cx = (_v.x * 0.5 + 0.5) * w, base = (1 - (_v.y * 0.5 + 0.5)) * h
  _v.set(a.x, a.y + a.h * k, a.z).project(cam)
  const top = (1 - (_v.y * 0.5 + 0.5)) * h
  // px per world unit at that depth
  const depth = _v.set(a.x, a.y, a.z).applyMatrix4(cam.matrixWorldInverse).z
  const rw = ((a.r * k * cam.projectionMatrix.elements[5]) / Math.max(0.1, -depth)) * h * 0.5
  return { l: cx - rw, r: cx + rw, t: top, b: base + rw * 0.4, cx, top, base, rw }
}

/**
 * Choose where the card for `id` opens: the spot that covers the least of the other
 * landmarks and ports, stays inside the safe area, and is closest to "straight above".
 * Solved once, for the pose the camera is heading to, so the card does not hop about
 * while the camera is still moving.
 */
export function placeCard(stage: Stage, live: THREE.PerspectiveCamera, width: number, height: number, id: string, focus: Focus | null, animate: boolean, card: { w: number; h: number }, pop = 0): CardPlacement {
  const self = stage.anchors.get(id)
  const safe = stage.safe()
  if (!self || !safe) return CARD_ABOVE

  const wide = isWide(width, height)
  const focused = animate && focus && stage.anchors.get(focus.id)
  _cam.copy(live, false)
  if (focused) _focus.set(focused.x, focused.y + focused.h * 0.45, focused.z)
  const pose = (daz = 0, dpolar = 0) => settledPose(_cam, wide, focused ? _focus : null, focused ? focus.kind : null, focused ? focused.h : 0, daz, dpolar)

  // The camera never quite rests: it drifts, and the pointer leans it. The card rides with its landmark, so first find how
  // far that can carry it each way, and keep that much further inside the safe area.
  let exL = 0, exR = 0, exT = 0, exB = 0
  if (animate) {
    const top = 1 + pop
    pose()
    const at = screenRect(self, _cam, width, height, top)
    for (const sa of [-1, 1])
      for (const sp of [-1, 1]) {
        pose(sa * (DRIFT.az + PARALLAX.az), sp * (DRIFT.polar + PARALLAX.polar))
        const r = screenRect(self, _cam, width, height, top)
        exL = Math.max(exL, at.cx - r.cx)
        exR = Math.max(exR, r.cx - at.cx)
        exT = Math.max(exT, at.top - r.top)
        exB = Math.max(exB, r.top - at.top)
      }
  }
  pose()

  const me = screenRect(self, _cam, width, height, 1 + pop)
  // the pin sits a little above the model's top
  const pin = { x: me.cx, y: me.top - 6 }
  const others: { rect: Rect; weight: number }[] = []
  for (const [key, a] of stage.anchors) {
    if (key === id) continue
    // covering a landmark costs far more than a longer leader or a less obvious side; a port marker is a small thing to cover
    others.push({ rect: screenRect(a, _cam, width, height), weight: key.startsWith('port:') ? 0.6 : 3 })
  }

  const mid = (me.top + me.base) / 2
  const side = me.rw + 20
  type Cand = { x: number; y: number; ax: number; ay: number; lx: number; ly: number; bias: number }
  const cands: Cand[] = []
  // above, at a few heights: a longer leader is a small price for clearing a neighbour. A phone has no height to spare, so its leaders are short.
  const lift = width < 640 ? 0.5 : 1
  for (const [reach, bias] of [[28, 0], [58, 700], [92, 1600], [128, 2800]] as const) {
    const gap = reach * lift
    cands.push({ x: pin.x, y: pin.y - gap, ax: 0.5, ay: 1, lx: pin.x, ly: pin.y, bias })
    cands.push({ x: pin.x + 16, y: pin.y - gap, ax: 0.16, ay: 1, lx: pin.x, ly: pin.y, bias: bias + 500 })
    cands.push({ x: pin.x - 16, y: pin.y - gap, ax: 0.84, ay: 1, lx: pin.x, ly: pin.y, bias: bias + 500 })
  }
  // beside
  for (const ay of [0.5, 0.82, 0.2]) {
    cands.push({ x: me.cx + side, y: mid, ax: 0, ay, lx: me.cx + me.rw * 0.5, ly: mid, bias: 2200 + (ay === 0.5 ? 0 : 500) })
    cands.push({ x: me.cx - side, y: mid, ax: 1, ay, lx: me.cx - me.rw * 0.5, ly: mid, bias: 2200 + (ay === 0.5 ? 0 : 500) })
  }
  // below and to one side: over the foreground sea, for something on the near shore with the island behind it
  cands.push({ x: me.cx + side * 0.6, y: me.b + 18, ax: 0, ay: 0, lx: me.cx + me.rw * 0.4, ly: me.b, bias: 3000 })
  cands.push({ x: me.cx - side * 0.6, y: me.b + 18, ax: 1, ay: 0, lx: me.cx - me.rw * 0.4, ly: me.b, bias: 3000 })
  // straight below: last resort, the foreground is where the nearest landmarks are
  cands.push({ x: me.cx, y: me.b + 24, ax: 0.5, ay: 0, lx: me.cx, ly: me.b, bias: 5000 })

  let best: Cand | null = null, bestScore = Infinity, bx = 0, by = 0
  for (const c of cands) {
    let l = c.x - c.ax * card.w, t = c.y - c.ay * card.h
    // slide into the safe area, less the room the drift needs
    const dx = Math.max(safe.left + exL - l, 0) + Math.min(safe.right - exR - (l + card.w), 0)
    const dy = Math.max(safe.top + exT - t, 0) + Math.min(safe.bottom - exB - (t + card.h), 0)
    l += dx
    t += dy
    const rect = { l, t, r: l + card.w, b: t + card.h }
    let score = c.bias + (Math.abs(dx) + Math.abs(dy)) * 60
    // a card that has been slid onto its own model is the worst outcome of all
    score += overlap(rect, me) * 4
    for (const o of others) score += overlap(rect, o.rect) * o.weight
    if (score < bestScore) {
      bestScore = score
      best = c
      bx = dx
      by = dy
    }
  }
  if (!best) return CARD_ABOVE
  return { x: best.x + bx - pin.x, y: best.y + by - pin.y, ax: best.ax, ay: best.ay, lx: best.lx - pin.x, ly: best.ly - pin.y }
}
