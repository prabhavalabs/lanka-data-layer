// Every instrument on one axis, 2010 to today: baselines are tall marks, amendments small ones, banded by head of state.
// The date handle (drag it, press the arrow keys, or click a mark or the axis) sets the date the rest of the page shows.
import { scaleTime } from 'd3-scale'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import type { Instrument } from '@/api/civic'
import { useMeasure } from '@/components/charts/chartKit'
import { fmtDay, isoDay, parseDay } from './shared'

const START = new Date(2010, 0, 1, 12)
const DAY = 86_400_000
const MIN_W = 1180
const M = { l: 14, r: 18 }
const H = 150
const AXIS_Y = 112
const BAND_H = 24

/** Presidential terms, the fallback when an instrument carries no `head_of_state` and the extent of a band for a known name. */
const TERMS = [
  { name: 'Rajapaksa', from: '2005-11-19', to: '2015-01-09' },
  { name: 'Sirisena', from: '2015-01-09', to: '2019-11-18' },
  { name: 'G. Rajapaksa', from: '2019-11-18', to: '2022-07-15' },
  { name: 'Wickremesinghe', from: '2022-07-15', to: '2024-09-23' },
  { name: 'Dissanayake', from: '2024-09-23', to: '9999-12-31' },
]
const termByDate = (iso: string) => TERMS.find((t) => iso >= t.from && iso < t.to)?.name
/** The engine sends full names ("Gotabaya Rajapaksa"); the first name tells the two Rajapaksas apart. */
const FIRST: Record<string, string> = { mahinda: 'Rajapaksa', maithripala: 'Sirisena', gotabaya: 'G. Rajapaksa', ranil: 'Wickremesinghe', anura: 'Dissanayake' }
const termByName = (name: string) => {
  const n = name.toLowerCase()
  const label = TERMS.find((t) => t.name.toLowerCase() === n)?.name ?? FIRST[n.split(/\s+/)[0]] ?? TERMS.find((t) => n.endsWith(` ${t.name.toLowerCase()}`))?.name
  return TERMS.find((t) => t.name === label)
}

type Band = { name: string; from: Date; to: Date }
function bandsOf(instruments: Instrument[], end: Date): Band[] {
  const groups = new Map<string, string[]>()
  for (const i of instruments) {
    const head = i.head_of_state ? (termByName(i.head_of_state)?.name ?? i.head_of_state) : termByDate(i.effective_from)
    if (head) groups.set(head, [...(groups.get(head) ?? []), i.effective_from])
  }
  const bands: Band[] = []
  for (const [name, dates] of groups) {
    const term = TERMS.find((t) => t.name === name)
    const sorted = [...dates].sort()
    const from = term ? parseDay(term.from) : parseDay(sorted[0])
    const to = term && term.to < '9999' ? parseDay(term.to) : term ? end : parseDay(sorted[sorted.length - 1])
    bands.push({ name, from: from < START ? START : from, to: to > end ? end : to })
  }
  return bands.sort((a, b) => +a.from - +b.from)
}

