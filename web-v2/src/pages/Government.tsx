import { useQuery } from '@tanstack/react-query'
import { GitCompareArrows, Landmark } from 'lucide-react'
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { diffQuery, earliestOf, instrumentsQuery, isApplied, structureQuery } from '@/api/civic'
import { ApiError } from '@/api/http'
import { SampleMark } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { AmendmentDrawer } from '@/features/government/AmendmentDrawer'
import { CompareControls, CompareView } from '@/features/government/Compare'
import { HistoryDrawer } from '@/features/government/HistoryDrawer'
import { fmtDay, isDay, isoDay, StateBox } from '@/features/government/shared'
import { StructureView } from '@/features/government/Structure'
import { Timeline } from '@/features/government/Timeline'

const today = isoDay(new Date())

function Skeleton({ h }: { h: string }) {
  return (
    <div className="flex flex-col gap-3" aria-busy aria-label="Loading">
      <div className="skeleton h-6 w-[min(520px,90%)]" />
      <div className="skeleton h-4 w-[min(320px,70%)]" />
      <div className={`skeleton ${h} w-full`} />
    </div>
  )
}

function Failure({ error, retry }: { error: unknown; retry: () => void }) {
  return (
    <StateBox title="The record could not be loaded" tone="bad">
      <p>{error instanceof Error ? error.message : 'Something went wrong.'}</p>
      <Button variant="outline" size="sm" className="mt-3" onClick={retry}>
        Try again
      </Button>
    </StateBox>
  )
}

