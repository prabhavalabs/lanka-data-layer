import { scaleLinear } from 'd3-scale'
import { useState } from 'react'
import type { PyramidRow } from '@/api/types'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, hbar, useMeasure } from './chartKit'

/** Population pyramid: current year as bars, the comparison year as a stepped outline. */
export function Pyramid({ rows, male, female, yearNow, yearThen }: { rows: PyramidRow[]; male: string; female: string; yearNow: number; yearThen: number }) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const [hi, setHi] = useState<number | null>(null)
  const mid = 48
  const top = 24
  const rowH = 17
  const barH = 12
  const data = [...rows].reverse() // oldest at the top
  const half = (w - mid) / 2 - 4
  const max = Math.ceil(Math.max(...rows.flatMap((r) => [r.m, r.f, r.m0, r.f0])))
  const sx = scaleLinear([0, max], [0, half])
  const xl = w / 2 - mid / 2
  const xr = w / 2 + mid / 2
  const bottom = top + data.length * rowH
  const height = bottom + 34
  const y = (i: number) => top + i * rowH
  const step = (side: -1 | 1, key: 'm0' | 'f0') => {
    const base = side < 0 ? xl : xr
    return data.map((r, i) => `${i ? 'H' : 'M'}${base + side * sx(r[key])}${i ? '' : ` ${y(0)}`}V${y(i) + rowH}`).join('')
  }
  const ticks = sx.ticks(w < 420 ? 2 : 4).filter((t) => t > 0)
  const h = hi === null ? null : data[hi]
  const pct = (v: number) => `${v.toFixed(1)}%`

  return (
    <div ref={ref} className="relative" data-chart>
      <svg
        viewBox={`0 0 ${w} ${height}`}
        width="100%"
        height={height}
        role="img"
        aria-label={`Population pyramid by five-year age band and sex, ${yearNow} against ${yearThen}`}
        className="block touch-pan-y"
        onPointerMove={(e) => {
          const b = e.currentTarget.getBoundingClientRect()
          const i = Math.floor((e.clientY - b.top - top) / rowH)
          setHi(i >= 0 && i < data.length ? i : null)
        }}
        onPointerLeave={() => setHi(null)}
      >
        <text x={xl} y={11} textAnchor="end" {...AXIS_TEXT} fill="var(--ink2)">
          Male
        </text>
        <text x={xr} y={11} {...AXIS_TEXT} fill="var(--ink2)">
          Female
        </text>
        <text x={w / 2} y={11} textAnchor="middle" {...AXIS_TEXT}>
          age
        </text>
        {ticks.map((t) => (
          <g key={t}>
            <line x1={xl - sx(t)} x2={xl - sx(t)} y1={top} y2={bottom} stroke="var(--chart-grid)" />
            <line x1={xr + sx(t)} x2={xr + sx(t)} y1={top} y2={bottom} stroke="var(--chart-grid)" />
            <text x={xl - sx(t)} y={bottom + 14} textAnchor="middle" {...AXIS_TEXT}>
              {t}
            </text>
            <text x={xr + sx(t)} y={bottom + 14} textAnchor="middle" {...AXIS_TEXT}>
              {t}
            </text>
          </g>
        ))}
        <text x={xl} y={bottom + 14} textAnchor="middle" {...AXIS_TEXT}>
          0
        </text>
        <text x={xr} y={bottom + 14} textAnchor="middle" {...AXIS_TEXT}>
          0
        </text>
        <text x={w / 2} y={bottom + 30} textAnchor="middle" {...AXIS_TEXT}>
          % of residents
        </text>
        {hi !== null && <rect x={0} width={w} y={y(hi)} height={rowH} fill="var(--chart-grid)" rx={3} />}
        {data.map((r, i) => (
          <g key={r.age}>
            <path d={hbar(xl, xl - sx(r.m), y(i) + (rowH - barH) / 2, barH)} fill={male} />
            <path d={hbar(xr, xr + sx(r.f), y(i) + (rowH - barH) / 2, barH)} fill={female} />
            <text x={w / 2} y={y(i) + rowH / 2 + 3.5} textAnchor="middle" {...AXIS_TEXT} fill={hi === i ? 'var(--ink)' : 'var(--ink2)'}>
              {r.age}
            </text>
          </g>
        ))}
        <line x1={xl} x2={xl} y1={top} y2={bottom} stroke="var(--chart-axis)" />
        <line x1={xr} x2={xr} y1={top} y2={bottom} stroke="var(--chart-axis)" />
        <path d={step(-1, 'm0')} fill="none" stroke="var(--ink)" strokeWidth={1.25} strokeLinejoin="round" opacity={0.75} />
        <path d={step(1, 'f0')} fill="none" stroke="var(--ink)" strokeWidth={1.25} strokeLinejoin="round" opacity={0.75} />
      </svg>
      <ChartTip
        width={w}
        tip={
          h && hi !== null
            ? {
                x: w / 2,
                y: y(hi),
                body: (
                  <>
                    <TipTitle>Age {h.age}</TipTitle>
                    <TipRow color={male} label={`Male ${yearNow}`} value={pct(h.m)} />
                    <TipRow color="var(--ink)" mark="outline" label={`Male ${yearThen}`} value={pct(h.m0)} />
                    <TipRow color={female} label={`Female ${yearNow}`} value={pct(h.f)} />
                    <TipRow color="var(--ink)" mark="outline" label={`Female ${yearThen}`} value={pct(h.f0)} />
                  </>
                ),
              }
            : null
        }
      />
    </div>
  )
}
