import { ChevronDown } from 'lucide-react'
import { useState } from 'react'
import { allLayers, isGroup, type Layer, type LayerNode } from '@/lib/map-spec'
import { cn } from '@/lib/utils'
import { shownLayers, useWorkspace } from './context'
import type { LegendModel } from './engine/compile'
import { fmt, unitSuffix } from './format'
import { rampCss } from './inspector/controls'
import { useWs } from './store'

function Swatch({ item }: { item: Extract<LegendModel, { kind: 'categories' }>['items'][number] }) {
  if (item.shape === 'line' || item.shape === 'dash')
    return <span className="h-0 w-4 shrink-0 border-t-2" style={{ borderColor: item.color, borderTopStyle: item.shape === 'dash' ? 'dashed' : 'solid' }} />
  if (item.shape === 'ring') return <span className="size-2.5 shrink-0 rounded-[3px] border-2" style={{ borderColor: item.color }} />
  return <span className={cn('size-2.5 shrink-0', item.shape === 'dot' ? 'rounded-full' : 'rounded-[3px]')} style={{ background: item.color }} />
}

/** One layer's key. The same model is drawn into the PNG export. */
export function LegendBody({ model }: { model: LegendModel }) {
  switch (model.kind) {
    case 'classes': {
      const n = model.colors.length
      // too many break labels collide: keep every other one past six classes
      const skip = n > 6 ? 2 : 1
      return (
        <div>
          <div className="flex h-2.5 gap-px overflow-hidden rounded-[3px]" role="img" aria-label={`${n} classes, breaks at ${model.breaks.map((b) => fmt(b, model.unit)).join(', ')} ${unitSuffix(model.unit)}`}>
            {model.colors.map((c, i) => (
              <span key={i} className="flex-1" style={{ background: c }} />
            ))}
          </div>
          <div className="relative mt-1.5 h-3" aria-hidden>
            {model.breaks.map((b, i) =>
              i % skip ? null : (
                <span key={i} className="t-num absolute -translate-x-1/2 font-mono text-[10px] leading-none text-ink3" style={{ left: `${((i + 1) / n) * 100}%` }}>
                  {fmt(b, model.unit)}
                </span>
              ),
            )}
          </div>
          {model.hatch && (
            <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-ink3">
              <span className="ws-hatch size-2.5 rounded-[3px] ring-1 ring-line2" />
              modelled, fewer than three listings
            </div>
          )}
        </div>
      )
    }
    case 'ramp':
      return (
        <div>
          <div className="h-2.5 rounded-[3px]" style={{ background: rampCss(model.stops) }} role="img" aria-label={`From ${fmt(model.domain[0], model.unit)} to ${fmt(model.domain[1], model.unit)} ${unitSuffix(model.unit)}`} />
          <div className="t-num mt-1.5 flex justify-between font-mono text-[10px] leading-none text-ink3" aria-hidden>
            <span>{fmt(model.domain[0], model.unit)}</span>
            <span>{fmt(model.domain[1], model.unit)}+</span>
          </div>
        </div>
      )
    case 'heat':
      return (
        <div>
          <div className="h-2.5 rounded-[3px]" style={{ background: rampCss(['transparent', ...model.stops]) }} />
          <div className="mt-1.5 flex justify-between font-mono text-[10px] leading-none text-ink3">
            <span>sparse</span>
            <span>dense</span>
          </div>
        </div>
      )
    case 'categories':
      return (
        <ul className={cn('grid gap-x-3 gap-y-1', model.items.length > 2 ? 'grid-cols-2' : 'grid-cols-1')}>
          {model.items.map((it) => (
            <li key={it.label} className="flex min-w-0 items-center gap-1.5 text-[11.5px] text-ink2">
              <Swatch item={it} />
              <span className="truncate first-letter:uppercase">{it.label}</span>
            </li>
          ))}
        </ul>
      )
    case 'bivariate':
      return (
        <div className="flex items-center gap-3.5 py-0.5">
          <div className="relative size-[54px] shrink-0">
            <div className="grid size-full grid-cols-3 grid-rows-3 gap-px overflow-hidden rounded-[4px]" role="img" aria-label={`Three by three colour key: ${model.x} across, ${model.y} up`}>
              {[6, 7, 8, 3, 4, 5, 0, 1, 2].map((i) => (
                <span key={i} style={{ background: model.colors[i] }} />
              ))}
            </div>
          </div>
          <div className="min-w-0 flex-1 space-y-1.5 text-[11.5px] leading-tight text-ink2">
            <div className="flex items-center gap-1.5">
              <span className="font-mono text-[11px] text-ink3">↑</span>
              <span className="truncate">{model.y}</span>
            </div>
            <div className="flex items-center gap-1.5">
              <span className="font-mono text-[11px] text-ink3">→</span>
              <span className="truncate">{model.x}</span>
            </div>
            <div className="font-mono text-[10px] text-ink3">low · middle · high thirds</div>
          </div>
        </div>
      )
    default:
      return null
  }
}

