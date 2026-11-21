import { scaleLinear } from 'd3-scale'
import { useState } from 'react'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, useMeasure } from './chartKit'

export type StripPoint = { pcode: string; name: string; value: number }

/**
 * Strip plot: every sibling on one shared scale, the national value as a labelled
 * reference tick, and this unit picked out in the selection colour.
 */
export function RankStrip({
  points,
  current,
  value,
  national,
  fmt,
  label,
  onPick,
}: {
  points: StripPoint[]
  /** p-code of the unit this page is about (may be absent from `points` for the country) */
  current: string
  value: number
  national: number
  fmt: (v: number) => string
  label: string
  onPick?: (pcode: string) => void
}) {
  const [ref, w] = useMeasure<HTMLDivElement>(300)
  const [hi, setHi] = useState<number | null>(null)
  const height = 66
  const cy = 34
  const all = [...points.map((p) => p.value), national, value]
  const lo = Math.min(...all)
  const hiV = Math.max(...all)
  const sx = scaleLinear([lo, hiV], [10, w - 10])
  const self = points.some((p) => p.pcode === current)
  const ux = sx(value)
  const nx = sx(national)
  const h = hi === null ? null : points[hi]
  const anchor = (x: number) => (x < 34 ? 'start' : x > w - 34 ? 'end' : 'middle')
  const edge = (x: number) => (x < 34 ? 2 : x > w - 34 ? w - 2 : x)

  return (
    <div ref={ref} className="relative" data-chart>
      <svg
        viewBox={`0 0 ${w} ${height}`}
        width="100%"
        height={height}
        role="img"
        aria-label={`${label}: this unit ${fmt(value)}, national ${fmt(national)}, range ${fmt(lo)} to ${fmt(hiV)} across ${points.length} units`}
        className={onPick ? 'block cursor-pointer touch-pan-y' : 'block touch-pan-y'}
        onPointerMove={(e) => {
          const b = e.currentTarget.getBoundingClientRect()
          const px = ((e.clientX - b.left) / b.width) * w
          let best = 0
          points.forEach((p, i) => {
            if (Math.abs(sx(p.value) - px) < Math.abs(sx(points[best].value) - px)) best = i
          })
          setHi(Math.abs(sx(points[best].value) - px) < 22 ? best : null)
        }}
        onPointerLeave={() => setHi(null)}
        onClick={() => h && h.pcode !== current && onPick?.(h.pcode)}
      >
        <line x1={4} x2={w - 4} y1={cy} y2={cy} stroke="var(--chart-grid)" strokeWidth={1} />
        <line x1={10} x2={10} y1={cy - 3} y2={cy + 3} stroke="var(--chart-axis)" />
        <line x1={w - 10} x2={w - 10} y1={cy - 3} y2={cy + 3} stroke="var(--chart-axis)" />
        {/* national reference */}
        {self && (
          <>
            <line x1={nx} x2={nx} y1={cy - 11} y2={cy + 11} stroke="var(--ink2)" strokeWidth={1.25} />
            <text x={edge(nx)} y={12} textAnchor={anchor(nx)} {...AXIS_TEXT} fill="var(--ink2)">
              national {fmt(national)}
            </text>
          </>
        )}
        {points.map((p, i) =>
          p.pcode === current ? null : (
            <circle key={p.pcode} cx={sx(p.value)} cy={cy} r={hi === i ? 5 : 3.5} fill={hi === i ? 'var(--ink)' : 'var(--ink3)'} fillOpacity={hi === i ? 1 : 0.5} stroke="var(--bg2)" strokeWidth={1} />
          ),
        )}
        {/* this unit — selection colour, never a data colour */}
        <circle cx={ux} cy={cy} r={6} fill="var(--brand)" stroke="var(--bg2)" strokeWidth={2} />
        <text x={edge(ux)} y={height - 6} textAnchor={anchor(ux)} fontSize={11} fontWeight={600} fill="var(--ink)" style={{ fontVariantNumeric: 'tabular-nums' }}>
          {fmt(value)}
        </text>
        {ux > 70 && (
          <text x={2} y={height - 6} {...AXIS_TEXT}>
            {fmt(lo)}
          </text>
        )}
        {ux < w - 70 && (
          <text x={w - 2} y={height - 6} textAnchor="end" {...AXIS_TEXT}>
            {fmt(hiV)}
          </text>
        )}
      </svg>
      <ChartTip
        width={w}
        tip={
          h
            ? {
                x: sx(h.value),
                y: cy - 6,
                body: (
                  <>
                    <TipTitle>
                      {h.name} <span className="ml-1 font-mono text-[10.5px] font-normal text-ink3">{h.pcode}</span>
                    </TipTitle>
                    <TipRow mark="none" label={label} value={fmt(h.value)} />
                    {onPick && h.pcode !== current && <div className="mt-1 text-[11px] text-ink3">Click to open</div>}
                  </>
                ),
              }
            : null
        }
      />
    </div>
  )
}