export function Timeline({ instruments, at, onChange, today }: { instruments: Instrument[]; at: string; onChange: (iso: string) => void; today: string }) {
  const [boxRef, boxW] = useMeasure<HTMLDivElement>(MIN_W)
  const scroller = useRef<HTMLDivElement | null>(null)
  const svgRef = useRef<SVGSVGElement | null>(null)
  const [drag, setDrag] = useState<string | null>(null)
  const w = Math.max(boxW, MIN_W)
  const end = useMemo(() => parseDay(today), [today])
  const x = useMemo(() => scaleTime().domain([START, end]).range([M.l, w - M.r]), [end, w])
  const bands = useMemo(() => bandsOf(instruments, end), [instruments, end])
  const sorted = useMemo(() => [...instruments].sort((a, b) => a.effective_from.localeCompare(b.effective_from)), [instruments])
  const shown = drag ?? at
  const hx = Math.min(Math.max(x(parseDay(shown)), M.l), w - M.r)
  // 2403/53 and 2412/08 sit a few pixels apart: label only the newer of any crowded pair
  const labelled = useMemo(() => {
    const bs = sorted.filter((i) => i.kind === 'baseline')
    return new Set(bs.filter((b, k) => !bs[k + 1] || x(parseDay(bs[k + 1].effective_from)) - x(parseDay(b.effective_from)) > 44).map((b) => b.doc_id))
  }, [sorted, x])
  // marks in a cluster share the gap between them as their click target, so each stays reachable
  const hitW = useMemo(() => {
    const m = new Map<string, number>()
    sorted.forEach((i, k) => {
      const px = x(parseDay(i.effective_from))
      const gaps = [sorted[k - 1], sorted[k + 1]].filter(Boolean).map((o) => Math.abs(x(parseDay(o.effective_from)) - px))
      m.set(i.doc_id, Math.max(3, Math.min(14, Math.min(...gaps, 14))))
    })
    return m
  }, [sorted, x])
  const years = useMemo(() => Array.from({ length: end.getFullYear() - 2010 + 1 }, (_, i) => 2010 + i), [end])

  const clampDay = (d: Date) => isoDay(new Date(Math.min(Math.max(+d, +START), +end)))
  const dayAt = (clientX: number) => {
    const r = svgRef.current!.getBoundingClientRect()
    return clampDay(x.invert((clientX - r.left) * (w / r.width)))
  }
  /** a release within a few pixels of an instrument lands exactly on its date */
  const snap = (iso: string) => {
    const px = x(parseDay(iso))
    let near: Instrument | undefined
    let best = 5
    for (const i of sorted) {
      const d = Math.abs(x(parseDay(i.effective_from)) - px)
      if (d <= best) (near = i), (best = d)
    }
    return near ? near.effective_from : iso
  }

  // keep the handle in view when the date changes from outside (a mark, the URL, the keyboard)
  useEffect(() => {
    const el = scroller.current
    if (!el || drag) return
    if (hx < el.scrollLeft + 24 || hx > el.scrollLeft + el.clientWidth - 24) el.scrollTo({ left: Math.max(0, hx - el.clientWidth / 2), behavior: 'smooth' })
  }, [hx, drag])

  const step = (dir: 1 | -1) => {
    const t = dir > 0 ? sorted.find((i) => i.effective_from > shown) : [...sorted].reverse().find((i) => i.effective_from < shown)
    if (t) onChange(t.effective_from)
  }
  const shift = (days: number) => onChange(clampDay(new Date(+parseDay(shown) + days * DAY)))
  const onKey = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 30 : 1
    const jump = (dir: 1 | -1) => {
      const t = dir > 0 ? sorted.find((i) => i.effective_from > shown) : [...sorted].reverse().find((i) => i.effective_from < shown)
      if (t) onChange(t.effective_from)
    }
    const k = e.key
    if (k === 'ArrowLeft') shift(-step)
    else if (k === 'ArrowRight') shift(step)
    else if (k === 'PageDown') shift(-365)
    else if (k === 'PageUp') shift(365)
    else if (k === 'ArrowUp') jump(1)
    else if (k === 'ArrowDown') jump(-1)
    else if (k === 'Home') onChange(isoDay(START))
    else if (k === 'End') onChange(today)
    else return
    e.preventDefault()
  }

  const dragging = useRef(false)
  const down = (e: PointerEvent) => {
    dragging.current = true
    e.currentTarget.setPointerCapture(e.pointerId)
    setDrag(dayAt(e.clientX))
  }
  const move = (e: PointerEvent) => dragging.current && setDrag(dayAt(e.clientX))
  const up = (e: PointerEvent) => {
    if (!dragging.current) return
    dragging.current = false
    const iso = snap(dayAt(e.clientX))
    setDrag(null)
    onChange(iso)
  }

  return (
    <div ref={boxRef} className="min-w-0">
      <div ref={scroller} className="scroll-thin overflow-x-auto rounded-2xl border border-line bg-bg2" tabIndex={-1}>
        <svg
          ref={svgRef}
          width={w}
          height={H}
          viewBox={`0 0 ${w} ${H}`}
          className="block select-none"
          role="group"
          aria-label="Timeline of gazettes from 2010 to today. Choose a mark, or move the date handle."
          onClick={(e) => {
            if ((e.target as Element).closest('[data-mark],[data-handle]')) return
            onChange(snap(dayAt(e.clientX)))
          }}
        >
          {/* head-of-state bands */}
          {bands.map((b, i) => {
            const x0 = x(b.from)
            const bw = x(b.to) - x0
            return (
              <g key={b.name}>
                <rect x={x0} y={0} width={Math.max(0, bw)} height={AXIS_Y} fill={i % 2 ? 'var(--bg3)' : 'var(--bg4)'} opacity={i % 2 ? 0.55 : 0.4} />
                <rect x={x0} y={0} width={Math.max(0, bw)} height={BAND_H} fill="none" />
                {bw > 56 && (
                  <text x={x0 + 8} y={16} fontSize={11} fontWeight={600} fill="var(--ink2)" fontFamily="var(--font-sans)">
                    {b.name}
                  </text>
                )}
                {i > 0 && <line x1={x0} x2={x0} y1={0} y2={AXIS_Y} stroke="var(--line2)" />}
              </g>
            )
          })}
          {/* axis */}
          <line x1={M.l} x2={w - M.r} y1={AXIS_Y} y2={AXIS_Y} stroke="var(--chart-axis)" />
          {years.map((y) => {
            const px = x(new Date(y, 0, 1, 12))
            return (
              <g key={y}>
                <line x1={px} x2={px} y1={AXIS_Y} y2={AXIS_Y + 5} stroke="var(--chart-axis)" />
                {y % 2 === 0 && px < w - 20 && (
                  <text x={px} y={AXIS_Y + 20} textAnchor="middle" fontSize={10.5} fill="var(--ink3)" fontFamily="var(--font-sans)" style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {y}
                  </text>
                )}
              </g>
            )
          })}
          {/* instruments */}
          {sorted.map((i) => {
            const px = x(parseDay(i.effective_from))
            const base = i.kind === 'baseline'
            const parsedOk = i.portfolios > 0 || i.kind === 'amendment'
            const h = base ? 52 : i.kind === 'supplement' ? 34 : 18
            const bw = base ? 6 : i.kind === 'supplement' ? 5 : 3
            const hit = hitW.get(i.doc_id) ?? 12
            const label = `Gazette ${i.number}, ${i.kind}, ${fmtDay(i.effective_from)}${i.head_of_state ? `, President ${i.head_of_state}` : ''}${parsedOk ? '' : ', not yet parsed'}`
            const select = () => onChange(i.effective_from)
            return (
              <g
                key={i.doc_id}
                data-mark
                role="button"
                tabIndex={0}
                aria-label={label}
                aria-current={i.effective_from === shown ? 'date' : undefined}
                className="cursor-pointer outline-none [&:focus-visible_rect.m]:stroke-[var(--brand)] [&:focus-visible_rect.m]:stroke-2"
                onClick={select}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    select()
                  }
                }}
              >
                <title>{label}</title>
                <rect x={px - hit / 2} y={AXIS_Y - h - 6} width={hit} height={h + 6} fill="transparent" />
                <rect
                  className="m"
                  x={px - bw / 2}
                  y={AXIS_Y - h}
                  width={bw}
                  height={h}
                  rx={2}
                  fill={parsedOk ? (base ? 'var(--ink)' : i.kind === 'supplement' ? 'var(--ink2)' : 'var(--brand)') : 'var(--bg2)'}
                  stroke={parsedOk ? 'none' : 'var(--ink3)'}
                  strokeWidth={1.25}
                  strokeDasharray={parsedOk ? undefined : '2 2'}
                />
                {base && labelled.has(i.doc_id) && (
                  <text x={px} y={AXIS_Y - h - 6} textAnchor="middle" fontSize={9.5} fill="var(--ink3)" fontFamily="var(--font-mono)">
                    {i.number}
                  </text>
                )}
              </g>
            )
          })}
          {/* the handle */}
          <g data-handle>
            <line x1={hx} x2={hx} y1={BAND_H} y2={AXIS_Y} stroke="var(--brand)" strokeWidth={1.5} strokeDasharray="3 3" pointerEvents="none" />
            <g
              role="slider"
              tabIndex={0}
              aria-label="Date shown below"
              aria-orientation="horizontal"
              aria-valuemin={0}
              aria-valuemax={Math.round((+end - +START) / DAY)}
              aria-valuenow={Math.round((+parseDay(shown) - +START) / DAY)}
              aria-valuetext={fmtDay(shown)}
              className="cursor-ew-resize outline-none [&:focus-visible_circle.g]:stroke-ink [&:focus-visible_circle.g]:stroke-2"
              style={{ touchAction: 'none' }}
              onPointerDown={down}
              onPointerMove={move}
              onPointerUp={up}
              onPointerCancel={() => ((dragging.current = false), setDrag(null))}
              onKeyDown={onKey}
            >
              <circle cx={hx} cy={BAND_H + 14} r={18} fill="transparent" />
              <circle className="g" cx={hx} cy={BAND_H + 14} r={8} fill="var(--brand)" stroke="var(--bg2)" strokeWidth={2.5} />
              <path d={`M${hx - 2.5} ${BAND_H + 11}v6M${hx + 2.5} ${BAND_H + 11}v6`} stroke="var(--on-brand)" strokeWidth={1.2} strokeLinecap="round" pointerEvents="none" />
            </g>
          </g>
        </svg>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2">
        <div className="inline-flex items-center overflow-hidden rounded-lg border border-line2 bg-bg2 text-[13px]">
          <button type="button" onClick={() => step(-1)} aria-label="Previous gazette" className="pressable grid h-9 w-9 place-items-center text-ink2 hover:bg-bg3 hover:text-ink">
            <ChevronLeft className="size-4" />
          </button>
          <span className="t-num min-w-[112px] border-x border-line2 px-3 py-2 text-center font-semibold text-brand-ink" aria-live="polite">
            {fmtDay(shown)}
          </span>
          <button type="button" onClick={() => step(1)} aria-label="Next gazette" className="pressable grid h-9 w-9 place-items-center text-ink2 hover:bg-bg3 hover:text-ink">
            <ChevronRight className="size-4" />
          </button>
        </div>
        <button type="button" onClick={() => onChange(today)} className="pressable rounded-lg px-2.5 py-2 text-[12.5px] font-medium text-ink2 hover:bg-bg3 hover:text-ink">
          Today
        </button>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px] text-ink3">
        <span className="inline-flex items-center gap-1.5"><span className="inline-block h-3.5 w-1.5 rounded-[2px] bg-ink" /> Baseline gazette</span>
        <span className="inline-flex items-center gap-1.5"><span className="inline-block h-2.5 w-1 rounded-[2px] bg-ink2" /> Supplement</span>
        <span className="inline-flex items-center gap-1.5"><span className="inline-block h-2 w-1 rounded-[2px] bg-brand" /> Amendment</span>
        <span className="inline-flex items-center gap-1.5"><span className="inline-block h-3.5 w-1.5 rounded-[2px] border border-dashed border-ink3" /> Not yet read</span>
        <span className="max-md:hidden">Drag the handle, click a mark, or use the arrow keys. Marks within a cluster: use the arrows beside the date.</span>
        <span className="md:hidden">Scroll sideways; tap a mark.</span>
      </div>
    </div>
  )
}
