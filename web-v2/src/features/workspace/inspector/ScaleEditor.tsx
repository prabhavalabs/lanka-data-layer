// The one classification UI: every numeric scale in the workspace (choropleth fills, raster ramps,
// point colours) is edited here, over a histogram of the values it classifies.
import { Minus, Plus, X } from 'lucide-react'
import { useMemo, useRef, type PointerEvent as ReactPointerEvent } from 'react'
import { Slider } from '@/components/ui/slider'
import type { Theme } from '@/lib/hooks'
import type { Scale, ScaleType } from '@/lib/map-spec'
import { quantile, tidy } from '@/lib/stats'
import { cn } from '@/lib/utils'
import { colorAt, resolveScale } from '../engine/compile'
import { fmt, unitSuffix } from '../format'
import { Field, RampPicker, Seg, useLive } from './controls'

const BINS = 44
type Mode = 'quantile' | 'equal' | 'jenks' | 'manual' | 'continuous'
const MODES: { value: Mode; label: string; hint: string }[] = [
  { value: 'quantile', label: 'Quantile', hint: 'The same number of values in every class' },
  { value: 'equal', label: 'Equal', hint: 'Classes of the same width' },
  { value: 'jenks', label: 'Jenks', hint: 'Natural breaks: classes that are alike inside and unlike each other' },
  { value: 'manual', label: 'Manual', hint: 'Breaks you set; drag them on the histogram' },
  { value: 'continuous', label: 'Smooth', hint: 'No classes: colour follows the value' },
]

