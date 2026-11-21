// Horizontal bar rows laid out with CSS grid so they stay legible from 375px up.
import { scaleLinear } from 'd3-scale'
import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { ChartTip, TipRow, TipTitle, useMeasure, useTip } from './chartKit'

export type AccessRow = { id: string; label: string; minutes: number; national: number; within30: number }

/** Travel time bars against a national reference tick, with "% within 30 min" as a bar-in-cell. */
export function AccessBars({ rows, bar, cell }: { rows: AccessRow[]; bar: string; cell: string }) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const { tip, show, hide } = useTip()
  const sx = scaleLinear([0, Math.max(...rows.flatMap((r) => [r.minutes, r.national]))], [0, 100]).nice(4)
  const ticks = sx.ticks(4)
  const grid = 'grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2 sm:grid-cols-[minmax(120px,0.9fr)_minmax(0,2fr)_52px_minmax(110px,0.9fr)]'
  return (
    <div ref={ref} className="relative" data-chart onPointerLeave={hide}>
      <div className={cn(grid, 'hidden pb-2 text-[11px] text-ink3 sm:grid')}>
        <span>Destination</span>
        <span>Mean travel time, minutes</span>
        <span />
        <span>Residents within 30 min</span>
      </div>
      {rows.map((r) => (
        <div
          key={r.id}
          className={cn(grid, 'border-t border-line py-3 sm:py-2.5')}
          onPointerMove={(e) =>
            show(
              e,
              <>
                <TipTitle>{r.label}</TipTitle>
                <TipRow color={bar} label="This area" value={`${r.minutes.toFixed(1)} min`} />
                <TipRow color="var(--ink2)" mark="line" label="National" value={`${r.national.toFixed(1)} min`} />
                <TipRow color={cell} label="Within 30 min" value={`${r.within30.toFixed(0)}%`} />
              </>,
            )
          }
        >
          <span className="text-[13px] text-ink">{r.label}</span>
          <span className="t-num text-right text-[13px] font-medium text-ink sm:order-3">
            {r.minutes.toFixed(1)}
            <span className="ml-1 font-normal text-ink3 sm:hidden">min</span>
          </span>
          <div className="relative col-span-2 h-3 sm:order-2 sm:col-span-1">
            {ticks.map((t) => (
              <span key={t} className="absolute -inset-y-3 w-px sm:-inset-y-2.5" style={{ left: `${sx(t)}%`, background: t === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)' }} />
            ))}
            <span className="absolute inset-y-0 left-0 rounded-r-[3px]" style={{ width: `${sx(r.minutes)}%`, background: bar }} />
            <span className="absolute -inset-y-[5px] w-0.5 -translate-x-1/2 rounded-full bg-ink2" style={{ left: `${sx(r.national)}%` }} />
          </div>
          <div className="relative col-span-2 h-[22px] overflow-hidden rounded-[5px] bg-bg3 sm:order-4 sm:col-span-1">
            <span className="absolute inset-y-0 left-0 rounded-r-[3px]" style={{ width: `${r.within30}%`, background: cell }} />
            <span className="t-num absolute inset-y-0 left-2 flex items-center text-[12px] font-medium text-ink">
              {r.within30.toFixed(0)}%<span className="ml-1.5 font-normal text-ink2 sm:hidden">within 30 min</span>
            </span>
          </div>
        </div>
      ))}
      <div className={cn(grid, 'border-t border-line pt-1.5')}>
        <span className="hidden sm:block" />
        <div className="relative col-span-2 h-4 sm:col-span-1">
          {ticks.map((t, i) => (
            <span key={t} className={cn('t-num absolute text-[10.5px] whitespace-nowrap text-ink3', i === 0 ? '' : i === ticks.length - 1 && sx(t) > 90 ? '-translate-x-full' : '-translate-x-1/2')} style={{ left: `${sx(t)}%` }}>
              {t}
              {i === ticks.length - 1 && <span className="sm:hidden"> min</span>}
            </span>
          ))}
        </div>
      </div>
      <ChartTip tip={tip} width={w} />
    </div>
  )
}

export type ShareRow = { id: string; label: string; share: number; value: number; color: string }

/** Share bars: one row per category, colour by identity in a fixed order. */
export function ShareBars({ rows, valueFmt, valueLabel, tipExtra }: { rows: ShareRow[]; valueFmt: (v: number) => string; valueLabel: string; tipExtra?: ReactNode }) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const { tip, show, hide } = useTip()
  const max = Math.max(...rows.map((r) => r.share))
  const sx = scaleLinear([0, max], [0, 100]).nice(4)
  const ticks = sx.ticks(4)
  const grid = 'grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-4 gap-y-2 sm:grid-cols-[110px_minmax(0,1fr)_52px_92px]'
  return (
    <div ref={ref} className="relative" data-chart onPointerLeave={hide}>
      <div className={cn(grid, 'hidden pb-2 text-[11px] text-ink3 sm:grid')}>
        <span>Party</span>
        <span>Share of valid votes, %</span>
        <span className="text-right">Share</span>
        <span className="text-right">{valueLabel}</span>
      </div>
      {rows.map((r) => (
        <div
          key={r.id}
          className={cn(grid, 'border-t border-line py-3 sm:py-2.5')}
          onPointerMove={(e) =>
            show(
              e,
              <>
                <TipTitle>{r.label}</TipTitle>
                <TipRow color={r.color} label="Share" value={`${r.share.toFixed(1)}%`} />
                <TipRow mark="none" label={valueLabel} value={valueFmt(r.value)} />
                {tipExtra}
              </>,
            )
          }
        >
          <span className="flex items-center gap-2 text-[13px] text-ink">
            <span className="size-2.5 shrink-0 rounded-[2px]" style={{ background: r.color }} />
            {r.label}
          </span>
          <span className="t-num text-right text-[13px] font-medium text-ink sm:order-3">{r.share.toFixed(1)}%</span>
          <span className="t-num text-right text-[13px] text-ink2 sm:order-4">{valueFmt(r.value)}</span>
          <div className="relative col-span-3 h-3 sm:order-2 sm:col-span-1">
            {ticks.map((t) => (
              <span key={t} className="absolute -inset-y-3 w-px sm:-inset-y-2.5" style={{ left: `${sx(t)}%`, background: t === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)' }} />
            ))}
            <span className="absolute inset-y-0 left-0 rounded-r-[3px]" style={{ width: `${sx(r.share)}%`, background: r.color }} />
          </div>
        </div>
      ))}
      <div className={cn(grid, 'border-t border-line pt-1.5')}>
        <span className="hidden sm:block" />
        <div className="relative col-span-3 h-4 sm:col-span-1">
          {ticks.map((t, i) => (
            <span key={t} className={cn('t-num absolute text-[10.5px] whitespace-nowrap text-ink3', i === 0 ? '' : i === ticks.length - 1 && sx(t) > 90 ? '-translate-x-full' : '-translate-x-1/2')} style={{ left: `${sx(t)}%` }}>
              {t}
            </span>
          ))}
        </div>
      </div>
      <ChartTip tip={tip} width={w} />
    </div>
  )
}
