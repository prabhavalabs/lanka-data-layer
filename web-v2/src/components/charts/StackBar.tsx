import { useState } from 'react'
import { cn } from '@/lib/utils'
import { ChartTip, TipRow, useMeasure, useTip } from './chartKit'

export type StackSegment = { id: string; label: string; share: number; color: string }

/** 100% stacked bar. Fixed category order, 2px surface gap, direct labels only where they fit. */
export function StackBar({ segments, labelMin = 8, ariaLabel }: { segments: StackSegment[]; labelMin?: number; ariaLabel: string }) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const { tip, show, hide } = useTip()
  const [hi, setHi] = useState<string | null>(null)
  const total = segments.reduce((s, x) => s + x.share, 0) || 1
  return (
    <div ref={ref} className="relative" data-chart role="img" aria-label={ariaLabel}>
      {/* direct labels sit above the bar in ink, never inside in the series colour */}
      <div className="flex gap-[2px]" aria-hidden>
        {segments.map((s) => {
          const pct = (s.share / total) * 100
          const px = (pct / 100) * w
          return (
            <div key={s.id} style={{ flex: `${pct} 1 0%` }} className="min-w-0 pb-1.5">
              {pct >= labelMin && px > 44 && (
                <div className="border-l border-line2 pl-1.5 leading-tight">
                  <div className="t-num text-[13px] font-semibold text-ink">{pct.toFixed(0)}%</div>
                  <div className="truncate text-[11px] text-ink2">{s.label}</div>
                </div>
              )}
            </div>
          )
        })}
      </div>
      <div className="flex h-[18px] gap-[2px]" onPointerLeave={() => (hide(), setHi(null))}>
        {segments.map((s, i) => (
          <div
            key={s.id}
            style={{ flex: `${(s.share / total) * 100} 1 0%`, background: s.color }}
            className={cn('relative min-w-[3px] transition-opacity duration-150 before:absolute before:-inset-y-3 before:inset-x-0', i === 0 && 'rounded-l-[4px]', i === segments.length - 1 && 'rounded-r-[4px]', hi && hi !== s.id && 'opacity-45')}
            onPointerMove={(e) => {
              setHi(s.id)
              show(e, <TipRow color={s.color} label={s.label} value={`${((s.share / total) * 100).toFixed(1)}%`} />)
            }}
          />
        ))}
      </div>
      <div className="mt-1.5 flex justify-between font-sans text-[10.5px] text-ink3 tabular-nums" aria-hidden>
        <span>0</span>
        <span>50</span>
        <span>100% of land area</span>
      </div>
      <ChartTip tip={tip} width={w} />
    </div>
  )
}
