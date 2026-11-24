// Ministries that lost institutions on the left, ministries that gained them on the right, and one ribbon per
// (from, to) pair of moved institutions, as wide as the count. d3-shape draws the curves; the labels are HTML so they wrap.
import { linkHorizontal } from 'd3-shape'
import { useMemo } from 'react'
import { shortOf, type Moved } from '@/api/civic'
import { ChartTip, TipTitle, useMeasure, useTip } from '@/components/charts/chartKit'
import type { Theme } from '@/lib/hooks'
import { ministryColor } from './shared'

export type Pair = { id: string; from: { key: string; title: string }; to: { key: string; title: string }; items: Moved[] }

export function pairsOf(moved: Moved[]): Pair[] {
  const m = new Map<string, Pair>()
  for (const x of moved) {
    const id = `${x.from.portfolio_key}>${x.to.portfolio_key}`
    const p = m.get(id) ?? { id, from: { key: x.from.portfolio_key, title: shortOf(x.from) }, to: { key: x.to.portfolio_key, title: shortOf(x.to) }, items: [] }
    p.items.push(x)
    m.set(id, p)
  }
  return [...m.values()]
}

const MAX_UNIT = 15
const MIN_UNIT = 2
/** the tallest column is scaled to about this many pixels, so 170 moves and 8 moves both fit a screen */
const TARGET = 420
/** a label needs room for two lines, so a node with one or two institutions still gets a slot that tall */
const SLOT_MIN = 30
const GAP = 6
const NODE_W = 12
const MIN_W = 640
const PAD = 8

/** Selection: a ribbon (its pair id) or a whole ministry (`L:key` for the left, `R:key` for the right). */
export const nodeSel = (side: 'L' | 'R', key: string) => `${side}:${key}`
export const matchesSel = (p: Pair, sel: string | null) => !!sel && (sel === p.id || sel === nodeSel('L', p.from.key) || sel === nodeSel('R', p.to.key))

type Node = { key: string; title: string; count: number; y: number; h: number; slotY: number; slotH: number }