export function ScaleEditor({ scale, values, unit, scheme, onChange, smooth = false }: { scale: Scale; values: number[]; unit: string; scheme: Theme; onChange: (scale: Scale, key?: string) => void; smooth?: boolean }) {
  const mode = (scale.type === 'diverging' || scale.type === 'categorical' ? 'quantile' : scale.type) as Mode
  const resolved = useMemo(() => resolveScale(values, scale, scheme), [values, scale, scheme])
  const sorted = useMemo(() => [...values].sort((a, b) => a - b), [values])
  // the axis ignores the last half-percent at either end, so one outlier does not flatten the rest
  const lo = sorted.length ? Math.min(sorted[0], quantile(sorted, 0.004)) : 0
  const hi = sorted.length ? quantile(sorted, 0.996) : 1
  const span = hi - lo || 1
  const bins = useMemo(() => {
    const n = new Array<number>(BINS).fill(0)
    for (const v of sorted) n[Math.max(0, Math.min(BINS - 1, Math.floor(((v - lo) / span) * BINS)))]++
    const max = Math.max(1, ...n)
    // square-root heights: the long tail of a skewed distribution stays visible
    return n.map((c) => Math.sqrt(c / max))
  }, [sorted, lo, span])
  const breaks = resolved.kind === 'classes' ? resolved.breaks : []
  const classes = scale.classes ?? 5
  const plot = useRef<HTMLDivElement>(null)
  const [clamp, setClamp, endClamp] = useLive<[number, number]>([scale.clamp?.[0] ?? sorted[0] ?? 0, scale.clamp?.[1] ?? sorted[sorted.length - 1] ?? 1])

  const setMode = (m: Mode) => {
    if (m === mode) return
    if (m === 'manual') onChange({ ...scale, type: 'manual', breaks: breaks.map(tidy) })
    else if (m === 'continuous') onChange({ ...scale, type: 'continuous', breaks: undefined, transform: scale.transform ?? 'linear' })
    else onChange({ ...scale, type: m as ScaleType, breaks: undefined, classes: Math.max(2, Math.min(9, mode === 'manual' ? breaks.length + 1 : classes)) })
  }
  const setBreak = (i: number, v: number) => {
    const next = [...breaks]
    next[i] = Math.max(i ? next[i - 1] : -Infinity, Math.min(i < next.length - 1 ? next[i + 1] : Infinity, v))
    onChange({ ...scale, type: 'manual', breaks: next }, 'breaks')
  }
  const drag = (i: number) => (e: ReactPointerEvent<HTMLSpanElement>) => {
    if (mode !== 'manual') return
    e.currentTarget.setPointerCapture(e.pointerId)
    const box = plot.current!.getBoundingClientRect()
    const move = (ev: PointerEvent) => setBreak(i, tidy(lo + (Math.max(0, Math.min(1, (ev.clientX - box.left) / box.width)) * span)))
    const el = e.currentTarget
    const up = () => {
      el.removeEventListener('pointermove', move)
      el.removeEventListener('pointerup', up)
      el.removeEventListener('pointercancel', up)
    }
    el.addEventListener('pointermove', move)
    el.addEventListener('pointerup', up)
    el.addEventListener('pointercancel', up)
  }
  const at = (v: number) => `${Math.max(0, Math.min(100, ((v - lo) / span) * 100))}%`

  return (
    <div className="space-y-3">
      <Seg value={mode} onChange={setMode} label="Classification" options={MODES.filter((m) => smooth || m.value !== 'continuous')} />

      <div>
        <div ref={plot} className="relative h-[68px]">
          <div className="flex h-full items-end gap-px" aria-hidden>
            {bins.map((h, i) => {
              const mid = lo + ((i + 0.5) / BINS) * span
              const out = resolved.kind === 'continuous' && (mid < clamp[0] || mid > clamp[1])
              return <span key={i} className="min-w-0 flex-1 rounded-t-[1.5px] transition-[height] duration-200 ease-[var(--ease-out)]" style={{ height: `${Math.max(h ? 3 : 0, h * 100)}%`, background: colorAt(resolved, mid), opacity: out ? 0.35 : 1 }} />
            })}
          </div>
          {breaks.map((b, i) =>
            b < lo || b > hi ? null : (
              <span
                key={i}
                role={mode === 'manual' ? 'slider' : undefined}
                tabIndex={mode === 'manual' ? 0 : undefined}
                aria-label={mode === 'manual' ? `Break ${i + 1}` : undefined}
                aria-valuenow={mode === 'manual' ? b : undefined}
                aria-valuetext={mode === 'manual' ? `${fmt(b, unit)} ${unitSuffix(unit)}` : undefined}
                onPointerDown={drag(i)}
                onKeyDown={(e) => {
                  if (mode !== 'manual' || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return
                  e.preventDefault()
                  setBreak(i, tidy(b + (e.key === 'ArrowRight' ? 1 : -1) * span * (e.shiftKey ? 0.05 : 0.01)))
                }}
                data-sheet-nodrag
                className={cn('absolute top-0 bottom-0 -ml-[7px] flex w-[15px] justify-center outline-none', mode === 'manual' && 'group/brk cursor-ew-resize touch-none')}
                style={{ left: at(b) }}
              >
                <span className="h-full w-px bg-ink/70" />
                {mode === 'manual' && <span className="absolute -top-1 size-[9px] rounded-full border-[1.5px] border-ink bg-bg2 transition-transform duration-150 ease-out group-focus-visible/brk:ring-4 group-focus-visible/brk:ring-brand/30 group-active/brk:scale-125" />}
              </span>
            ),
          )}
        </div>
        <div className="mt-1.5 flex justify-between font-mono text-[10px] leading-none text-ink3">
          <span className="t-num">{fmt(lo, unit)}</span>
          <span>{values.length.toLocaleString('en')} values</span>
          <span className="t-num">
            {fmt(hi, unit)} {unitSuffix(unit)}
          </span>
        </div>
      </div>

      {resolved.kind === 'classes' && (
        <div className="flex flex-wrap items-center gap-1" aria-label="Class breaks">
          {mode === 'manual' ? (
            <>
              {breaks.map((b, i) => (
                <span key={i} className="flex h-7 items-center rounded-md border border-line2 bg-bg2/60 pl-1.5 max-md:h-9">
                  <span className="mr-1 size-2 shrink-0 rounded-[2px]" style={{ background: resolved.colors[i + 1] }} />
                  <input
                    type="number"
                    value={b}
                    step="any"
                    aria-label={`Break ${i + 1}`}
                    data-sheet-nodrag
                    onChange={(e) => e.target.value !== '' && setBreak(i, Number(e.target.value))}
                    className="t-num w-[58px] bg-transparent font-mono text-[11.5px] text-ink outline-none [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none"
                  />
                  <button type="button" disabled={breaks.length <= 1} onClick={() => onChange({ ...scale, breaks: breaks.filter((_, k) => k !== i) })} aria-label={`Remove break ${i + 1}`} className="grid h-full w-6 place-items-center text-ink3 outline-none disabled:opacity-30 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
                    <X className="size-3" />
                  </button>
                </span>
              ))}
              <button
                type="button"
                disabled={breaks.length >= 8}
                onClick={() => {
                  // split the widest class
                  const edges = [lo, ...breaks, hi]
                  let k = 0
                  for (let i = 1; i < edges.length - 1; i++) if (edges[i + 1] - edges[i] > edges[k + 1] - edges[k]) k = i
                  onChange({ ...scale, breaks: [...breaks, tidy((edges[k] + edges[k + 1]) / 2)].sort((a, b) => a - b) })
                }}
                className="pressable flex h-7 items-center gap-1 rounded-md border border-dashed border-line2 px-2 text-[12px] text-ink2 outline-none disabled:opacity-40 max-md:h-9 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink"
              >
                <Plus className="size-3" />
                Break
              </button>
            </>
          ) : (
            <>
              <span className="mr-auto flex min-w-0 flex-wrap gap-x-2 gap-y-1 font-mono text-[11px] text-ink2">
                {breaks.map((b, i) => (
                  <span key={i} className="t-num inline-flex items-center gap-1">
                    <span className="size-2 rounded-[2px]" style={{ background: resolved.colors[i + 1] }} />
                    {fmt(b, unit)}
                  </span>
                ))}
              </span>
              <span className="flex h-7 shrink-0 items-center rounded-md border border-line2 bg-bg2/60 max-md:h-9" role="group" aria-label="Number of classes">
                <button type="button" disabled={classes <= 2} onClick={() => onChange({ ...scale, classes: classes - 1 })} aria-label="Fewer classes" className="grid h-full w-7 place-items-center text-ink2 outline-none disabled:opacity-30 max-md:w-9">
                  <Minus className="size-3" />
                </button>
                <span className="t-num w-4 text-center font-mono text-[11.5px] text-ink">{classes}</span>
                <button type="button" disabled={classes >= 9} onClick={() => onChange({ ...scale, classes: classes + 1 })} aria-label="More classes" className="grid h-full w-7 place-items-center text-ink2 outline-none disabled:opacity-30 max-md:w-9">
                  <Plus className="size-3" />
                </button>
              </span>
            </>
          )}
        </div>
      )}

      {resolved.kind === 'continuous' && sorted.length > 0 && (
        <>
          <Field label="Stretch">
            <Seg
              value={scale.transform ?? 'linear'}
              onChange={(t) => onChange({ ...scale, transform: t })}
              label="Transform"
              options={[
                { value: 'linear', label: 'Linear' },
                { value: 'sqrt', label: 'Root' },
                { value: 'log', label: 'Log', disabled: sorted[0] <= 0 && (scale.clamp?.[0] ?? 0) <= 0 },
              ]}
            />
          </Field>
          <Field label="Clamp" stack hint={`${fmt(clamp[0], unit)} – ${fmt(clamp[1], unit)}`}>
            <Slider
              className="max-md:h-8"
              min={0}
              max={1000}
              step={1}
              minStepsBetweenThumbs={10}
              aria-label="Colour range"
              value={[((clamp[0] - sorted[0]) / (sorted[sorted.length - 1] - sorted[0] || 1)) ** 0.5 * 1000, ((clamp[1] - sorted[0]) / (sorted[sorted.length - 1] - sorted[0] || 1)) ** 0.5 * 1000]}
              onValueChange={([a, b]) => {
                // the slider is square-root spaced, so the low end of a skewed range has room
                const un = (t: number) => tidy(sorted[0] + (t / 1000) ** 2 * (sorted[sorted.length - 1] - sorted[0]))
                const next: [number, number] = [un(a), un(b)]
                setClamp(next)
                onChange({ ...scale, clamp: next }, 'clamp')
              }}
              onValueCommit={endClamp}
            />
          </Field>
        </>
      )}

      <Field label="Ramp">
        <RampPicker value={scale.ramp ?? 'ldl.seq.blue'} reverse={!!scale.reverse} scheme={scheme} onChange={(ramp) => onChange({ ...scale, ramp })} onReverse={(reverse) => onChange({ ...scale, reverse })} />
      </Field>
    </div>
  )
}
