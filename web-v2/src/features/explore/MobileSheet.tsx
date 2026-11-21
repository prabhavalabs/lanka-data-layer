import { ChevronUp, Pause, Play, SkipForward } from 'lucide-react'
import { useCallback, useEffect, useRef, type ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { LegendStrip } from './Legend'
import { LEVEL_NAME, unitSuffix, type MapModel } from './model'
import { useSheetDrag } from './useSheetDrag'

/** Height of the always-visible summary, excluding the safe-area inset. */
export const SHEET_PEEK = 124
/** Share of the viewport the sheet takes at its middle detent: the controls and the map, both in view. */
const HALF = 0.54

/**
 * Mobile controls: a summary bar that is the top of a bottom sheet.
 *
 * Three detents: the summary alone, half the screen (controls with the map still in view), and
 * the whole sheet. It is dragged by its header at any time; below the top detent the content
 * drags it too, and at the top detent the content scrolls, handing back to the sheet when pulled
 * down from its start. The sheet is laid out once and only translated, never resized.
 */
export function MobileSheet({
  model,
  year,
  playing,
  onPlay,
  reducedMotion,
  onRest,
  children,
}: {
  model: MapModel | null
  year: number
  playing: boolean
  onPlay: () => void
  reducedMotion: boolean
  /** the sheet has come to rest showing this many px (never more than the middle detent: the map is not asked to fit in a sliver) */
  onRest?: (visible: number) => void
  children: ReactNode
}) {
  const sheet = useRef<HTMLElement>(null)
  const body = useRef<HTMLDivElement>(null)
  const inset = useRef<HTMLDivElement>(null)
  const detents = useCallback(() => {
    const full = sheet.current?.offsetHeight ?? 0
    const peek = Math.min(full, SHEET_PEEK + (inset.current?.offsetHeight ?? 0))
    const half = Math.round(window.innerHeight * HALF)
    // a middle stop too close to either end is no stop at all
    return half - peek < 96 || full - half < 96 ? [peek, full] : [peek, half, full]
  }, [])
  const { index, snapTo, handlers } = useSheetDrag(sheet, body, {
    detents,
    initial: 0,
    reducedMotion,
    onSettle: (_i, visible) => onRest?.(Math.min(visible, Math.round(window.innerHeight * HALF))),
    // the content comes up out of nothing as the sheet leaves its lowest stop, so none of it shows in the strip behind the home indicator
    onMove: (visible) => {
      if (body.current) body.current.style.opacity = String(Math.min(1, Math.max(0, (visible - detents()[0]) / 72)))
    },
  })
  const open = index > 0
  const top = index === detents().length - 1 && open
  // below the top stop the content cannot be scrolled, so it goes back to its start rather than sitting half-way down
  useEffect(() => {
    if (!top) body.current?.scrollTo({ top: 0, behavior: reducedMotion ? 'auto' : 'smooth' })
  }, [top, reducedMotion])
  return (
    <section
      ref={sheet}
      aria-label="Map controls"
      {...handlers}
      className="explore-solid glass fixed inset-x-0 bottom-0 z-30 flex max-h-[84dvh] flex-col rounded-t-[20px] border-x-0 border-b-0 will-change-transform"
      // before the first measure: the same resting place, in CSS, so nothing flashes on load
      style={{ transform: `translateY(calc(100% - ${SHEET_PEEK}px - env(safe-area-inset-bottom)))` }}
    >
      <div className="shrink-0 cursor-grab touch-none px-4 select-none [[data-dragging]_&]:cursor-grabbing" style={{ height: SHEET_PEEK }}>
        <div className="flex h-4 items-end justify-center" aria-hidden>
          <span className="h-1 w-9 rounded-full bg-line2 transition-[background-color,width] duration-200 ease-[var(--ease-out)] [[data-dragging]_&]:w-11 [[data-dragging]_&]:bg-ink3" />
        </div>
        <div className="flex h-[52px] items-center gap-2">
          <button
            type="button"
            onClick={() => snapTo(open ? 0 : 1)}
            aria-expanded={open}
            aria-label={open ? 'Collapse controls' : 'Expand controls'}
            className="pressable -ml-1.5 flex h-11 min-w-0 flex-1 items-center gap-2 rounded-lg pl-1.5 text-left"
          >
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
      {/* measures the home-indicator inset, which the summary sits clear of while the sheet is down */}
      <div ref={inset} className="pointer-events-none absolute h-[env(safe-area-inset-bottom)] w-px" aria-hidden />
      <div
        ref={body}
        // below the top detent a drag anywhere moves the sheet; at the top the content scrolls, and gives way to the sheet when pulled down from its start
        className={cn('scroll-thin min-h-0 flex-1 overscroll-contain border-t border-glass-line pb-[env(safe-area-inset-bottom)]', top ? 'touch-pan-y overflow-y-auto' : 'touch-none overflow-hidden')}
        inert={!open}
      >
        {children}
      </div>
    </section>
  )
}