export function legendEntries(nodes: LayerNode[], shown: Set<string>): Layer[] {
  // top of the stack first, as the panel lists them
  const out: Layer[] = []
  const visit = (list: LayerNode[]) => {
    for (let i = list.length - 1; i >= 0; i--) {
      const n = list[i]
      if (isGroup(n)) visit(n.children)
      else if (shown.has(n.id) && n.legend.show) out.push(n)
    }
  }
  visit(nodes)
  return out
}

export function LegendList({ className }: { className?: string }) {
  const layers = useWs((s) => s.doc!.layers)
  const solo = useWs((s) => s.solo)
  const { engine, tick } = useWorkspace()
  void tick
  const entries = legendEntries(layers, shownLayers(layers, solo)).flatMap((l) => {
    const model = engine.current?.legend(l.id)
    return model && model.kind !== 'relief' ? [{ layer: l, model }] : []
  })
  if (entries.length === 0) return <p className={cn('px-3.5 py-3 text-[12px] text-ink3', className)}>Nothing on the map has a key.</p>
  return (
    <ul className={className}>
      {entries.map(({ layer, model }) => {
        const unit = 'unit' in model ? unitSuffix(model.unit) : ''
        return (
          <li key={layer.id} className="border-t border-glass-line px-3.5 pt-2.5 pb-3 first:border-t-0">
            <div className="mb-2 flex items-baseline gap-2">
              <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium tracking-[-0.005em] text-ink">{layer.legend.title ?? layer.name}</span>
              {(unit || model.period) && <span className="t-label t-num shrink-0">{[unit, model.period].filter(Boolean).join(' · ')}</span>}
            </div>
            <LegendBody model={model} />
            {model.note && <div className="mt-1.5 font-mono text-[10px] text-ink3">{model.note}</div>}
          </li>
        )
      })}
    </ul>
  )
}

/** The map's key: one entry per visible layer that has something to explain, in stack order. */
export function LegendCard({ className, style }: { className?: string; style?: React.CSSProperties }) {
  const [open, setOpen] = useState(true)
  const count = useWs((s) => allLayers(s.doc!.layers).filter((l) => l.legend.show && l.visible).length)
  return (
    <div className={cn('glass flex max-h-[min(46vh,420px)] w-[264px] flex-col overflow-hidden rounded-2xl', className)} style={style} role="group" aria-label="Map legend">
      <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open} className="flex h-9 shrink-0 items-center gap-2 px-3.5 text-left outline-none">
        <span className="t-label tracking-[0.08em] uppercase">Legend</span>
        {!open && <span className="t-label t-num text-ink2">{count}</span>}
        <ChevronDown className={cn('ml-auto size-3.5 text-ink3 transition-transform duration-200 ease-[var(--ease-out)]', !open && 'rotate-180')} />
      </button>
      {open && <LegendList className="scroll-thin min-h-0 overflow-y-auto border-t border-glass-line" />}
    </div>
  )
}
