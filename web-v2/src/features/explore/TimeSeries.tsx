import { fmtBare } from './model'

type Point = { year: number; value: number }

/** The national line is only drawn when it sits on a comparable scale to the area's own values. */
export function sharesScale(points: Point[] | undefined, reference: Point[] | undefined): boolean {
  if (!points?.length || !reference?.length) return false
  const vals = points.map((p) => p.value)
  const ref = reference.map((p) => p.value)
  return Math.max(...ref) <= Math.max(...vals) * 3 && Math.min(...ref) >= Math.min(...vals) / 3
}

/**
 * One line across the indicator's years. Thin mark, recessive grid, one axis;
 * the current year is marked, and clicking a year moves the map to it.
 */
export function TimeSeries({
  points,
  reference,
  year,
  unit,
  color,
  label,
  onYear,
}: {
  points: Point[]
  reference?: Point[]
  year: number
  unit: string
  color: string
  label: string
  onYear: (year: number) => void
}) {
  const W = 332
  const H = 132
  const m = { l: 44, r: 12, t: 10, b: 22 }
  if (points.length < 2) return null
  const vals = points.map((p) => p.value)
  let lo = Math.min(...vals)
  let hi = Math.max(...vals)
  // show the national line only when it shares a sensible scale with the area
  const refVals = reference?.map((p) => p.value) ?? []
  const showRef = sharesScale(points, reference)
  if (showRef) {
    lo = Math.min(lo, ...refVals)
    hi = Math.max(hi, ...refVals)
  }
  const pad = (hi - lo || Math.abs(hi) || 1) * 0.1
  const y0 = lo - pad
  const y1 = hi + pad
  const first = points[0].year
  const last = points[points.length - 1].year
  const x = (yr: number) => m.l + ((yr - first) / (last - first || 1)) * (W - m.l - m.r)
  const y = (v: number) => m.t + (1 - (v - y0) / (y1 - y0)) * (H - m.t - m.b)
  const path = (pts: Point[]) => pts.map((p, i) => `${i ? 'L' : 'M'}${x(p.year).toFixed(1)} ${y(p.value).toFixed(1)}`).join('')
  const cur = points.find((p) => p.year === year) ?? points[points.length - 1]
  const step = (W - m.l - m.r) / (points.length - 1)

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="block w-full overflow-visible" role="img" aria-label={`${label}, ${first} to ${last}. ${fmtBare(points[0].value, unit)} in ${first}, ${fmtBare(points[points.length - 1].value, unit)} in ${last}.`}>
      {[hi, (hi + lo) / 2, lo].map((v, i) => (
        <g key={i}>
          <line x1={m.l} x2={W - m.r} y1={y(v)} y2={y(v)} stroke="var(--chart-grid)" strokeWidth={1} />
          {i !== 1 && (
            <text x={m.l - 8} y={y(v) + 3.5} textAnchor="end" className="t-num fill-ink3 font-mono text-[10px]">
              {fmtBare(v, unit)}
            </text>
          )}
        </g>
      ))}
      {showRef && reference && <path d={path(reference)} fill="none" stroke="var(--chart-axis)" strokeWidth={1} strokeDasharray="3 3" />}
      <line x1={x(cur.year)} x2={x(cur.year)} y1={m.t} y2={H - m.b} stroke="var(--ink3)" strokeWidth={1} />
      <path d={path(points)} fill="none" stroke={color} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={x(cur.year)} cy={y(cur.value)} r={4} fill={color} stroke="var(--bg2)" strokeWidth={2} />
      {[first, last].map(
        (yr) =>
          Math.abs(x(yr) - x(cur.year)) > 34 && (
            <text key={yr} x={x(yr)} y={H - 6} textAnchor={yr === first ? 'start' : 'end'} className="t-num fill-ink3 font-mono text-[10px]">
              {yr}
            </text>
          ),
      )}
      <text x={x(cur.year)} y={H - 6} textAnchor={cur.year === first ? 'start' : cur.year === last ? 'end' : 'middle'} className="t-num fill-ink font-mono text-[10px] font-medium">
        {cur.year}
      </text>
      {points.map((p) => (
        <rect key={p.year} x={x(p.year) - step / 2} y={0} width={step} height={H} fill="transparent" className="cursor-pointer" onClick={() => onYear(p.year)}>
          <title>{`${p.year}: ${fmtBare(p.value, unit)}`}</title>
        </rect>
      ))}
    </svg>
  )
}
