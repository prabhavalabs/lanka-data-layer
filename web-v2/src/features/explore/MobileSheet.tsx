import { ChevronUp, Pause, Play, SkipForward } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { LegendStrip } from './Legend'
import { LEVEL_NAME, unitSuffix, type MapModel } from './model'

/** Height of the always-visible summary, excluding the safe-area inset. */
export const SHEET_PEEK = 124

/**
 * Mobile controls: a compact summary bar that expands into a bottom sheet.
 * The whole sheet is laid out once and only translated, never resized.
 */
export function MobileSheet({
  model,
  year,
  playing,
  onPlay,
  reducedMotion,
  children,
}: {
  model: MapModel | null
  year: number
  playing: boolean
  onPlay: () => void
  reducedMotion: boolean
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  return (
    <section
      aria-label="Map controls"
      className="glass explore-solid fixed inset-x-0 bottom-0 z-30 flex max-h-[84dvh] flex-col rounded-t-[20px] border-x-0 border-b-0 transition-transform duration-[400ms] ease-[var(--ease-drawer)] motion-reduce:transition-none"
      style={{ transform: open ? 'translateY(0)' : `translateY(calc(100% - ${SHEET_PEEK}px - env(safe-area-inset-bottom)))` }}
    >
      <div className="shrink-0 px-4" style={{ height: SHEET_PEEK }}>
        <div className="flex h-4 items-end justify-center" aria-hidden>
          <span className="h-1 w-9 rounded-full bg-line2" />
        </div>
        <div className="flex h-[52px] items-center gap-2">
          <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-label={open ? 'Collapse controls' : 'Expand controls'} className="pressable -ml-1.5 flex h-11 min-w-0 flex-1 items-center gap-2 rounded-lg pl-1.5 text-left">
            <ChevronUp className={cn('size-4 shrink-0 text-ink3 transition-transform duration-200 ease-[var(--ease-out)]', open && 'rotate-180')} />
            <span className="min-w-0">
              {model ? (
                <>
                  <span className="block truncate text-[14.5px] leading-tight font-semibold tracking-[-0.015em] text-ink">{model.indicator.label}</span>
                  <span className="t-label mt-1 block truncate">
                    {unitSuffix(model.indicator.unit)} · by {LEVEL_NAME[model.data.level].toLowerCase()}
                  </span>
                </>
              ) : (
                <>
                  <span className="skeleton block h-4 w-36" />
                  <span className="skeleton mt-1.5 block h-3 w-24" />
                </>
              )}
            </span>
          </button>
          <span className="t-num font-mono text-[22px] leading-none font-medium tracking-[-0.04em] text-ink">{year}</span>
          <button
            type="button"
            onClick={onPlay}
            aria-label={reducedMotion ? 'Next year' : playing ? 'Pause' : 'Play through years'}
            className="pressable grid size-11 shrink-0 place-items-center rounded-full border border-line2 bg-bg2/60 text-ink"
          >
            {reducedMotion ? <SkipForward className="size-4" /> : playing ? <Pause className="size-4" /> : <Play className="size-4 translate-x-px" />}
          </button>
        </div>
        <div className="mt-2.5">{model ? <LegendStrip model={model} compact /> : <div className="skeleton h-2.5 w-full" />}</div>
      </div>
      <div className="h-[env(safe-area-inset-bottom)] shrink-0" style={{ display: open ? 'none' : undefined }} />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto border-t border-glass-line pb-[env(safe-area-inset-bottom)]" inert={!open}>
        {children}
      </div>
    </section>
  )
}