export function FlowDiagram({ pairs, selected, onSelect, theme }: { pairs: Pair[]; selected: string | null; onSelect: (id: string | null) => void; theme: Theme }) {
  const [ref, boxW] = useMeasure<HTMLDivElement>(MIN_W)
  const { tip, show, hide } = useTip()
  const w = Math.max(boxW, MIN_W)
  const labelW = Math.min(250, Math.round(w * 0.3))

  const layout = useMemo(() => {
    const sideTotal = (pick: (p: Pair) => string) => {
      const m = new Map<string, number>()
      for (const p of pairs) m.set(pick(p), (m.get(pick(p)) ?? 0) + p.items.length)
      return [...m.values()]
    }
    const nodesL = sideTotal((p) => p.from.key)
    const nodesR = sideTotal((p) => p.to.key)
    const widest = Math.max(nodesL.reduce((a, b) => a + b, 0), nodesR.reduce((a, b) => a + b, 0), 1)
    const UNIT = Math.min(MAX_UNIT, Math.max(MIN_UNIT, TARGET / widest))
    const stack = (nodes: { key: string; title: string; count: number }[]): Node[] => {
      let y = PAD
      return nodes.map((n) => {
        const h = n.count * UNIT
        const slotH = Math.max(h, SLOT_MIN)
        const out = { ...n, y: y + (slotH - h) / 2, h, slotY: y, slotH }
        y += slotH + GAP
        return out
      })
    }
    const tally = (pick: (p: Pair) => { key: string; title: string }) => {
      const m = new Map<string, { key: string; title: string; count: number }>()
      for (const p of pairs) {
        const n = pick(p)
        const cur = m.get(n.key) ?? { ...n, count: 0 }
        cur.count += p.items.length
        m.set(n.key, cur)
      }
      return [...m.values()]
    }
    const left = stack(tally((p) => p.from).sort((a, b) => b.count - a.count))
    const leftCenter = new Map(left.map((n) => [n.key, n.y + n.h / 2]))
    const rightRaw = tally((p) => p.to)
    const bary = (key: string) => {
      const ps = pairs.filter((p) => p.to.key === key)
      return ps.reduce((s, p) => s + (leftCenter.get(p.from.key) ?? 0) * p.items.length, 0) / (ps.reduce((s, p) => s + p.items.length, 0) || 1)
    }
    const right = stack(rightRaw.sort((a, b) => bary(a.key) - bary(b.key)))
    const li = new Map(left.map((n, i) => [n.key, i]))
    const ri = new Map(right.map((n, i) => [n.key, i]))
    const lOff = new Map<string, number>()
    const rOff = new Map<string, number>()
    const ribbons = [...pairs]
      .sort((a, b) => (li.get(a.from.key)! - li.get(b.from.key)!) || (ri.get(a.to.key)! - ri.get(b.to.key)!))
      .map((p) => ({ p, ly: 0, ry: 0, wd: p.items.length * UNIT }))
    for (const r of ribbons) {
      const ln = left[li.get(r.p.from.key)!]
      r.ly = ln.y + (lOff.get(ln.key) ?? 0) + r.wd / 2
      lOff.set(ln.key, (lOff.get(ln.key) ?? 0) + r.wd)
    }
    for (const r of [...ribbons].sort((a, b) => (ri.get(a.p.to.key)! - ri.get(b.p.to.key)!) || (li.get(a.p.from.key)! - li.get(b.p.from.key)!))) {
      const rn = right[ri.get(r.p.to.key)!]
      r.ry = rn.y + (rOff.get(rn.key) ?? 0) + r.wd / 2
      rOff.set(rn.key, (rOff.get(rn.key) ?? 0) + r.wd)
    }
    const end = (ns: Node[]) => (ns.length ? ns[ns.length - 1].slotY + ns[ns.length - 1].slotH : 0)
    const height = Math.max(end(left), end(right)) + PAD
    return { left, right, ribbons, height, unit: UNIT }
  }, [pairs])

  const xl = labelW
  const xr = w - labelW - NODE_W
  const link = linkHorizontal<{ s: [number, number]; t: [number, number] }, [number, number]>().source((d) => d.s).target((d) => d.t)

  const describe = (p: Pair) => `${p.items.length} ${p.items.length === 1 ? 'institution' : 'institutions'} from ${p.from.title} to ${p.to.title}`

  return (
    <div className="scroll-thin overflow-x-auto rounded-2xl border border-line bg-bg2 p-3 sm:p-4" tabIndex={-1}>
      <div ref={ref} style={{ width: w, minWidth: '100%' }} className="relative" data-chart>
        <svg width={w} height={layout.height} viewBox={`0 0 ${w} ${layout.height}`} className="block" role="group" aria-label="Flow of institutions between ministries" onPointerLeave={hide} onClick={(e) => e.target === e.currentTarget && onSelect(null)}>
          {layout.left.map((n) => (
            <rect key={n.key} x={xl} y={n.y} width={NODE_W} height={n.h} rx={3} fill="var(--ink2)" />
          ))}
          {layout.right.map((n) => (
            <rect key={n.key} x={xr} y={n.y} width={NODE_W} height={n.h} rx={3} fill="var(--ink2)" />
          ))}
          {layout.ribbons.map(({ p, ly, ry, wd }) => {
            const on = matchesSel(p, selected)
            const dim = selected && !on
            const color = ministryColor(p.from.key, theme)
            const names = p.items.slice(0, 5).map((i) => i.name)
            return (
              <g
                key={p.id}
                role="button"
                tabIndex={0}
                aria-pressed={selected === p.id}
                aria-label={describe(p)}
                className="cursor-pointer outline-none [&:focus-visible_path.v]:stroke-ink [&:focus-visible_path.v]:stroke-[2px]"
                onClick={(e) => (e.stopPropagation(), onSelect(selected === p.id ? null : p.id))}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') (e.preventDefault(), onSelect(selected === p.id ? null : p.id))
                }}
                onPointerMove={(e) =>
                  show(
                    e,
                    <div>
                      <TipTitle>
                        {p.from.title} → {p.to.title}
                      </TipTitle>
                      <div className="text-ink2">{names.join(', ')}{p.items.length > names.length ? `, and ${p.items.length - names.length} more` : ''}</div>
                    </div>,
                  )
                }
              >
                <path d={link({ s: [xl + NODE_W, ly], t: [xr, ry] }) ?? ''} fill="none" stroke="transparent" strokeWidth={Math.max(wd, 9)} pointerEvents="stroke" />
                <path d={link({ s: [xl + NODE_W, ly], t: [xr, ry] }) ?? ''} fill="none" stroke={color} strokeWidth={Math.max(wd, 1.5)} strokeOpacity={dim ? 0.1 : on ? 0.85 : theme === 'dark' ? 0.55 : 0.42} className="transition-[stroke-opacity] duration-150" pointerEvents="none" />
                <path className="v" d={link({ s: [xl + NODE_W, ly], t: [xr, ry] }) ?? ''} fill="none" stroke="transparent" strokeWidth={wd + 2} pointerEvents="none" />
                {wd >= 20 && (
                  <text x={(xl + xr + NODE_W) / 2} y={(ly + ry) / 2 + 4} textAnchor="middle" fontSize={12} fontWeight={600} fill="var(--ink)" pointerEvents="none" style={{ paintOrder: 'stroke', stroke: 'var(--bg2)', strokeWidth: 3, fontVariantNumeric: 'tabular-nums' }}>
                    {p.items.length}
                  </text>
                )}
              </g>
            )
          })}
        </svg>
        {layout.left.map((n) => (
          <button
            key={n.key}
            type="button"
            aria-pressed={selected === nodeSel('L', n.key)}
            onClick={() => onSelect(selected === nodeSel('L', n.key) ? null : nodeSel('L', n.key))}
            title={n.title}
            className="absolute flex items-center justify-end rounded-md pr-2 text-right hover:bg-bg3"
            style={{ left: 0, width: labelW, top: n.slotY, height: n.slotH }}
          >
            <span className="line-clamp-2 text-[11.5px] leading-tight font-medium text-ink">
              {n.title} <span className="t-num font-normal text-ink3">{n.count}</span>
            </span>
          </button>
        ))}
        {layout.right.map((n) => (
          <button
            key={n.key}
            type="button"
            aria-pressed={selected === nodeSel('R', n.key)}
            onClick={() => onSelect(selected === nodeSel('R', n.key) ? null : nodeSel('R', n.key))}
            title={n.title}
            className="absolute flex items-center rounded-md pl-2 text-left hover:bg-bg3"
            style={{ left: xr + NODE_W, width: labelW, top: n.slotY, height: n.slotH }}
          >
            <span className="line-clamp-2 text-[11.5px] leading-tight font-medium text-ink">
              {n.title} <span className="t-num font-normal text-ink3">{n.count}</span>
            </span>
          </button>
        ))}
        <div className="pointer-events-none absolute top-0 left-0 w-full" aria-hidden>
          <ChartTip tip={tip} width={w} />
        </div>
      </div>
    </div>
  )
}
