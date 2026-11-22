import { ChevronUp, Plus } from 'lucide-react'
import { useCallback, useEffect, useRef, type ReactNode } from 'react'
import { useSheetDrag } from '@/features/explore/useSheetDrag'
import { cn } from '@/lib/utils'
import { useWs } from './store'

/** Height of the always-visible summary, excluding the safe-area inset. */
export const SHEET_PEEK = 76
const HALF = 0.56

/**
 * On a phone the docks become one bottom sheet with three stops: the summary, half the screen (the
 * panel with the map still in view) and the whole sheet. Dragged by its header at any time; below
 * the top stop the content drags it too, at the top stop the content scrolls and hands back to the
 * sheet when pulled down from its start. Laid out once and only translated.
 */
export function WorkspaceSheet({ reducedMotion, onRest, onAdd, snap, children }: { reducedMotion: boolean; onRest: (visible: number) => void; onAdd: () => void; /** bumping this opens the sheet to its middle stop */ snap: number; children: ReactNode }) {
  const title = useWs((s) => s.doc!.title)
  const dirty = useWs((s) => s.doc !== s.saved)
  const sheet = useRef<HTMLElement>(null)
  const body = useRef<HTMLDivElement>(null)
  const inset = useRef<HTMLDivElement>(null)
  const detents = useCallback(() => {
    const full = sheet.current?.offsetHeight ?? 0
    const peek = Math.min(full, SHEET_PEEK + (inset.current?.offsetHeight ?? 0))
    const half = Math.round(window.innerHeight * HALF)
    return half - peek < 96 || full - half < 96 ? [peek, full] : [peek, half, full]
  }, [])
  const { index, snapTo, handlers } = useSheetDrag(sheet, body, {
    detents,
    initial: 0,
    reducedMotion,
    onSettle: (_i, visible) => onRest(Math.min(visible, Math.round(window.innerHeight * HALF))),
    onMove: (visible) => {
      if (body.current) body.current.style.opacity = String(Math.min(1, Math.max(0, (visible - detents()[0]) / 72)))
    },
  })
  const open = index > 0
  const top = index === detents().length - 1 && open
  const first = useRef(true)
  useEffect(() => {
    if (first.current) return void (first.current = false)
    snapTo(1)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snap])
  return (
    <section
      ref={sheet}
      aria-label="Map panels"
      {...handlers}
      className="ws-solid glass fixed inset-x-0 bottom-0 z-30 flex h-[88dvh] flex-col rounded-t-[20px] border-x-0 border-b-0 will-change-transform"
      style={{ transform: `translateY(calc(100% - ${SHEET_PEEK}px - env(safe-area-inset-bottom)))` }}
    >
      <div className="shrink-0 cursor-grab touch-none px-3 select-none [[data-dragging]_&]:cursor-grabbing" style={{ height: SHEET_PEEK }}>
        <div className="flex h-4 items-end justify-center" aria-hidden>
          <span className="h-1 w-9 rounded-full bg-line2 transition-[background-color,width] duration-200 ease-[var(--ease-out)] [[data-dragging]_&]:w-11 [[data-dragging]_&]:bg-ink3" />
        </div>
        <div className="flex h-[56px] items-center gap-2">
          <button type="button" onClick={() => snapTo(open ? 0 : 1)} aria-expanded={open} aria-label={open ? 'Collapse the panel' : 'Expand the panel'} className="pressable flex h-11 min-w-0 flex-1 items-center gap-2 rounded-lg pl-1 text-left">
            <ChevronUp className={cn('size-4 shrink-0 text-ink3 transition-transform duration-200 ease-[var(--ease-out)]', open && 'rotate-180')} />
            <span className="min-w-0">
              <span className="block truncate text-[14.5px] leading-tight font-semibold tracking-[-0.015em] text-ink">{title}</span>
              <span className="t-label mt-1 flex items-center gap-1.5">
                <span className={cn('size-1.5 rounded-full', dirty ? 'bg-warn' : 'bg-ink3')} />
                {dirty ? 'edited' : 'layers, style and analysis'}
              </span>
            </span>
          </button>
          <button type="button" onClick={onAdd} aria-label="Add layer" className="pressable grid size-11 shrink-0 place-items-center rounded-full border border-line2 bg-bg2/60 text-ink">
            <Plus className="size-4" />
          </button>
        </div>
      </div>
      <div ref={inset} className="pointer-events-none absolute h-[env(safe-area-inset-bottom)] w-px" aria-hidden />
      <div ref={body} className={cn('scroll-thin flex min-h-0 flex-1 flex-col overscroll-contain border-t border-glass-line pb-[env(safe-area-inset-bottom)]', top ? 'touch-pan-y overflow-y-auto' : 'touch-none overflow-hidden')} inert={!open}>
        {children}
      </div>
    </section>
  )
}
