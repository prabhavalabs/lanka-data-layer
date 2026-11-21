import type { Area } from '@/api/types'
import { cn } from '@/lib/utils'
import { fmtBare, unitSuffix, type MapModel } from './model'

/** Sorted bars per area. Click a row to select it and fly there. */
export function RankedList({
  model,
  areas,
  selected,
  bounds,
  onPick,
}: {
  model: MapModel | null
  areas: Map<string, Area> | undefined
  selected: string | null
  bounds: [number, number]
  onPick: (pcode: string) => void
}) {
  if (!model) {
    return (
      <div className="space-y-2.5 px-4 py-4">
        {Array.from({ length: 12 }, (_, i) => (
          <div key={i} className="skeleton h-7" style={{ width: `${96 - i * 5}%` }} />
        ))}
      </div>
    )
  }
  const unit = model.indicator.unit
  const rows = model.data.pcodes.map((pcode, i) => ({ pcode, value: model.data.values[i] })).sort((a, b) => b.value - a.value)
  const max = model.max || 1
  return (
    <div className="px-2 py-2.5">
      <div className="flex items-center justify-between px-2 pb-2">
        <span className="t-label tracking-[0.08em] uppercase">Highest first</span>
        <span className="t-label t-num">
          {unitSuffix(unit)} · {model.data.year}
        </span>
      </div>
      <ol>
        {rows.map((r, i) => {
          const on = r.pcode === selected
          const cls = model.byPcode.get(r.pcode)
          const inRange = r.value >= bounds[0] && r.value <= bounds[1]
          return (
            <li key={r.pcode}>
              <button
                type="button"
                onClick={() => onPick(r.pcode)}
                aria-pressed={on}
                className={cn(
                  'pressable relative grid w-full grid-cols-[22px_1fr_auto] items-center gap-x-2 rounded-lg px-2 py-[7px] text-left max-md:py-2.5 [@media(hover:hover)_and_(pointer:fine)]:hover:bg-bg3/70',
                  on && 'bg-brand-soft [@media(hover:hover)_and_(pointer:fine)]:hover:bg-brand-soft',
                  !inRange && 'opacity-40',
                )}
              >
                <span className={cn('t-num font-mono text-[10.5px]', on ? 'text-brand-ink' : 'text-ink3')}>{String(i + 1).padStart(2, '0')}</span>
                <span className="min-w-0">
                  <span className="block truncate text-[13px] leading-tight text-ink">{areas?.get(r.pcode)?.name.en ?? r.pcode}</span>
                  <span className="mt-1.5 block h-[3px] rounded-full bg-line2/70">
                    <span className="block h-full origin-left rounded-full" style={{ width: `${Math.max(1.5, (r.value / max) * 100)}%`, background: cls?.color }} />
                  </span>
                </span>
                <span className="t-num font-mono text-[12px] text-ink">{fmtBare(r.value, unit)}</span>
              </button>
            </li>
          )
        })}
      </ol>
    </div>
  )
}
