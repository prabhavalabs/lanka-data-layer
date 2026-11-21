// Drag physics for a bottom sheet.
//
// The sheet is laid out once at its full height and only ever translated. Its position is
// "how much of it is showing", in px, and rests at one of a few detents. A finger moves it
// 1:1 from wherever it took hold, past the ends it stretches like rubber, and on release
// it is thrown: the resting detent is chosen from where the gesture was heading, not from
// where it happened to stop, and a spring carries it there starting at the finger's own
// velocity. Touching a sheet in flight catches it where it is.
//
// Nothing here goes through React while the sheet moves: the transform is written straight
// to the element. React hears about it once, when the sheet has come to rest.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'

/** px a pointer has to travel before a press becomes a drag, so taps on the header still land */
const SLOP = 6
/**
 * How far a throw carries, as seconds of the release velocity: Apple's projection, v · d / (1 − d), with a
 * deceleration rate of 0.9965. Scroll views use 0.998; a sheet with a middle stop wants a little less, or
 * every ordinary flick would sail past it.
 */
const THROW = 0.9965 / (1 - 0.9965) / 1000
/** px/s above which a release counts as a flick and has to travel at least one detent */
const FLICK = 420
/** spring response in seconds, and damping: critically damped at rest, a touch under when the hand gave it momentum */
const RESPONSE = 0.36
const OMEGA = (2 * Math.PI) / RESPONSE

/** the further past the end, the less it follows */
const rubber = (over: number, dim: number) => (over * dim * 0.55) / (dim + 0.55 * over)

/** controls a drag must not steal: they have a horizontal or text gesture of their own */
const OWN_GESTURE = 'input, textarea, select, [data-slot="slider"], [role="slider"], [data-sheet-nodrag]'

export type SheetOptions = {
  /** how much of the sheet shows at each detent, ascending, in px. Read when needed, so it can depend on the viewport. */
  detents: () => number[]
  /** the detent it starts at */
  initial: number
  reducedMotion: boolean
  /** the sheet has come to rest */
  onSettle?: (index: number, visible: number) => void
  /** every time the sheet is moved, with the px showing. For compositor-only follow-ons (a fade); must not touch layout. */
  onMove?: (visible: number) => void
}

