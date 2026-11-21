import { cn } from '@/lib/utils'

/** Inline trend. Decorative by default (the value sits beside it), so no axes. */
export function Sparkline({
  data,
  width = 88,
  height = 26,
  color = 'var(--ink2)',
  fill = false,
  dot = true,
  className,
}: {
  data: number[]
  width?: number
  height?: number
  color?: string
  fill?: boolean
  dot?: boolean
  className?: string
}) {
  if (data.length < 2) return null
  const min = Math.min(...data)
  const max = Math.max(...data)
  const pad = 3
  const x = (i: number) => pad + (i / (data.length - 1)) * (width - pad * 2)
  const y = (v: number) => height - pad - ((v - min) / (max - min || 1)) * (height - pad * 2)
  const d = data.map((v, i) => `${i ? 'L' : 'M'}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join('')
  const last = data.length - 1
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} className={cn('shrink-0 overflow-visible', className)} aria-hidden>
      {fill && <path d={`${d}L${x(last)} ${height}L${x(0)} ${height}Z`} fill={color} opacity={0.12} />}
      <path d={d} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" />
      {dot && <circle cx={x(last)} cy={y(data[last])} r={2.5} fill={color} stroke="var(--bg2)" strokeWidth={1.5} />}
    </svg>
  )
}
