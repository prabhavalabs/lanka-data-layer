import { useQuery } from '@tanstack/react-query'
import { ArrowDownRight, ArrowRight, ArrowUpRight, Equal, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState, type AnimationEvent, type ReactNode } from 'react'
import { Link } from 'react-router'
import { areaProfileQuery, indicatorSeriesQuery } from '@/api/queries'
import type { Area } from '@/api/types'
import { PcodePlate } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { fmtValue } from '@/lib/format'
import { useReducedMotion, type Theme } from '@/lib/hooks'
import { fmtBare, isSum, LEVEL_NAME, ordinal, SELECT_HEX, unitSuffix, type MapModel } from './model'
import { sharesScale, TimeSeries } from './TimeSeries'
import { useSheetDrag } from './useSheetDrag'

/** The source names carry an administrative suffix ("… District") that the English name already drops. */
const siName = (s: string) => s.replace(/\s+(දිස්ත්|පළාත)\S*$/u, '')

const taName = (s: string) => s.replace(/\s+(மாவட|மாகாண)\S*$/u, '')

/** Every sibling as a dot along the value axis; the selected one carries the selection accent. */
function RankStrip({ model, pcode, theme }: { model: MapModel; pcode: string; theme: Theme }) {
  const { min, max, data } = model
  const unit = model.indicator.unit
  const at = (v: number) => `${((v - min) / (max - min || 1)) * 100}%`
  const self = model.byPcode.get(pcode)
  const showNational = !isSum(unit) && data.national >= min && data.national <= max
  return (
    <div>
      <div className="relative mx-1 h-5">
        <span className="absolute inset-x-0 top-1/2 h-px bg-line2" />
        {showNational && <span className="absolute top-[3px] h-[14px] w-px bg-ink3" style={{ left: at(data.national) }} title="National" />}
        {data.pcodes.map((p, i) =>
          p === pcode ? null : <span key={p} className="absolute top-1/2 size-[5px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink3/80" style={{ left: at(data.values[i]) }} />,
        )}
        {self && (
          <span
            className="absolute top-1/2 size-[11px] -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-bg2"
            style={{ left: at(self.value), background: SELECT_HEX[theme], boxShadow: `0 0 0 1px ${SELECT_HEX[theme]}` }}
          />
        )}
      </div>
      <div className="t-num mt-0.5 flex justify-between font-mono text-[10px] text-ink3">
        <span>{fmtBare(min, unit)}</span>
        <span>{fmtBare(max, unit)}</span>
      </div>
    </div>
  )
}