export function useSheetDrag(sheet: RefObject<HTMLElement | null>, scroller: RefObject<HTMLElement | null>, options: SheetOptions) {
  const opts = useRef(options)
  opts.current = options
  const [index, setIndex] = useState(options.initial)
  const live = useRef({
    /** px of the sheet showing right now */
    v: -1,
    /** px/s, positive while more of it is coming into view */
    vel: 0,
    index: options.initial,
    raf: 0,
    last: 0,
    target: 0,
    zeta: 1,
  })
  const drag = useRef<{ id: number; y0: number; v0: number; active: boolean; t: number[]; y: number[] } | null>(null)
  const suppressClick = useRef(false)

  const write = useCallback(
    (v: number) => {
      const el = sheet.current
      if (!el) return
      live.current.v = v
      el.style.transform = `translate3d(0, ${Math.max(0, el.offsetHeight - v).toFixed(2)}px, 0)`
      opts.current.onMove?.(v)
    },
    [sheet],
  )

  const stop = useCallback(() => {
    cancelAnimationFrame(live.current.raf)
    live.current.raf = 0
  }, [])

  const settle = useCallback(
    (i: number) => {
      const s = live.current
      const d = opts.current.detents()
      s.index = Math.min(i, d.length - 1)
      s.vel = 0
      write(d[s.index])
      setIndex(s.index)
      opts.current.onSettle?.(s.index, d[s.index])
    },
    [write],
  )

  /** spring to detent `i` from the current position, carrying `vel` px/s */
  const springTo = useCallback(
    (i: number, vel = 0) => {
      const s = live.current
      const d = opts.current.detents()
      i = Math.max(0, Math.min(d.length - 1, i))
      stop()
      if (opts.current.reducedMotion || s.v < 0) return settle(i)
      // what is inside becomes reachable (or stops being) as the move starts, not when it lands
      s.index = i
      setIndex(i)
      s.target = d[i]
      s.vel = vel
      // a bounce only when the hand gave it momentum: a sheet that was merely asked to open should not overshoot
      s.zeta = Math.abs(vel) > FLICK ? 0.82 : 1
      s.last = performance.now()
      const step = (now: number) => {
        let dt = Math.min(0.05, (now - s.last) / 1000)
        s.last = now
        let x = s.v
        // fixed sub-steps: stable whatever the frame time
        while (dt > 0) {
          const h = Math.min(dt, 1 / 240)
          s.vel += (-OMEGA * OMEGA * (x - s.target) - 2 * s.zeta * OMEGA * s.vel) * h
          x += s.vel * h
          dt -= h
        }
        if (Math.abs(x - s.target) < 0.4 && Math.abs(s.vel) < 12) {
          s.raf = 0
          return settle(i)
        }
        write(x)
        s.raf = requestAnimationFrame(step)
      }
      s.raf = requestAnimationFrame(step)
    },
    [settle, stop, write],
  )

  /* ---- the gesture, fed by pointer events on the sheet and by touch events on its scroller ---- */
  const begin = (id: number, y: number) => {
    const s = live.current
    // catch a sheet in flight exactly where it is
    const caught = s.raf !== 0
    stop()
    drag.current = { id, y0: y, v0: s.v, active: caught, t: [performance.now()], y: [y] }
    if (caught) sheet.current?.setAttribute('data-dragging', '')
  }
  const move = (y: number) => {
    const g = drag.current
    if (!g) return false
    if (!g.active) {
      if (Math.abs(y - g.y0) < SLOP) return false
      // take hold from here, so the sheet does not jump by the slop
      g.active = true
      g.y0 = y
      g.v0 = live.current.v
      sheet.current?.setAttribute('data-dragging', '')
    }
    const d = opts.current.detents()
    const min = d[0], max = d[d.length - 1]
    let v = g.v0 - (y - g.y0)
    if (v > max) v = max + rubber(v - max, 160)
    else if (v < min) v = min - rubber(min - v, 160)
    write(v)
    const now = performance.now()
    g.t.push(now)
    g.y.push(y)
    // velocity is read over the last ~90 ms of the gesture
    while (g.t.length > 2 && now - g.t[0] > 90) {
      g.t.shift()
      g.y.shift()
    }
    return true
  }
  const end = () => {
    const g = drag.current
    drag.current = null
    sheet.current?.removeAttribute('data-dragging')
    if (!g || !g.active) return false
    const s = live.current
    const n = g.t.length
    const dt = (g.t[n - 1] - g.t[0]) / 1000
    // a finger that stopped before lifting has no velocity to hand over
    const vel = n > 1 && dt > 0.008 && performance.now() - g.t[n - 1] < 80 ? -(g.y[n - 1] - g.y[0]) / dt : 0
    const d = opts.current.detents()
    const projected = s.v + vel * THROW
    let target = 0
    for (let i = 1; i < d.length; i++) if (Math.abs(d[i] - projected) < Math.abs(d[target] - projected)) target = i
    // a flick goes at least one detent the way it was thrown, however short the stroke
    if (Math.abs(vel) > FLICK) {
      if (vel > 0 && d[target] <= s.v) target = d.findIndex((x) => x > s.v + 1)
      else if (vel < 0 && d[target] >= s.v) target = d.findLastIndex((x) => x < s.v - 1)
      if (target < 0) target = vel > 0 ? d.length - 1 : 0
    }
    springTo(target, vel)
    return true
  }

  const onPointerDown = (e: ReactPointerEvent<HTMLElement>) => {
    // one finger owns the sheet: a second touch must not make it jump. (A press that never became a drag is simply replaced.)
    if (drag.current?.active || (e.pointerType === 'mouse' && e.button !== 0)) return
    const target = e.target as HTMLElement
    if (target.closest(OWN_GESTURE)) return
    // Inside the scroller a touch belongs to native scrolling whenever there is something to scroll (the touch handlers
    // below take it back at the top), and a mouse is there to select text and press things, never to drag.
    const sc = scroller.current
    if (sc && sc.contains(target) && (e.pointerType === 'mouse' || getComputedStyle(sc).overflowY !== 'hidden')) return
    begin(e.pointerId, e.clientY)
  }
  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    const g = drag.current
    if (!g || g.id !== e.pointerId) return
    const was = g.active
    if (move(e.clientY) && !was) {
      // from here on the gesture is ours wherever the finger goes
      try {
        e.currentTarget.setPointerCapture(e.pointerId)
      } catch {
        /* the pointer is already gone */
      }
    }
  }
  const onPointerUp = (e: ReactPointerEvent<HTMLElement>) => {
    if (drag.current?.id !== e.pointerId) return
    // a drag that started on a button must not also press it
    if (end()) suppressClick.current = true
  }
  // Taking the capture ourselves makes the element the touch began on (which held it implicitly) lose it, and that
  // event bubbles up to here: only the sheet's own loss means the gesture is over.
  const onLostPointerCapture = (e: ReactPointerEvent<HTMLElement>) => {
    if (e.target === e.currentTarget) onPointerUp(e)
  }
  const onClickCapture = (e: React.MouseEvent) => {
    if (!suppressClick.current) return
    suppressClick.current = false
    e.preventDefault()
    e.stopPropagation()
  }
  // the flag is only for the click that follows this pointer-up, never for a later one
  const onPointerDownCapture = () => void (suppressClick.current = false)

  // Hand-off from the scroller: at the top of its content, a pull down moves the sheet instead of rubber-banding the list.
  // Touch events, not pointer events: only a cancelled touchmove can take a gesture back from native scrolling.
  useEffect(() => {
    const sc = scroller.current
    if (!sc) return
    let y0 = 0, eligible = false, ours = false
    const start = (e: TouchEvent) => {
      if (e.touches.length !== 1 || drag.current) return
      y0 = e.touches[0].clientY
      ours = false
      eligible = getComputedStyle(sc).overflowY !== 'hidden' && sc.scrollTop <= 0 && !(e.target as HTMLElement).closest(OWN_GESTURE)
    }
    const onMove = (e: TouchEvent) => {
      if (!eligible) return
      const y = e.touches[0].clientY
      if (!ours) {
        if (y < y0 - 2 || sc.scrollTop > 0) return void (eligible = false) // scrolling down the list: leave it to the browser
        if (y <= y0 || !e.cancelable) return
        ours = true
        begin(-1, y0)
      }
      e.preventDefault()
      move(y)
    }
    const finish = () => {
      if (ours) end()
      eligible = ours = false
    }
    sc.addEventListener('touchstart', start, { passive: true })
    sc.addEventListener('touchmove', onMove, { passive: false })
    sc.addEventListener('touchend', finish)
    sc.addEventListener('touchcancel', finish)
    return () => {
      sc.removeEventListener('touchstart', start)
      sc.removeEventListener('touchmove', onMove)
      sc.removeEventListener('touchend', finish)
      sc.removeEventListener('touchcancel', finish)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scroller])

  // place it before the first paint, and again whenever its own height or the viewport changes
  useLayoutEffect(() => {
    const el = sheet.current
    if (!el) return
    const place = () => {
      if (drag.current || live.current.raf) return
      const d = opts.current.detents()
      write(d[Math.min(live.current.index, d.length - 1)])
    }
    place()
    const ro = new ResizeObserver(place)
    ro.observe(el)
    window.addEventListener('resize', place)
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', place)
      stop()
    }
  }, [sheet, write, stop])

  return {
    /** the detent the sheet rests at, or is on its way to */
    index,
    snapTo: springTo,
    handlers: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp, onLostPointerCapture, onClickCapture, onPointerDownCapture },
  }
}
