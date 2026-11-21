// Shared chart plumbing: measured width, hover tooltip, legend, bar paths.
import { useCallback, useLayoutEffect, useRef, useState, type PointerEvent, type ReactNode } from 'react'
import { cn } from '@/lib/utils'

/** Container width in CSS px, so SVG text stays the same size at 375 and 1240. */
export function useMeasure<T extends HTMLElement>(initial = 560) {
  const ref = useRef<T | null>(null)
  const [width, setWidth] = useState(initial)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const set = (w: number) => setWidth(Math.max(220, Math.round(w)))
    set(el.clientWidth)
    const ro = new ResizeObserver(([e]) => set(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  return [ref, width] as const
}

export type Tip = { x: number; y: number; body: ReactNode }

/** Tooltip state positioned in the coordinate space of the element the handlers sit on. */
export function useTip() {
  const [tip, setTip] = useState<Tip | null>(null)
  const show = useCallback((e: PointerEvent<Element>, body: ReactNode, at?: { x?: number; y?: number }) => {
    const host = (e.currentTarget.closest('[data-chart]') ?? e.currentTarget) as Element
    const b = host.getBoundingClientRect()
    setTip({ x: at?.x ?? e.clientX - b.left, y: at?.y ?? e.clientY - b.top, body })
  }, [])
  const hide = useCallback(() => setTip(null), [])
  return { tip, show, hide }
}

export function ChartTip({ tip, width }: { tip: Tip | null; width: number }) {
  if (!tip) return null
  const align = tip.x < 110 ? '-12px' : tip.x > width - 110 ? 'calc(-100% + 12px)' : '-50%'
  return (
    <div
      role="status"
      className="pointer-events-none absolute top-0 left-0 z-20 w-max max-w-[240px] rounded-lg border border-line2 bg-bg2 px-2.5 py-2 text-[12px] leading-snug text-ink shadow-soft"
      style={{ transform: `translate3d(calc(${tip.x}px + ${align}), calc(${Math.max(tip.y, 0)}px - 100% - 12px), 0)` }}
    >
      {tip.body}
    </div>
  )
}

export function TipTitle({ children }: { children: ReactNode }) {
  return <div className="mb-1 font-medium text-ink">{children}</div>
}
export function TipRow({ color, label, value, mark = 'square' }: { color?: string; label: ReactNode; value: ReactNode; mark?: 'square' | 'line' | 'outline' | 'none' }) {
  return (
    <div className="flex items-center gap-2 text-ink2">
      {mark !== 'none' && <Swatch color={color ?? 'var(--ink3)'} kind={mark} />}
      <span>{label}</span>
      <span className="t-num ml-auto pl-3 font-medium text-ink">{value}</span>
    </div>
  )
}

export type SwatchKind = 'square' | 'line' | 'outline' | 'band' | 'dashed' | 'tick' | 'dot' | 'ring'
export function Swatch({ color, kind = 'square' }: { color: string; kind?: SwatchKind }) {
  const base = 'inline-block shrink-0'
  switch (kind) {
    case 'line':
      return <span className={cn(base, 'h-0.5 w-3.5 rounded-full')} style={{ background: color }} />
    case 'dashed':
      return <span className={cn(base, 'h-0 w-3.5 border-t-2 border-dashed')} style={{ borderColor: color }} />
    case 'outline':
      return <span className={cn(base, 'size-2.5 rounded-[2px] border-[1.5px]')} style={{ borderColor: color }} />
    case 'band':
      return <span className={cn(base, 'h-2.5 w-3.5 rounded-[2px] opacity-25')} style={{ background: color }} />
    case 'tick':
      return <span className={cn(base, 'h-3 w-0.5 rounded-full')} style={{ background: color }} />
    case 'dot':
      return <span className={cn(base, 'size-2 rounded-full')} style={{ background: color }} />
    case 'ring':
      return <span className={cn(base, 'size-2.5 rounded-full border-[1.5px] bg-bg2')} style={{ borderColor: color }} />
    default:
      return <span className={cn(base, 'size-2.5 rounded-[2px]')} style={{ background: color }} />
  }
}

export type LegendItem = { label: ReactNode; color: string; kind?: SwatchKind }
export function Legend({ items, className }: { items: LegendItem[]; className?: string }) {
  return (
    <ul className={cn('flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[12px] text-ink2', className)}>
      {items.map((it, i) => (
        <li key={i} className="inline-flex items-center gap-1.5">
          <Swatch color={it.color} kind={it.kind} />
          {it.label}
        </li>
      ))}
    </ul>
  )
}

/** Horizontal bar anchored at x0 with only the data end (x1) rounded. */
export function hbar(x0: number, x1: number, y: number, h: number, r = 3) {
  const dir = x1 >= x0 ? 1 : -1
  const rr = Math.min(r, Math.abs(x1 - x0), h / 2)
  return `M${x0} ${y}H${x1 - dir * rr}Q${x1} ${y} ${x1} ${y + rr}V${y + h - rr}Q${x1} ${y + h} ${x1 - dir * rr} ${y + h}H${x0}Z`
}
/** Vertical bar anchored at the baseline with only the top rounded. */
export function vbar(x: number, base: number, top: number, w: number, r = 3) {
  const rr = Math.min(r, Math.abs(base - top), w / 2)
  return `M${x} ${base}V${top + rr}Q${x} ${top} ${x + rr} ${top}H${x + w - rr}Q${x + w} ${top} ${x + w} ${top + rr}V${base}Z`
}

export const AXIS_TEXT = { fontSize: 10.5, fill: 'var(--ink3)', fontFamily: 'var(--font-sans)', style: { fontVariantNumeric: 'tabular-nums' } } as const
