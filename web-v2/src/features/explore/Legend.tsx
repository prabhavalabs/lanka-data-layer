import type { Dataset } from '@/api/types'
import { LicenceLight } from '@/components/common/bits'
import { classOf } from '@/lib/palettes'
import { cn } from '@/lib/utils'
import { fmtBare, isSum, unitSuffix, type MapModel } from './model'

/** Where the national value falls along the 7-class strip, 0..1. */
function nationalAt(model: MapModel): number {
  const { breaks, national } = model.data
  const k = classOf(national, breaks)
  const lo = k === 0 ? Math.min(model.min, national) : breaks[k - 1]
  const hi = k === breaks.length ? Math.max(model.max, national) : breaks[k]
  return Math.min(1, Math.max(0, (k + (national - lo) / (hi - lo || 1)) / (breaks.length + 1)))
}

/** The class strip alone: reused by the desktop card and the mobile summary bar. */
export function LegendStrip({ model, compact = false }: { model: MapModel; compact?: boolean }) {
  const { breaks, national } = model.data
  const unit = model.indicator.unit
  const showNational = !isSum(unit)
  const at = nationalAt(model)
  return (
    <div className={cn(showNational && !compact && 'pt-[18px]')}>
      <div className="relative">
        {showNational && !compact && (
          <div className="absolute bottom-full flex -translate-x-1/2 flex-col items-center" style={{ left: `${at * 100}%` }}>
            <span className={cn('t-num mb-0.5 font-mono text-[10px] leading-none whitespace-nowrap text-ink2', at > 0.86 && '-translate-x-[38%]', at < 0.14 && 'translate-x-[38%]')}>LK {fmtBare(national, unit)}</span>
            <span className="h-[5px] w-px bg-ink" />
          </div>
        )}
        <div className="flex h-2.5 gap-px overflow-hidden rounded-[3px]" role="img" aria-label={`Seven classes from ${fmtBare(model.min, unit)} to ${fmtBare(model.max, unit)} ${unitSuffix(unit)}`}>
          {model.colors.map((c) => (
            <span key={c} className="flex-1" style={{ background: c }} />
          ))}
        </div>
      </div>
      <div className="relative mt-1.5 h-3" aria-hidden>
        {breaks.map((b, i) =>
          compact && i % 2 === 1 ? null : (
            <span key={i} className="t-num absolute -translate-x-1/2 font-mono text-[10px] leading-none text-ink3" style={{ left: `${((i + 1) / (breaks.length + 1)) * 100}%` }}>
              {fmtBare(b, unit)}
            </span>
          ),
        )}
      </div>
    </div>
  )
}

/** `docked` renders it as the pinned footer of the control panel; otherwise it is a floating glass card. */
export function Legend({ model, dataset, loading, docked = false, className }: { model: MapModel | null; dataset: Dataset | undefined; loading: boolean; docked?: boolean; className?: string }) {
  const shell = docked ? 'border-t border-glass-line bg-bg2/30' : 'glass w-[312px] rounded-2xl'
  if (!model) {
    return (
      <div className={cn(shell, 'p-4', className)}>
        <div className="skeleton h-4 w-40" />
        <div className="skeleton mt-4 h-2.5 w-full" />
        <div className="skeleton mt-3 h-3 w-48" />
      </div>
    )
  }
  const { indicator, data } = model
  return (
    <div className={cn(shell, className)} role="group" aria-label="Map legend">
      <div className="px-4 pt-3.5 pb-3">
        <div className="flex items-baseline justify-between gap-3">
          <h2 className="truncate text-[13.5px] font-semibold tracking-[-0.01em] text-ink">{indicator.label}</h2>
          <span className={cn('t-label t-num shrink-0 transition-opacity duration-150', loading && 'opacity-50')}>
            {unitSuffix(indicator.unit)} · {data.year}
          </span>
        </div>
        <div className="mt-3">
          <LegendStrip model={model} />
        </div>
        <div className="mt-2.5 flex items-center gap-4 text-[11.5px] text-ink2">
          <span className="inline-flex items-center gap-1.5">
            <span className="explore-hatch size-2.5 rounded-[3px] ring-1 ring-line2" />
            No data
          </span>
          {isSum(indicator.unit) && <span className="t-num ml-auto font-mono text-[10.5px] text-ink3">LK total {fmtBare(data.national, indicator.unit)}</span>}
        </div>
      </div>
      <div className="flex items-center gap-2 border-t border-glass-line px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[11.5px] text-ink2">{dataset?.publisher ?? data.meta.source[0]?.publisher}</div>
          <div className="t-num mt-0.5 font-mono text-[10px] text-ink3">data_version {data.meta.data_version}</div>
        </div>
        {dataset && <LicenceLight status={dataset.licence.status} label={dataset.licence.label} className="shrink-0" />}
      </div>
    </div>
  )
}