function Body({ pcode, model, areas, theme, year, onYear }: { pcode: string; model: MapModel | null; areas: Map<string, Area> | undefined; theme: Theme; year: number; onYear: (y: number) => void }) {
  const profile = useQuery(areaProfileQuery(pcode))
  const indicator = model?.indicator
  const series = useQuery(indicatorSeriesQuery(indicator?.id ?? '', indicator ? [pcode, 'LK'] : []))
  const area = areas?.get(pcode) ?? profile.data?.area
  const parent = area?.parent ? areas?.get(area.parent) : undefined
  const self = model?.byPcode.get(pcode)
  const unit = indicator?.unit ?? ''
  const national = model?.data.national ?? 0
  const delta = self && national ? (unit === '%' ? self.value - national : ((self.value - national) / national) * 100) : 0
  const level = Math.abs(delta) < 0.05
  const areaPoints = series.data?.find((s) => s.pcode === pcode)?.points
  const nationalPoints = series.data?.find((s) => s.pcode === 'LK')?.points
  const others = profile.data?.headline.filter((h) => h.id !== indicator?.id).slice(0, 4)

  return (
    <>
      <header className="px-5 pt-4 pb-4">
        <div className="t-label flex h-8 items-center tracking-[0.08em] uppercase">
          {area ? (
            <>
              {LEVEL_NAME[area.level as 1 | 2] ?? 'Area'}
              {parent && parent.pcode !== 'LK' && <span className="text-ink3/80">&nbsp;·&nbsp;{parent.name.en} Province</span>}
            </>
          ) : (
            <span className="skeleton h-3 w-32" />
          )}
        </div>
        {area ? (
          <>
            <h2 className="mt-0.5 text-[28px] leading-[1.1] font-semibold tracking-[-0.035em] text-ink">{area.name.en}</h2>
            <p className="mt-1.5 flex flex-wrap items-baseline gap-x-2.5 text-[14.5px] text-ink2">
              <span lang="si">{siName(area.name.si)}</span>
              <span className="text-ink3" aria-hidden>
                /
              </span>
              <span lang="ta">{taName(area.name.ta)}</span>
            </p>
            <div className="mt-3">
              <PcodePlate code={area.pcode} copyable />
            </div>
          </>
        ) : (
          <>
            <div className="skeleton mt-1 h-8 w-44" />
            <div className="skeleton mt-3 h-4 w-52" />
            <div className="skeleton mt-3 h-7 w-20" />
          </>
        )}
      </header>

      <section className="border-t border-glass-line px-5 py-4">
        <div className="t-label flex items-center justify-between tracking-[0.08em] uppercase">
          <span className="truncate">{indicator?.label ?? 'Indicator'}</span>
          <span className="t-num shrink-0">{model?.data.year}</span>
        </div>
        {self && indicator ? (
          <>
            <div className="mt-2 flex items-baseline gap-1.5">
              <span className="t-num font-mono text-[38px] leading-none font-medium tracking-[-0.045em] text-ink">{fmtBare(self.value, unit)}</span>
              <span className="text-[13px] text-ink2">{unitSuffix(unit)}</span>
            </div>
            {!isSum(unit) && (
              <p className="mt-2.5 flex items-center gap-1.5 text-[12.5px] text-ink2">
                {level ? <Equal className="size-3.5 text-ink" /> : delta > 0 ? <ArrowUpRight className="size-3.5 text-ink" /> : <ArrowDownRight className="size-3.5 text-ink" />}
                <span className="t-num font-medium text-ink">
                  {level ? '' : delta > 0 ? '+' : '−'}
                  {Math.abs(delta).toFixed(Math.abs(delta) >= 100 ? 0 : 1)}
                  {unit === '%' ? ' pp' : '%'}
                </span>
                <span>
                  vs national <span className="t-num text-ink">{fmtValue(national, unit)}</span>
                </span>
              </p>
            )}
            <div className="mt-4 flex items-baseline justify-between text-[12.5px] text-ink2">
              <span>
                <span className="t-num text-[15px] font-semibold text-ink">{ordinal(self.rank)}</span> of {model.data.pcodes.length}
              </span>
              <span className="t-label inline-flex items-center gap-1.5">
                {!isSum(unit) && (
                  <>
                    <span className="h-3 w-px bg-ink3" />
                    national
                  </>
                )}
              </span>
            </div>
            <div className="mt-1.5">
              <RankStrip model={model} pcode={pcode} theme={theme} />
            </div>
          </>
        ) : (
          <>
            <div className="skeleton mt-2 h-10 w-36" />
            <div className="skeleton mt-3 h-3.5 w-48" />
            <div className="skeleton mt-5 h-5 w-full" />
          </>
        )}
      </section>

      <section className="border-t border-glass-line px-5 py-4">
        <div className="t-label mb-2 flex items-center justify-between tracking-[0.08em] uppercase">
          <span>Over time</span>
          {sharesScale(areaPoints, nationalPoints) && (
            <span className="inline-flex items-center gap-1.5 tracking-normal normal-case">
              <span className="w-4 border-t border-dashed border-[var(--chart-axis)]" />
              national
            </span>
          )}
        </div>
        {areaPoints && indicator && model ? (
          <TimeSeries points={areaPoints} reference={nationalPoints} year={year} unit={unit} color={model.colors[theme === 'dark' ? 4 : 5]} label={`${indicator.label} in ${area?.name.en ?? pcode}`} onYear={onYear} />
        ) : (
          <div className="skeleton h-[132px] w-full" />
        )}
      </section>

      <section className="border-t border-glass-line px-5 py-4">
        <div className="t-label mb-3 flex items-center justify-between">
          <span className="tracking-[0.08em] uppercase">At a glance</span>
          <span>p = provisional</span>
        </div>
        <dl className="grid grid-cols-2 gap-x-6 gap-y-4">
          {others
            ? others.map((h) => (
                <div key={h.id} className="min-w-0">
                  <dt className="flex items-baseline justify-between gap-2 text-[12px] text-ink2">
                    <span className="truncate">{h.label}</span>
                    <span className="t-label t-num shrink-0" title={h.status === 'provisional' ? 'Provisional figure' : undefined}>
                      {h.year}
                      {h.status === 'provisional' && ' p'}
                    </span>
                  </dt>
                  <dd className="mt-1 flex items-baseline gap-1">
                    <span className="t-num font-mono text-[17px] font-medium tracking-[-0.03em] text-ink">{fmtBare(h.value, h.unit)}</span>
                    {h.unit !== 'people' && h.unit !== 'households' && <span className="text-[11.5px] text-ink3">{unitSuffix(h.unit)}</span>}
                  </dd>
                </div>
              ))
            : Array.from({ length: 4 }, (_, i) => (
                <div key={i}>
                  <div className="skeleton h-3 w-20" />
                  <div className="skeleton mt-2 h-5 w-24" />
                </div>
              ))}
        </dl>
      </section>
    </>
  )
}

