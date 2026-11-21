import { extent } from 'd3-array'
import { scaleLinear } from 'd3-scale'
import { line as d3line } from 'd3-shape'
import { useState } from 'react'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, useMeasure, vbar } from './chartKit'

export type ClimateMonth = { month: string; rain: number; lo: number; hi: number; temp: number }

/**
 * Monthly rainfall bars over the normal band, with temperature as its own small
 * chart underneath on the same month grid. Two charts, two axes — never dual.
 */
export function ClimateBars({ data, rain, temp, band = 'var(--ink3)' }: { data: ClimateMonth[]; rain: string; temp: string; band?: string }) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const [hi, setHi] = useState<number | null>(null)
  const m = { l: 40, r: 6 }
  const H1 = 196
  const H2 = 104
  const t1 = 22
  const b1 = H1 - 22
  const t2 = 22
  const b2 = H2 - 22
  const step = (w - m.l - m.r) / data.length
  const cx = (i: number) => m.l + step * (i + 0.5)
  const bw = Math.min(22, step * 0.46)
  const sy = scaleLinear([0, Math.max(...data.map((d) => Math.max(d.hi, d.rain)))], [b1, t1]).nice(4)
  const [tLo, tHi] = extent(data, (d) => d.temp) as [number, number]
  const st = scaleLinear([Math.floor(tLo - 0.5), Math.ceil(tHi + 0.5)], [b2, t2])
  const path = d3line<ClimateMonth>()
    .x((_, i) => cx(i))
    .y((d) => st(d.temp))
  const short = step < 30
  const h = hi === null ? null : data[hi]
  const months = (y: number) =>
    data.map((d, i) => (
      <text key={d.month} x={cx(i)} y={y} textAnchor="middle" {...AXIS_TEXT} fill={hi === i ? 'var(--ink)' : 'var(--ink3)'}>
        {short ? d.month[0] : d.month}
      </text>
    ))

  return (
    <div
      ref={ref}
      className="relative touch-pan-y"
      data-chart
      onPointerMove={(e) => {
        const b = e.currentTarget.getBoundingClientRect()
        const i = Math.floor((e.clientX - b.left - m.l) / step)
        setHi(i >= 0 && i < data.length ? i : null)
      }}
      onPointerLeave={() => setHi(null)}
    >
      <svg viewBox={`0 0 ${w} ${H1}`} width="100%" height={H1} role="img" aria-label="Monthly rainfall in millimetres against the 1991–2020 normal range" className="block">
        <text x={0} y={10} {...AXIS_TEXT}>
          mm
        </text>
        {sy.ticks(4).map((t) => (
          <g key={t}>
            <line x1={m.l} x2={w - m.r} y1={sy(t)} y2={sy(t)} stroke={t === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)'} />
            <text x={m.l - 8} y={sy(t) + 3.5} textAnchor="end" {...AXIS_TEXT}>
              {t}
            </text>
          </g>
        ))}
        {hi !== null && <rect x={m.l + step * hi} width={step} y={t1 - 6} height={b1 - t1 + 6} fill="var(--chart-grid)" rx={3} />}
        {data.map((d, i) => (
          <g key={d.month}>
            <rect x={cx(i) - step * 0.39} width={step * 0.78} y={sy(d.hi)} height={Math.max(1, sy(d.lo) - sy(d.hi))} fill={band} opacity={0.2} rx={2} />
            <path d={vbar(cx(i) - bw / 2, b1, sy(d.rain), bw, 3.5)} fill={rain} />
          </g>
        ))}
        {months(H1 - 6)}
      </svg>
      <svg viewBox={`0 0 ${w} ${H2}`} width="100%" height={H2} role="img" aria-label="Mean monthly temperature in degrees Celsius" className="mt-3 block">
        <text x={0} y={10} {...AXIS_TEXT}>
          °C
        </text>
        {st.ticks(3).map((t, i) => (
          <g key={t}>
            <line x1={m.l} x2={w - m.r} y1={st(t)} y2={st(t)} stroke={i === 0 ? 'var(--chart-axis)' : 'var(--chart-grid)'} />
            <text x={m.l - 8} y={st(t) + 3.5} textAnchor="end" {...AXIS_TEXT}>
              {t}
            </text>
          </g>
        ))}
        {hi !== null && <line x1={cx(hi)} x2={cx(hi)} y1={t2 - 6} y2={b2} stroke="var(--chart-axis)" strokeDasharray="2 3" />}
        <path d={path(data) ?? ''} fill="none" stroke={temp} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
        {data.map((d, i) => (
          <circle key={d.month} cx={cx(i)} cy={st(d.temp)} r={hi === i ? 4.5 : 2.5} fill={temp} stroke="var(--bg2)" strokeWidth={1.5} />
        ))}
        {months(H2 - 6)}
      </svg>
      <ChartTip
        width={w}
        tip={
          h && hi !== null
            ? {
                x: cx(hi),
                y: sy(Math.max(h.hi, h.rain)),
                body: (
                  <>
                    <TipTitle>{h.month}</TipTitle>
                    <TipRow color={rain} label="Rainfall" value={`${Math.round(h.rain)} mm`} />
                    <TipRow color={band} mark="outline" label="Normal range" value={`${Math.round(h.lo)}–${Math.round(h.hi)} mm`} />
                    <TipRow color={temp} mark="line" label="Mean temperature" value={`${h.temp.toFixed(1)} °C`} />
                  </>
                ),
              }
            : null
        }
      />
    </div>
  )
}
