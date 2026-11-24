// Histogram of one value per unit, with the median (and optionally the national figure) marked.
import { bin, max as dmax } from 'd3-array'
import { scaleLinear } from 'd3-scale'
import { useState } from 'react'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, useMeasure, vbar } from './chartKit'

export function Distribution({
  values,
  median,
  reference,
  fmt,
  color,
  unit,
  noun = 'units',
  height = 188,
  ariaLabel,
}: {
  values: number[]
  median: number
  reference?: { value: number; label: string } | null
  fmt: (v: number) => string
  color: string
  unit: string
  /** what a bar counts ("districts") */
  noun?: string
  height?: number
  ariaLabel: string
}) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const [hi, setHi] = useState<number | null>(null)
  const m = { l: 30, r: 26, t: 30, b: 24 }
  const lo = Math.min(...values)
  const top = Math.max(...values)
  const k = Math.max(5, Math.min(20, Math.ceil(Math.log2(values.length) + 1)))
  const sx = scaleLinear([lo, top === lo ? lo + 1 : top], [m.l, w - m.r]).nice(k * 2)
  const bins = bin().domain(sx.domain() as [number, number]).thresholds(sx.ticks(k * 2))(values)
  const ymax = dmax(bins, (b) => b.length) ?? 1
  const sy = scaleLinear([0, ymax], [height - m.b, m.t]).nice(3)
  const xt = sx.ticks(Math.max(2, Math.floor((w - m.l - m.r) / 84)))
  const b = hi === null ? null : bins[hi]
  const flag = (x: number, text: string, row: number) => {
    const px = sx(x)
    const anchor = px < m.l + 40 ? 'start' : px > w - 60 ? 'end' : 'middle'
    return (
      <g>
        <line x1={px} x2={px} y1={m.t - 6 - row * 0} y2={height - m.b} stroke="var(--ink2)" strokeWidth={1.25} strokeDasharray={row ? '3 3' : undefined} />
        <text x={px} y={m.t - 12 - row * 12} textAnchor={anchor} {...AXIS_TEXT} fill="var(--ink)" fontWeight={600}>
          {text}
        </text>
      </g>
    )
  }
  return (
    <div ref={ref} className="relative" data-chart>
      <svg viewBox={`0 0 ${w} ${height + 12}`} width="100%" height={height + 12} role="img" aria-label={ariaLabel} className="block touch-pan-y" onPointerLeave={() => setHi(null)}>
        <g transform="translate(0 12)">
          {sy.ticks(3).map((t) => (
            <g key={t}>
              <line x1={m.l} x2={w - m.r} y1={sy(t)} y2={sy(t)} stroke={t === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)'} />
              <text x={m.l - 6} y={sy(t) + 3.5} textAnchor="end" {...AXIS_TEXT}>
                {t}
              </text>
            </g>
          ))}
          {bins.map((d, i) => {
            const x0 = sx(d.x0 as number)
            const x1 = sx(d.x1 as number)
            const bw = Math.max(1, x1 - x0 - 1.5)
            return (
              <g key={i} onPointerMove={() => setHi(i)} onPointerEnter={() => setHi(i)}>
                <rect x={x0} y={m.t} width={Math.max(1, x1 - x0)} height={height - m.b - m.t} fill="transparent" />
                {d.length > 0 && <path d={vbar(x0 + 0.75, height - m.b, sy(d.length), bw, 2.5)} fill={color} opacity={hi === null || hi === i ? 1 : 0.55} />}
              </g>
            )
          })}
          {xt.map((t) => (
            <text key={t} x={sx(t)} y={height - 6} textAnchor="middle" {...AXIS_TEXT}>
              {fmt(t)}
            </text>
          ))}
          {flag(median, `median ${fmt(median)}`, 0)}
          {reference && Math.abs(sx(reference.value) - sx(median)) > 4 && reference.value >= sx.domain()[0] && reference.value <= sx.domain()[1] && flag(reference.value, `${reference.label} ${fmt(reference.value)}`, 1)}
        </g>
      </svg>
      <ChartTip
        width={w}
        tip={
          b
            ? {
                x: (sx(b.x0 as number) + sx(b.x1 as number)) / 2,
                y: sy(b.length) + 12,
                body: (
                  <>
                    <TipTitle>
                      {fmt(b.x0 as number)} to {fmt(b.x1 as number)}
                    </TipTitle>
                    <TipRow mark="none" label={noun} value={b.length} />
                  </>
                ),
              }
            : null
        }
      />
      <p className="mt-1 text-[11px] text-ink3">Number of {noun} (bar height) by value, in {unit}.</p>
    </div>
  )
}
