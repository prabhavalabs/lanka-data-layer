// Several lines on one time axis. A period with no value breaks the line; it is never drawn as zero.
import { extent } from 'd3-array'
import { scaleLinear, scaleTime } from 'd3-scale'
import { line as d3line } from 'd3-shape'
import { useMemo, useState } from 'react'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, useMeasure } from './chartKit'

export type LinePt = { t: Date; period: string; y: number | null }
export type LineSeries = { id: string; label: string; color: string; dashed?: boolean; points: LinePt[] }

const MAX_POINTS = 700

/** Every nth point, always keeping the last, so a daily series stays light in the SVG. */
function thin(points: LinePt[]): LinePt[] {
  if (points.length <= MAX_POINTS) return points
  const step = Math.ceil(points.length / MAX_POINTS)
  return points.filter((_, i) => i % step === 0 || i === points.length - 1)
}

export function MultiLine({
  series,
  unit,
  yFmt,
  xLabel,
  height = 236,
  zero = false,
  ariaLabel,
}: {
  series: LineSeries[]
  unit: string
  yFmt: (v: number) => string
  /** a period as a label for the tooltip */
  xLabel: (period: string) => string
  height?: number
  /** start the y axis at zero (counts and totals) */
  zero?: boolean
  ariaLabel: string
}) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const [hi, setHi] = useState<number | null>(null)
  const drawn = useMemo(() => series.map((s) => ({ ...s, points: thin(s.points) })), [series])
  const all = drawn.flatMap((s) => s.points)
  const [t0, t1] = extent(all, (p) => p.t) as [Date, Date]
  const ys = all.flatMap((p) => (p.y === null ? [] : [p.y]))
  const [y0, y1] = extent(ys) as [number, number]
  const pad = (y1 - y0) * 0.12 || Math.abs(y1) * 0.1 || 1
  const sy = scaleLinear([zero && y0 >= 0 ? 0 : y0 - pad, y1 + pad], [height - 24, 22]).nice(4)
  const yt = sy.ticks(4)
  // the left margin fits the widest tick label
  const m = { l: Math.max(40, 16 + 6 * Math.max(...yt.map((t) => yFmt(t).length))), r: 14, t: 22, b: 24 }
  const sx = scaleTime([t0, t1], [m.l, w - m.r])
  const xt = sx.ticks(Math.max(2, Math.floor((w - m.l - m.r) / 90)))
  const xf = sx.tickFormat()
  const gen = d3line<LinePt>()
    .defined((p) => p.y !== null)
    .x((p) => sx(p.t))
    .y((p) => sy(p.y as number))
  // the moments the pointer can land on: every period of every series, once
  const stops = useMemo(() => {
    const seen = new Map<number, string>()
    for (const s of drawn) for (const p of s.points) seen.set(p.t.getTime(), p.period)
    return [...seen.entries()].sort((a, b) => a[0] - b[0])
  }, [drawn])
  const at = hi === null ? null : stops[hi]
  const dots = all.length <= 80 * Math.max(1, series.length)

  return (
    <div ref={ref} className="relative" data-chart>
      <svg
        viewBox={`0 0 ${w} ${height}`}
        width="100%"
        height={height}
        role="img"
        aria-label={ariaLabel}
        className="block touch-pan-y"
        onPointerMove={(e) => {
          const b = e.currentTarget.getBoundingClientRect()
          const t = sx.invert(((e.clientX - b.left) / b.width) * w).getTime()
          let best = 0
          for (let i = 1; i < stops.length; i++) if (Math.abs(stops[i][0] - t) < Math.abs(stops[best][0] - t)) best = i
          setHi(best)
        }}
        onPointerLeave={() => setHi(null)}
      >
        <text x={0} y={10} {...AXIS_TEXT}>
          {unit}
        </text>
        {yt.map((t) => (
          <g key={t}>
            <line x1={m.l} x2={w - m.r} y1={sy(t)} y2={sy(t)} stroke={t === sy.domain()[0] ? 'var(--chart-axis)' : 'var(--chart-grid)'} />
            <text x={m.l - 8} y={sy(t) + 3.5} textAnchor="end" {...AXIS_TEXT}>
              {yFmt(t)}
            </text>
          </g>
        ))}
        {xt.map((t) => (
          <text key={t.getTime()} x={sx(t)} y={height - 6} textAnchor="middle" {...AXIS_TEXT}>
            {xf(t)}
          </text>
        ))}
        {at && <line x1={sx(new Date(at[0]))} x2={sx(new Date(at[0]))} y1={m.t} y2={height - m.b} stroke="var(--chart-axis)" strokeDasharray="2 3" />}
        {drawn.map((s) => (
          <g key={s.id}>
            <path d={gen(s.points) ?? ''} fill="none" stroke={s.color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" strokeDasharray={s.dashed ? '5 4' : undefined} />
            {/* a point with a missing neighbour on both sides would not show as a line */}
            {s.points.map((p, i) =>
              p.y !== null && (dots || (s.points[i - 1]?.y ?? null) === null && (s.points[i + 1]?.y ?? null) === null) ? <circle key={p.period} cx={sx(p.t)} cy={sy(p.y)} r={3} fill={s.color} stroke="var(--bg2)" strokeWidth={1.5} /> : null,
            )}
          </g>
        ))}
        {at &&
          drawn.map((s) => {
            const p = s.points.find((q) => q.t.getTime() === at[0])
            return p && p.y !== null ? <circle key={s.id} cx={sx(p.t)} cy={sy(p.y)} r={5} fill={s.color} stroke="var(--bg2)" strokeWidth={2} /> : null
          })}
      </svg>
      <ChartTip
        width={w}
        tip={
          at
            ? {
                x: sx(new Date(at[0])),
                y: m.t + 14,
                body: (
                  <>
                    <TipTitle>{xLabel(at[1])}</TipTitle>
                    {drawn.map((s) => {
                      const p = s.points.find((q) => q.t.getTime() === at[0])
                      return <TipRow key={s.id} color={s.color} mark="line" label={s.label} value={p && p.y !== null ? yFmt(p.y) : <span className="font-normal text-ink3">no data</span>} />
                    })}
                  </>
                ),
              }
            : null
        }
      />
    </div>
  )
}