/**
 * Non-modal detail panel: slides in from the right on desktop, up from the bottom on
 * mobile. The map stays interactive. Esc closes it instantly (keyboard actions don't animate).
 */
export function AreaDrawer({
  pcode,
  model,
  areas,
  theme,
  year,
  mobile,
  onYear,
  onClose,
}: {
  pcode: string | null
  model: MapModel | null
  areas: Map<string, Area> | undefined
  theme: Theme
  year: number
  mobile: boolean
  onYear: (y: number) => void
  onClose: () => void
}) {
  // keep the last area mounted while the exit animation plays
  const [shown, setShown] = useState<string | null>(pcode)
  const [instant, setInstant] = useState(false)
  if (pcode && pcode !== shown) setShown(pcode)
  if (pcode && instant) setInstant(false)

  // gone at once, with no exit animation: for Esc, and for a sheet that has already been dragged off the screen
  const dismiss = useCallback(() => {
    setInstant(true)
    setShown(null)
    onClose()
  }, [onClose])
  useEffect(() => {
    if (!pcode) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      // leave Esc to any open menu, select or dialog
      if (document.querySelector('[role="dialog"], [role="listbox"], [data-radix-popper-content-wrapper]')) return
      dismiss()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [pcode, dismiss])

  if (!shown || (!pcode && instant)) return null
  const state = pcode ? 'open' : 'closed'
  const onAnimationEnd = (e: AnimationEvent<HTMLElement>) => {
    if (e.target === e.currentTarget && !pcode) setShown(null)
  }
  const close = (
    <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="Close area details" className="absolute top-3.5 right-3.5 z-10 max-md:size-11 max-md:top-2 max-md:right-2">
      <X />
    </Button>
  )
  const body = <Body key={shown} pcode={shown} model={model} areas={areas} theme={theme} year={year} onYear={onYear} />
  const footer = (
    <div className="border-t border-glass-line px-5 pt-3 pb-[max(12px,env(safe-area-inset-bottom))]">
      <Button asChild size="lg" className="w-full">
        <Link to={`/area/${shown}`}>
          Open profile
          <ArrowRight />
        </Link>
      </Button>
    </div>
  )
  if (mobile)
    return (
      <AreaSheet state={state} onAnimationEnd={onAnimationEnd} onDismiss={dismiss} close={close} footer={footer}>
        {body}
      </AreaSheet>
    )
  return (
    <aside data-state={state} onAnimationEnd={onAnimationEnd} aria-label="Area details" className="explore-drawer glass absolute top-[68px] right-4 bottom-4 z-40 flex w-[380px] flex-col overflow-hidden rounded-2xl">
      {close}
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">{body}</div>
      {footer}
    </aside>
  )
}

/**
 * The phone's version: a bottom sheet that can be thrown away. It is dragged by its handle, or by its
 * content once that is scrolled to the start; let go past half-way, or flick it, and it leaves.
 */
function AreaSheet({ state, onAnimationEnd, onDismiss, close, footer, children }: { state: 'open' | 'closed'; onAnimationEnd: (e: AnimationEvent<HTMLElement>) => void; onDismiss: () => void; close: ReactNode; footer: ReactNode; children: ReactNode }) {
  const sheet = useRef<HTMLElement>(null)
  const scroller = useRef<HTMLDivElement>(null)
  const reducedMotion = useReducedMotion()
  const { handlers } = useSheetDrag(sheet, scroller, {
    detents: () => [0, sheet.current?.offsetHeight ?? 0],
    initial: 1,
    reducedMotion,
    onSettle: (i) => {
      if (i === 0) onDismiss()
    },
  })
  return (
    <aside
      ref={sheet}
      data-state={state}
      onAnimationEnd={onAnimationEnd}
      aria-label="Area details"
      {...handlers}
      className="explore-sheet explore-solid glass fixed inset-x-0 bottom-0 z-40 flex h-[62dvh] flex-col overflow-hidden rounded-t-[20px] border-x-0 border-b-0 will-change-transform"
    >
      <div className="absolute inset-x-0 top-0 z-10 flex h-6 cursor-grab touch-none items-start justify-center pt-2 [[data-dragging]_&]:cursor-grabbing" aria-hidden>
        <span className="h-1 w-9 rounded-full bg-line2 transition-[background-color,width] duration-200 ease-[var(--ease-out)] [[data-dragging]_&]:w-11 [[data-dragging]_&]:bg-ink3" />
      </div>
      {close}
      <div ref={scroller} className="scroll-thin min-h-0 flex-1 touch-pan-y overflow-y-auto overscroll-contain">
        {children}
      </div>
      {footer}
    </aside>
  )
}
