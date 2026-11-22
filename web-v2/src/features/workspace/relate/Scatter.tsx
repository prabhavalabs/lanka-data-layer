// Scatter of the aligned units, with the fitted line and its 95 % band. One x axis, one y axis.
// Above a few hundred units the marks become hexagonal bins of count: position is still read exactly,
// overplotting is not.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { RelateData } from '@/api/workspace'
import { rampAt, rampStops, rgbToHex } from '@/lib/palettes'
import type { Theme } from '@/lib/hooks'
import { olsBand } from '@/lib/stats'
import { fmt, fmtWith, unitSuffix } from '../format'

const M = { l: 44, r: 12, t: 12, b: 30 }
const H = 248
const R = 7

function niceStep(span: number, target: number) {
  const raw = span / target
  const mag = 10 ** Math.floor(Math.log10(raw))
  const n = raw / mag
  return (n < 1.5 ? 1 : n < 3.5 ? 2 : n < 7.5 ? 5 : 10) * mag
}

export function Scatter({ data, mode, colors, scheme, hoverCode, onHover }: { data: RelateData; mode: 'points' | 'hexbin'; colors: string[]; scheme: Theme; hoverCode: string | null; onHover: (code: string | null) => void }) {
  // density is a magnitude: one hue, light to dark (mirrored on a dark panel)
  const density = useMemo(() => rampStops('ldl.seq.violet', scheme), [scheme])
  const wrap = useRef<HTMLDivElement>(null)
  const [W, setW] = useState(340)
  const [near, setNear] = useState<number | null>(null)
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const ro = new ResizeObserver(() => setW(el.clientWidth))
    ro.observe(el)
    setW(el.clientWidth)
    return () => ro.disconnect()
  }, [])
  const { table, x, y, fit, breaks } = data
  const logY = y.transform === 'log', logX = x.transform === 'log'
  const model = useMemo(() => {
    const tx = (v: number) => (logX ? Math.log(v) : v), ty = (v: number) => (logY ? Math.log(v) : v)
    const xs = table.x, ys = table.y
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity
    for (let i = 0; i < xs.length; i++) {
      x0 = Math.min(x0, xs[i])
      x1 = Math.max(x1, xs[i])
      y0 = Math.min(y0, ys[i])
      y1 = Math.max(y1, ys[i])
    }
    if (!logX) x0 = Math.min(0, x0)
    const px = (v: number) => M.l + ((tx(v) - tx(x0)) / (tx(x1) - tx(x0) || 1)) * (W - M.l - M.r)
    const py = (v: number) => H - M.b - ((ty(v) - ty(y0)) / (ty(y1) - ty(y0) || 1)) * (H - M.t - M.b) * 0.96
    const xt: number[] = []
    if (logX) for (let v = 10 ** Math.floor(Math.log10(x0)); v <= x1; v *= 10) for (const k of [1, 2, 5]) if (v * k >= x0 && v * k <= x1) xt.push(v * k)
    if (!logX) {
      const step = niceStep(x1 - x0, 5)
      for (let v = Math.ceil(x0 / step) * step; v <= x1 + 1e-9; v += step) xt.push(+v.toPrecision(6))
    }
    const yt: number[] = []
    if (logY) for (let v = 10 ** Math.floor(Math.log10(y0)); v <= y1; v *= 10) for (const k of [1, 2.5, 5]) if (v * k >= y0 && v * k <= y1) yt.push(v * k)
    if (!logY) {
      const step = niceStep(y1 - y0, 4)
      for (let v = Math.ceil(y0 / step) * step; v <= y1 + 1e-9; v += step) yt.push(+v.toPrecision(6))
    }
    // the fitted line and its band, sampled across x
    const line: string[] = [], hi: string[] = [], lo: string[] = [], robust: string[] = []
    for (let k = 0; k <= 40; k++) {
      const vx = logX ? Math.exp(Math.log(x0) + (k / 40) * (Math.log(x1) - Math.log(x0))) : x0 + (k / 40) * (x1 - x0)
      const t = tx(vx)
      const m = fit.intercept + fit.slope * t, band = olsBand(fit, t)
      const un = (v: number) => (logY ? Math.exp(v) : v)
      const X = px(vx).toFixed(1)
      line.push(`${X},${py(un(m)).toFixed(1)}`)
      hi.push(`${X},${py(un(m + band)).toFixed(1)}`)
      lo.push(`${X},${py(un(m - band)).toFixed(1)}`)
      robust.push(`${X},${py(un(fit.robust.intercept + fit.robust.slope * t)).toFixed(1)}`)
    }
    const pts = xs.map((v, i) => [px(v), py(ys[i])] as [number, number])
    // hexagonal bins in pixel space
    const dx = R * Math.sqrt(3), dy = R * 1.5
    const bins = new Map<string, { cx: number; cy: number; n: number }>()
    for (const [X, Y] of pts) {
      const row = Math.round(Y / dy)
      const col = Math.round((X - (row & 1 ? dx / 2 : 0)) / dx)
      const key = `${col},${row}`
      const b = bins.get(key)
      if (b) b.n++
      else bins.set(key, { cx: col * dx + (row & 1 ? dx / 2 : 0), cy: row * dy, n: 1 })
    }
    const hex = Array.from({ length: 6 }, (_, k) => `${(R * Math.sin((k * Math.PI) / 3)).toFixed(2)},${(-R * Math.cos((k * Math.PI) / 3)).toFixed(2)}`).join(' ')
    return { px, py, xt, yt, line: line.join(' '), band: [...hi, ...lo.reverse()].join(' '), robust: robust.join(' '), pts, bins: [...bins.values()], maxN: Math.max(1, ...[...bins.values()].map((b) => b.n)), hex, x0, x1, y0, y1 }
  }, [table, fit, W, logX, logY])

  const raf = useRef(0)
  const onMove = (e: React.PointerEvent<SVGSVGElement>) => {
    const box = e.currentTarget.getBoundingClientRect()
    const mx = e.clientX - box.left, my = e.clientY - box.top
    cancelAnimationFrame(raf.current)
    raf.current = requestAnimationFrame(() => {
      let best = -1, bd = 18 * 18
      for (let i = 0; i < model.pts.length; i++) {
        const d = (model.pts[i][0] - mx) ** 2 + (model.pts[i][1] - my) ** 2
        if (d < bd) {
          bd = d
          best = i
        }
      }
      setNear(best < 0 ? null : best)
      onHover(best < 0 ? null : table.codes[best])
    })
  }
  const leave = () => {
    cancelAnimationFrame(raf.current)
    setNear(null)
    onHover(null)
  }
  useEffect(() => () => cancelAnimationFrame(raf.current), [])
  // a unit hovered on the map lights its point here
  const linked = near ?? (hoverCode ? table.codes.indexOf(hoverCode) : -1)
  const mark = linked >= 0 ? model.pts[linked] : null
  const tipLeft = mark ? Math.min(W - 168, Math.max(4, mark[0] + (mark[0] > W * 0.6 ? -172 : 12))) : 0

  return (
    <div ref={wrap} className="relative">
      <svg width={W} height={H} className="ws-chart block touch-none select-none" role="img" aria-label={`Scatter of ${y.label} against ${x.label} for ${table.codes.length} units, with a fitted line`} onPointerMove={onMove} onPointerLeave={leave}>
        {model.yt.map((v) => (
          <g key={v}>
            <line x1={M.l} x2={W - M.r} y1={model.py(v)} y2={model.py(v)} stroke="var(--chart-grid)" />
            <text x={M.l - 7} y={model.py(v) + 3} textAnchor="end">
              {fmt(v, y.unit)}
            </text>
          </g>
        ))}
        {model.xt.map((v) => (
          <text key={v} x={model.px(v)} y={H - M.b + 14} textAnchor="middle">
            {fmt(v, x.unit).replace(/\.0$/, '')}
          </text>
        ))}
        <line x1={M.l} x2={W - M.r} y1={H - M.b} y2={H - M.b} stroke="var(--chart-axis)" />
        {/* where the map's thirds fall */}
        {breaks.x.map((b) => b > model.x0 && b < model.x1 && <line key={`x${b}`} x1={model.px(b)} x2={model.px(b)} y1={M.t} y2={H - M.b} stroke="var(--ink3)" strokeDasharray="2 3" opacity="0.5" />)}
        {breaks.y.map((b) => b > model.y0 && b < model.y1 && <line key={`y${b}`} x1={M.l} x2={W - M.r} y1={model.py(b)} y2={model.py(b)} stroke="var(--ink3)" strokeDasharray="2 3" opacity="0.5" />)}

        {mode === 'hexbin'
          ? model.bins.map((b) => <polygon key={`${b.cx},${b.cy}`} points={model.hex} transform={`translate(${b.cx.toFixed(1)},${b.cy.toFixed(1)})`} fill={rgbToHex(rampAt(density, 0.26 + 0.74 * Math.sqrt(b.n / model.maxN)))} stroke="var(--bg2)" strokeWidth="1" />)
          : model.pts.map(([X, Y], i) => (table.estimated[i] ? <circle key={i} cx={X} cy={Y} r="2.4" fill="none" stroke={colors[table.cls[i]]} strokeWidth="1.1" /> : <circle key={i} cx={X} cy={Y} r="2.7" fill={colors[table.cls[i]]} stroke="var(--bg2)" strokeWidth="0.6" />))}

        <polygon points={model.band} fill="var(--ink)" fillOpacity="0.16" />
        <polyline points={model.robust} fill="none" stroke="var(--ink2)" strokeWidth="1.2" strokeDasharray="4 3" />
        <polyline points={model.line} fill="none" stroke="var(--bg2)" strokeWidth="4.5" strokeLinecap="round" opacity="0.85" />
        <polyline points={model.line} fill="none" stroke="var(--ink)" strokeWidth="2" strokeLinecap="round" />
        {mark && (
          <g>
            <circle cx={mark[0]} cy={mark[1]} r="6.5" fill="none" stroke="var(--bg2)" strokeWidth="3.5" />
            <circle cx={mark[0]} cy={mark[1]} r="6.5" fill={colors[table.cls[linked]]} stroke="var(--ink)" strokeWidth="1.6" />
          </g>
        )}
        <text x={W - M.r} y={H - 3} textAnchor="end" className="ws-axis-title">
          {x.label} ({unitSuffix(x.unit)}{logX ? ', log' : ''}) →
        </text>
      </svg>
      {mode === 'hexbin' && (
        <div className="absolute top-0 right-3 flex items-center gap-1.5 font-mono text-[10px] text-ink3" aria-hidden>
          1
          <span className="h-1.5 w-12 rounded-full" style={{ background: `linear-gradient(to right, ${density.slice(1).join(', ')})` }} />
          {model.maxN} units
        </div>
      )}
      {mark && (
        <div className="pointer-events-none absolute z-10 w-[164px] rounded-lg border border-line2 bg-bg2 px-2.5 py-2 shadow-soft" style={{ left: tipLeft, top: Math.max(0, mark[1] - 62) }}>
          <div className="truncate text-[12px] font-semibold text-ink">{table.names[linked]}</div>
          <div className="mt-1 flex justify-between gap-2 text-[11.5px]">
            <span className="truncate text-ink3">{x.short || 'x'}</span>
            <span className="t-num font-mono text-ink">{fmtWith(table.x[linked], x.unit)}</span>
          </div>
          <div className="flex justify-between gap-2 text-[11.5px]">
            <span className="truncate text-ink3">{y.short || 'y'}</span>
            <span className="t-num font-mono text-ink">{fmtWith(table.y[linked], y.unit)}</span>
          </div>
        </div>
      )}
    </div>
  )
}