export default function GovernmentPage() {
  const [params, setParams] = useSearchParams()
  const update = (patch: Record<string, string | null>) => {
    const next = new URLSearchParams(params)
    for (const [k, v] of Object.entries(patch)) {
      if (v === null) next.delete(k)
      else next.set(k, v)
    }
    setParams(next, { replace: true })
  }
  const rawAt = params.get('at')
  const at = isDay(rawAt) ? (rawAt > today ? today : rawAt) : today
  const compare = params.get('view') === 'compare'
  const entity = params.get('e')
  const amendment = params.get('am')

  const instruments = useQuery(instrumentsQuery())
  const list = instruments.data?.data
  // the dates a comparison can start or end on: every gazette that carries a schedule (or an applied amendment), newest first
  const schedules = useMemo(() => (list ?? []).filter((i) => (i.kind !== 'amendment' && i.portfolios > 0) || isApplied(i)).sort((a, b) => b.effective_from.localeCompare(a.effective_from)), [list])
  const baselines = useMemo(() => schedules.filter((i) => i.kind === 'baseline'), [schedules])

  // the comparison defaults to the baseline in force at `at` against the one before it
  const defTo = baselines.find((b) => b.effective_from <= at) ?? baselines[0]
  const defFrom = defTo ? baselines[baselines.indexOf(defTo) + 1] : undefined
  const rawFrom = params.get('from')
  const rawTo = params.get('to')
  const to = isDay(rawTo) ? rawTo : (defTo?.effective_from ?? '')
  const from = isDay(rawFrom) ? rawFrom : (defFrom?.effective_from ?? '')

  const structure = useQuery({ ...structureQuery(at), enabled: !compare })
  const diff = useQuery({ ...diffQuery(from, to), enabled: compare && !!from && !!to })

  const sample = !!(instruments.data?.sample || structure.data?.sample || diff.data?.sample)
  const first = earliestOf(structure.error) ?? earliestOf(diff.error) ?? schedules[schedules.length - 1]?.effective_from
  const noSchedule = structure.error instanceof ApiError && structure.error.status === 404
  const open = (key: string) => update({ e: key })

  return (
    <article className="mx-auto max-w-[1240px] px-5 pt-28 pb-24 sm:px-8 sm:pt-32">
      <header>
        <p className="t-label inline-flex items-center gap-2">
          <Landmark className="size-3.5" aria-hidden /> Civic record
          {sample && <SampleMark title="The live API does not serve this yet: these are bundled sample values, based on the real gazettes" />}
        </p>
        <h1 className="t-h1 mt-3 max-w-[18ch]">Government</h1>
        <p className="t-lead mt-4 max-w-[64ch]">
          Who is responsible for what. This page lists every ministry, and the departments, statutory institutions and corporations in each minister’s charge, as the President’s gazettes set them out. Pick a date to see the structure then, or compare two dates to see what changed.
        </p>
      </header>

      <section className="mt-8" aria-label="Timeline of gazettes">
        {instruments.isPending ? (
          <div className="skeleton h-[150px] w-full rounded-2xl" aria-busy aria-label="Loading the timeline" />
        ) : instruments.isError ? (
          <Failure error={instruments.error} retry={() => void instruments.refetch()} />
        ) : list && list.length ? (
          <Timeline instruments={list} at={at} today={today} onChange={(d) => update({ at: d })} />
        ) : (
          <StateBox title="No gazettes in the record yet">Nothing has been read from the gazettes. Check again later.</StateBox>
        )}
      </section>

      <div className="mt-10 flex flex-wrap items-end justify-between gap-3">
        <ToggleGroup type="single" value={compare ? 'compare' : 'structure'} onValueChange={(v) => v && update({ view: v === 'compare' ? 'compare' : null })} aria-label="View" className="h-10">
          <ToggleGroupItem value="structure" className="px-4 text-[13px]">
            <Landmark /> Structure
          </ToggleGroupItem>
          <ToggleGroupItem value="compare" className="px-4 text-[13px]">
            <GitCompareArrows /> Compare
          </ToggleGroupItem>
        </ToggleGroup>
        {compare && schedules.length > 1 && <CompareControls baselines={schedules} from={from} to={to} onChange={(f, t) => update({ from: f, to: t })} />}
      </div>

      <div className="mt-6 min-h-[320px]">
        {instruments.isPending ? (
          <Skeleton h="h-72" />
        ) : compare ? (
          !from || !to ? (
            <StateBox title="Two schedules are needed to compare">Only one gazette has been read so far, so there is nothing to compare it with yet.</StateBox>
          ) : diff.isPending ? (
            <Skeleton h="h-80" />
          ) : diff.isError ? (
            diff.error instanceof ApiError && diff.error.status === 404 ? (
              <StateBox title="No schedule in force on one of these dates">The record starts on {first ? fmtDay(first) : 'the first parsed baseline'}. Choose dates on or after it.</StateBox>
            ) : (
              <Failure error={diff.error} retry={() => void diff.refetch()} />
            )
          ) : (
            <CompareView diff={diff.data.data} sample={diff.data.sample} onOpen={open} />
          )
        ) : structure.isPending ? (
          <Skeleton h="h-96" />
        ) : structure.isError ? (
          noSchedule ? (
            <StateBox title={`No schedule read before ${first ? fmtDay(first) : 'the first gazette'}`}>
              Nothing in the record says who was responsible for what on {fmtDay(at)}.
              {first && (
                <div className="mt-3">
                  <Button variant="outline" size="sm" onClick={() => update({ at: first })}>
                    Go to {fmtDay(first)}
                  </Button>
                </div>
              )}
            </StateBox>
          ) : (
            <Failure error={structure.error} retry={() => void structure.refetch()} />
          )
        ) : structure.data.data.portfolios.length === 0 ? (
          <StateBox title="This gazette lists no ministries">Its schedule has not been read into the record.</StateBox>
        ) : (
          <StructureView s={structure.data.data} sample={structure.data.sample} at={at} onOpen={open} onAmendment={(d) => update({ am: d })} />
        )}
      </div>

      <section className="mt-16 border-t border-line pt-8" aria-labelledby="how-h">
        <h2 id="how-h" className="t-h3">
          How this is built
        </h2>
        <div className="mt-3 grid max-w-[78ch] gap-3 text-[14px] leading-relaxed text-ink2">
          <p>
            Under Article 44 of the Constitution the President sets out, by extraordinary gazette, each minister’s subjects and functions and the departments, statutory institutions and public corporations in their charge. A <strong className="font-semibold text-ink">baseline</strong> gazette lists the whole schedule; an <strong className="font-semibold text-ink">amendment</strong> changes a named baseline from a stated date.
          </p>
          <p>
            We read the schedules from the archived gazette PDFs, check them (ministries and items numbered without gaps, the count matching the text) and keep only those that pass. Amendments are read change by change (an item added, removed, reworded or renumbered, a ministry renamed, added or removed) and applied in date order to the structure in force. A change is applied only when the item or ministry it names is there, and when its printed number and text agree; otherwise the gazette waits for a person to check it, and the page says so. Every institution links to the gazette page it is printed on, which after an amendment can be the amendment itself.
          </p>
        </div>
      </section>

      <HistoryDrawer entityKey={entity} onClose={() => update({ e: null })} today={today} />
      <AmendmentDrawer docId={amendment} onClose={() => update({ am: null })} onCompare={(f, t) => update({ am: null, view: 'compare', from: f, to: t })} />
    </article>
  )
}
