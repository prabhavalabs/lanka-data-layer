import { extent } from 'd3-array'
import { scaleLinear } from 'd3-scale'
import { line as d3line } from 'd3-shape'
import { useState, type ReactNode } from 'react'
import { AXIS_TEXT, ChartTip, TipRow, TipTitle, useMeasure } from './chartKit'

export type LinePoint = { x: number; y: number }

/** Single-series line, one y-axis. Points from `modelledFrom` on are drawn dashed with hollow markers. */
export function LineChart({
  data,
  color,
  height = 168,
  unit,
  label,
  yFmt = (v) => String(v),
  xFmt = (v) => String(v),
  xTicks,
  zero = false,
  dots = true,
  modelledFrom,
  modelledNote = 'modelled',
  ariaLabel,
}: {
  data: LinePoint[]
  color: string
  height?: number
  /** unit shown at the head of the y-axis */
  unit: string
  /** series name used in the tooltip */
  label: string
  yFmt?: (v: number) => string
  xFmt?: (v: number) => ReactNode
  xTicks?: number[]
  zero?: boolean
  dots?: boolean
  /** x value from which points are projections */
  modelledFrom?: number
  modelledNote?: string
  ariaLabel: string
}) {
  const [ref, w] = useMeasure<HTMLDivElement>()
  const [hi, setHi] = useState<number | null>(null)
  const m = { l: 40, r: 14, t: 22, b: 22 }
  const [x0, x1] = extent(data, (d) => d.x) as [number, number]
  const [y0, y1] = extent(data, (d) => d.y) as [number, number]
  const pad = (y1 - y0) * 0.18 || 1
  const sx = scaleLinear([x0, x1], [m.l, w - m.r])
  const sy = scaleLinear([zero ? 0 : y0 - pad, y1 + pad], [height - m.b, m.t]).nice(4)
  const yt = sy.ticks(4)
  const isModel = (d: LinePoint) => modelledFrom !== undefined && d.x >= modelledFrom
  const solid = data.filter((d) => !isModel(d))
  const dashed = modelledFrom !== undefined ? [solid[solid.length - 1], ...data.filter(isModel)] : []
  const path = d3line<LinePoint>()
    .x((d) => sx(d.x))
    .y((d) => sy(d.y))
  const allTicks = xTicks ?? data.map((d) => d.x)
  const every = Math.max(1, Math.ceil(40 / ((w - m.l - m.r) / Math.max(1, allTicks.length - 1))))
  const ticks = allTicks.filter((_, i) => (allTicks.length - 1 - i) % every === 0)
  const h = hi === null ? null : data[hi]

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
          const px = ((e.clientX - b.left) / b.width) * w
          let best = 0
          data.forEach((d, i) => {
            if (Math.abs(sx(d.x) - px) < Math.abs(sx(data[best].x) - px)) best = i
          })
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
        {ticks.map((t) => (
          <text key={t} x={sx(t)} y={height - 5} textAnchor="middle" {...AXIS_TEXT}>
            {xFmt(t)}
          </text>
        ))}
        {h && <line x1={sx(h.x)} x2={sx(h.x)} y1={m.t} y2={height - m.b} stroke="var(--chart-axis)" strokeDasharray="2 3" />}
        <path d={path(solid) ?? ''} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
        {dashed.length > 1 && <path d={path(dashed) ?? ''} fill="none" stroke={color} strokeWidth={2} strokeDasharray="4 4" strokeLinecap="round" />}
        {dots &&
          data.map((d) => (
            <circle key={d.x} cx={sx(d.x)} cy={sy(d.y)} r={isModel(d) ? 3.5 : 3} fill={isModel(d) ? 'var(--bg2)' : color} stroke={isModel(d) ? color : 'var(--bg2)'} strokeWidth={1.5} />
          ))}
        {h && <circle cx={sx(h.x)} cy={sy(h.y)} r={5} fill={isModel(h) ? 'var(--bg2)' : color} stroke={isModel(h) ? color : 'var(--bg2)'} strokeWidth={2} />}
        {modelledFrom !== undefined && dashed.length > 1 && (
          <text x={sx(dashed[dashed.length - 1].x)} y={sy(dashed[dashed.length - 1].y) - 10} textAnchor="end" {...AXIS_TEXT} fill="var(--ink2)">
            {modelledNote}
          </text>
        )}
      </svg>
      <ChartTip
        width={w}
        tip={
          h
            ? {
                x: sx(h.x),
                y: sy(h.y),
                body: (
                  <>
                    <TipTitle>
                      {xFmt(h.x)}
                      {isModel(h) && <span className="ml-1.5 font-normal text-ink3">{modelledNote}</span>}
                    </TipTitle>
                    <TipRow color={color} mark="line" label={label} value={yFmt(h.y)} />
                  </>
                ),
              }
            : null
        }
      />
    </div>
  )
}
