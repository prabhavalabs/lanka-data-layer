// What the hero shows while the island is on its way: a glass cube on a turntable,
// with a mono status line fed by the real load stages. Pure CSS 3D, so it paints
// with the first frame and never waits on the three.js chunk it is announcing.
import { useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

/** Real milestones, in order. Each owns a slice of the bar; the number eases toward the end of its slice. */
export const LOAD_STAGES = [
  { label: 'loading renderer', until: 34 },
  { label: 'fetching coastline', until: 56 },
  { label: 'building terrain', until: 82 },
  { label: 'compiling shaders', until: 96 },
] as const

const FACES = ['front', 'back', 'right', 'left', 'top', 'bottom'] as const

export function HeroLoader({ stage, done, hold }: { stage: number; done: boolean; hold?: number }) {
  const num = useRef<HTMLSpanElement>(null)
  const bar = useRef<HTMLSpanElement>(null)
  const live = useRef({ stage, done })
  live.current = { stage, done }

  // The readout moves every frame, so it is written straight to the DOM: no re-render per tick.
  useEffect(() => {
    let raf = 0
    let v = hold ?? 0
    let last = performance.now()
    let shown = -1
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000)
      last = now
      const { stage: s, done: d } = live.current
      const target = hold ?? (d ? 100 : LOAD_STAGES[Math.min(s, LOAD_STAGES.length - 1)].until)
      // approach each milestone asymptotically: always moving, never claiming more than has happened
      v += (target - v) * (1 - Math.exp(-dt * (d ? 16 : 2.4)))
      const n = d && target - v < 0.6 ? 100 : Math.floor(v)
      if (n !== shown) {
        shown = n
        if (num.current) num.current.textContent = String(n).padStart(2, '0')
        if (bar.current) bar.current.style.transform = `scaleX(${n / 100})`
      }
      if (n < 100) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [hold])

  const label = done ? 'first light' : LOAD_STAGES[Math.min(stage, LOAD_STAGES.length - 1)].label

  return (
    <div className={cn('hero-loader', done && 'is-done')} role="status" aria-live="polite" aria-label={`Loading the island: ${label}`}>
      <div className="hero-loader-stage" aria-hidden>
        <span className="hero-loader-glow" />
        <div className="hero-cube">
          {FACES.map((f) => (
            <span key={f} className={`hero-cube-face is-${f}`} />
          ))}
          <div className="hero-cube-core">
            {FACES.map((f) => (
              <span key={f} className={`hero-cube-face is-${f}`} />
            ))}
          </div>
        </div>
        <div className="hero-cube-orbit">
          <span />
        </div>
        <span className="hero-loader-shadow" />
      </div>
      <div className="hero-loader-readout" aria-hidden>
        <span className="hero-loader-label">{label}</span>
        <span className="text-ink3/60">·</span>
        <span className="t-num text-ink2">
          <span ref={num}>00</span>%
        </span>
      </div>
      <span className="hero-loader-track" aria-hidden>
        <span ref={bar} />
      </span>
    </div>
  )
}
