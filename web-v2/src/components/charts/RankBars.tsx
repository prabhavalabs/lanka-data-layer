// Ranked horizontal bars for many units: sorted, the top and bottom labelled, a reference line, missing values shown as missing.
import { scaleLinear } from 'd3-scale'
import { ArrowUpRight, ChevronsUpDown } from 'lucide-react'
import { Link } from 'react-router'
import { cn } from '@/lib/utils'

export type RankRow = { id: string; label: string; value: number | null; href?: string }

const GRID = 'grid grid-cols-[minmax(84px,138px)_minmax(0,1fr)] items-center gap-x-3 sm:grid-cols-[minmax(110px,170px)_minmax(0,1fr)]'
const BAR = 'grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-2.5'

/**
 * Rows are given best-first, missing values last. Collapsed it shows `head` from the top and `tail` from the
 * bottom with a "show all" row between them. Bars start at zero; a missing value is an empty dashed track.
 */
export function RankBars({
  rows,
  short,
  color,
  marks,
  reference,
  onToggle,
  expanded,
  onExpand,
  head = 10,
  tail = 5,
  maxRows = 400,
  unit,
}: {
  rows: RankRow[]
  short: (v: number) => string
  color: string
  /** colour of the rows picked for comparison */
  marks?: Record<string, string>
  /** a value every bar is read against, drawn as a line (the national figure) */
  reference?: { value: number; label: string } | null
  /** clicking a bar picks a unit for comparison; leave out when nothing is compared */
  onToggle?: (id: string) => void
  expanded: boolean
  onExpand: () => void
  head?: number
  tail?: number
  maxRows?: number
  unit: string
}) {
  const have = rows.filter((r) => r.value !== null)
  const missing = rows.filter((r) => r.value === null)
  const vals = have.map((r) => r.value as number)
  const lo = Math.min(0, ...vals)
  const hi = Math.max(0, ...vals)
  const sx = scaleLinear([lo, hi || 1], [0, 100])
  const base = sx(0)
  const ticks = sx.ticks(4)
  const collapsible = have.length > head + tail + 2
  const showAll = expanded || !collapsible
  const capped = rows.length > maxRows
  const shown = showAll ? [...have, ...missing].slice(0, maxRows) : [...have.slice(0, head), ...have.slice(-tail)]
  const gapAt = showAll ? -1 : head
  const hidden = have.length - head - tail

  const bar = (r: RankRow, rank: number | null) => {
    const v = r.value
    const mark = marks?.[r.id]
    const inner = (
      <>
        <span className="relative block h-3.5 max-md:h-4">
          {ticks.map((t) => (
            <span key={t} className="absolute -inset-y-3 w-px" style={{ left: `${sx(t)}%`, background: t === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)' }} />
          ))}
          {v === null ? (
            <span className="absolute inset-y-[5px] right-0 left-0 rounded-sm border border-dashed border-line2" />
          ) : (
            <span className="absolute inset-y-0 rounded-[3px]" style={{ left: `${Math.min(base, sx(v))}%`, width: `${Math.max(0.6, Math.abs(sx(v) - base))}%`, background: mark ?? color, opacity: marks && Object.keys(marks).length && !mark ? 0.55 : 1 }} />
          )}
          {reference && v !== null && <span className="absolute -inset-y-[5px] w-0.5 -translate-x-1/2 rounded-full bg-ink2" style={{ left: `${Math.min(100, Math.max(0, sx(reference.value)))}%` }} />}
        </span>
        <span className={cn('t-num min-w-[3.4rem] text-right text-[12.5px]', v === null ? 'text-ink3 italic' : rank !== null && rank < 1 ? 'font-semibold text-ink' : 'text-ink')}>{v === null ? 'no data' : short(v)}</span>
      </>
    )
    return onToggle ? (
      <button type="button" aria-pressed={!!mark} aria-label={`Compare ${r.label} over time`} onClick={() => onToggle(r.id)} className={cn(BAR, 'pressable min-h-8 w-full cursor-pointer rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring max-md:min-h-10', mark && 'bg-bg3/70')}>
        {inner}
      </button>
    ) : (
      <div className={cn(BAR, 'min-h-8 max-md:min-h-10')}>{inner}</div>
    )
  }

  return (
    <div role="list" aria-label={`${rows.length} units, ranked by value, in ${unit}`}>
      <div className={cn(GRID, 'pb-1.5 text-[11px] text-ink3')} aria-hidden>
        <span>Unit</span>
        <span className="relative block h-4">
          {ticks.map((t, i) => (
            <span key={t} className={cn('t-num absolute whitespace-nowrap', i === 0 ? '' : i === ticks.length - 1 && sx(t) > 88 ? '-translate-x-full' : '-translate-x-1/2')} style={{ left: `${sx(t)}%` }}>
              {short(t)}
            </span>
          ))}
        </span>
      </div>
      {reference && (
        <div className={cn(GRID, 'pb-1 text-[11px] text-ink2')} aria-hidden>
          <span className="inline-flex items-center gap-1.5">
            <span className="h-3 w-0.5 rounded-full bg-ink2" />
            {reference.label}
          </span>
          <span className="t-num">{short(reference.value)}</span>
        </div>
      )}
      {shown.map((r, i) => {
        const rank = r.value === null ? null : have.indexOf(r)
        return (
          <div key={r.id}>
            {i === gapAt && (
              <div className="border-t border-dashed border-line py-1">
                <button type="button" onClick={onExpand} className="pressable inline-flex min-h-8 items-center gap-2 rounded-md px-2 text-[12.5px] text-ink2 hover:bg-bg3 hover:text-ink max-md:min-h-10">
                  <ChevronsUpDown className="size-3.5" />
                  Show {hidden.toLocaleString('en')} more {hidden === 1 ? 'unit' : 'units'}
                  {missing.length > 0 && <span className="text-ink3">(and {missing.length.toLocaleString('en')} with no data)</span>}
                </button>
              </div>
            )}
            <div role="listitem" className={cn(GRID, 'border-t border-line')}>
              <span className="flex min-w-0 items-center gap-1.5 py-0.5">
                <span className="t-num w-7 shrink-0 text-right font-mono text-[10.5px] text-ink3">{rank === null ? '' : rank + 1}</span>
                {r.href ? (
                  <Link to={r.href} aria-label={`Open the ${r.label} area page`} className="group inline-flex min-h-8 min-w-0 items-center gap-0.5 text-[13px] text-ink hover:text-brand-ink max-md:min-h-10">
                    <span className="truncate">{r.label}</span>
                    <ArrowUpRight className="size-3 shrink-0 text-ink3 opacity-0 transition-opacity group-hover:opacity-100 max-md:opacity-60" />
                  </Link>
                ) : (
                  <span className="truncate text-[13px] text-ink">{r.label}</span>
                )}
              </span>
              {bar(r, rank)}
            </div>
          </div>
        )
      })}
      {!showAll && (
        <div className="border-t border-line pt-2 text-[12px] text-ink3">
          {missing.length > 0 && `${missing.length.toLocaleString('en')} ${missing.length === 1 ? 'unit has' : 'units have'} no data and ${missing.length === 1 ? 'is' : 'are'} not drawn. `}Showing the top {head} and bottom {tail}.
        </div>
      )}
      {capped && showAll && <div className="border-t border-line pt-2 text-[12px] text-ink3">Showing the first {maxRows.toLocaleString('en')} of {rows.length.toLocaleString('en')} units. Use the map or the download for the rest.</div>}
    </div>
  )
}
