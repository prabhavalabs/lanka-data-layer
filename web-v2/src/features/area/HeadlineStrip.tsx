import { FileSearch } from 'lucide-react'
import { useState } from 'react'
import type { AreaProfile, Headline } from '@/api/types'
import { Sparkline } from '@/components/charts/Sparkline'
import { isSample } from '@/api/types'
import { SampleMark } from '@/components/common/bits'
import { ProvenanceDrawer } from '@/components/common/ProvenanceDrawer'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { fmtInt, fmtSigned } from '@/lib/format'
import { cn } from '@/lib/utils'

/** Status is carried by shape and a word — solid = final, dashed ring = provisional. */
export function StatusMark({ status }: { status: Headline['status'] }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span aria-hidden className={cn('size-[7px] rounded-full', status === 'final' ? 'bg-ink2' : 'border border-dashed border-ink2')} />
      {status}
    </span>
  )
}

export function HeadlineStrip({ profile }: { profile: AreaProfile }) {
  const [obs, setObs] = useState<{ id: string; title: string } | null>(null)
  return (
    <>
      <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[14px] border border-line bg-line shadow-[var(--shadow-sm)] lg:grid-cols-4">
        {profile.headline.map((h) => (
          <div key={h.id} className="relative flex min-w-0 flex-col bg-bg2 p-4 sm:p-5">
            <dt className="flex items-center justify-between gap-2">
              <span className="t-label">{h.label}</span>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    onClick={() => setObs({ id: h.observationId, title: `${h.label} · ${profile.area.name.en}` })}
                    aria-label={`Show the source of ${h.label.toLowerCase()}`}
                    className="pressable -m-1.5 grid size-8 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink"
                  >
                    <FileSearch className="size-[15px]" />
                  </button>
                </TooltipTrigger>
                <TooltipContent>Source and provenance</TooltipContent>
              </Tooltip>
            </dt>
            <dd className="mt-3 flex flex-wrap items-baseline gap-x-2">
              <span className="t-num text-[clamp(1.45rem,0.9rem+1.9vw,2.15rem)] leading-none font-semibold tracking-[-0.035em] text-ink">{fmtInt(h.value)}</span>
              <span className="text-[12.5px] text-ink2">{h.unit}</span>
            </dd>
            <dd className="mt-auto flex flex-wrap items-end justify-between gap-x-3 gap-y-2 pt-4">
              <span className="flex flex-col gap-1.5">
                {h.delta && (
                  <span className="t-num inline-flex w-fit items-center gap-1 rounded-md border border-line2 bg-bg3 px-1.5 py-0.5 text-[11.5px] font-medium text-ink">
                    <span aria-hidden className="text-[9px]">
                      {h.delta.value >= 0 ? '▲' : '▼'}
                    </span>
                    {fmtSigned(h.delta.value)}%<span className="font-normal text-ink3">since {h.delta.since}</span>
                  </span>
                )}
                <span className="t-num flex items-center gap-2 font-mono text-[11px] text-ink3">
                  {h.year}
                  <span aria-hidden>·</span>
                  <StatusMark status={h.status} />
                  {h.basis && (
                    <>
                      <span aria-hidden>·</span>
                      <span title={h.basis.hint}>{h.basis.label}</span>
                    </>
                  )}
                </span>
              </span>
              {h.id === 'pop.total' && (
                <span className="flex flex-col items-end gap-1 max-sm:hidden">
                  <Sparkline data={profile.popSeries.map((p) => p.value)} width={76} height={26} />
                  {isSample(profile.sample, 'popSeries') && <SampleMark />}
                </span>
              )}
            </dd>
          </div>
        ))}
      </dl>
      <ProvenanceDrawer observationId={obs?.id ?? null} onClose={() => setObs(null)} title={obs?.title} />
    </>
  )
}
