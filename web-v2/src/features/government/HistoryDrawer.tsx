// One institution's history: a bar for each assignment to a ministry, on a shared time axis, coloured by ministry.
import { useQuery } from '@tanstack/react-query'
import { scaleTime } from 'd3-scale'
import { entityQuery, gazetteNumber, KIND_LABEL, plain } from '@/api/civic'
import { SampleMark } from '@/components/common/bits'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { useTheme } from '@/lib/hooks'
import { Evidence, fmtDay, KindDot, ministryColor, parseDay, StateBox } from './shared'

export function HistoryDrawer({ entityKey, onClose, today }: { entityKey: string | null; onClose: () => void; today: string }) {
  const [theme] = useTheme()
  const q = useQuery(entityQuery(entityKey))
  const e = q.data?.data
  const rows = e ? [...e.history].sort((a, b) => a.valid_from.localeCompare(b.valid_from)) : []
  const t0 = rows.length ? parseDay(rows[0].valid_from) : parseDay(today)
  const t1 = parseDay(today)
  const span = Math.max(+t1 - +t0, 1)
  const x = scaleTime().domain([t0, t1]).range([0, 100])
  const pct = (iso: string | null) => x(iso ? parseDay(iso) : t1)
  const ticks = x.ticks(4)
  const current = rows.at(-1)
  return (
    <Sheet open={!!entityKey} onOpenChange={(o) => !o && onClose()}>
      <SheetContent aria-describedby="hist-desc" className="overflow-y-auto">
        <div className="px-5 pt-5 pr-14">
          <p className="t-label">Institution history</p>
          <SheetTitle className="mt-1.5 text-[20px] leading-tight font-semibold tracking-[-0.02em] text-ink">{e?.name ?? (q.isError ? 'Not found' : 'Loading')}</SheetTitle>
          <SheetDescription id="hist-desc" className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-ink2">
            {e && (
              <>
                <KindDot kind={e.kind} theme={theme} /> {KIND_LABEL[e.kind]}
                {q.data?.sample && <SampleMark />}
              </>
            )}
            {!e && !q.isError && 'Which ministry it has been in the charge of, and when.'}
          </SheetDescription>
        </div>
        <div className="px-5 pt-5 pb-8">
          {q.isPending && entityKey && (
            <div className="flex flex-col gap-4" aria-busy>
              <div className="skeleton h-4 w-2/3" />
              <div className="skeleton h-3 w-full" />
              <div className="skeleton h-4 w-1/2" />
              <div className="skeleton h-3 w-full" />
            </div>
          )}
          {q.isError && <StateBox title="No history for this institution" tone="bad">It is not in any gazette the record has read.</StateBox>}
          {e && (
            <>
              <p className="text-[14px] text-ink2">
                {current?.valid_to === null ? (
                  <>
                    In the charge of the <strong className="font-semibold text-ink">{plain(current.portfolio_title)}</strong> since {fmtDay(current.valid_from)}.
                  </>
                ) : (
                  <>Last listed under {plain(current?.portfolio_title ?? '')} until {fmtDay(current?.valid_to)}.</>
                )}
              </p>
              <ol className="mt-5 flex flex-col gap-5">
                {rows.map((r) => {
                  const left = pct(r.valid_from)
                  const width = Math.max(pct(r.valid_to) - left, 1.2)
                  const color = ministryColor(r.portfolio_key, theme)
                  return (
                    <li key={r.valid_from}>
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0 text-[13.5px] leading-snug">
                          <span className="inline-block size-2.5 rounded-[3px] align-[-1px]" style={{ background: color }} aria-hidden /> <span className="font-medium text-ink">{plain(r.portfolio_title)}</span>
                          <div className="text-[12px] text-ink3">
                            {fmtDay(r.valid_from)} to {r.valid_to ? fmtDay(r.valid_to) : 'now'}
                          </div>
                        </div>
                        <Evidence docId={r.doc_id} page={r.page} label={`${gazetteNumber(r.doc_id)} p.${r.page ?? '?'}`} />
                      </div>
                      <div className="relative mt-2 h-3.5 rounded-full bg-bg3" role="img" aria-label={`${r.portfolio_title}, ${fmtDay(r.valid_from)} to ${r.valid_to ? fmtDay(r.valid_to) : 'now'}`}>
                        <span className="absolute top-0 h-full rounded-full" style={{ left: `${left}%`, width: `${Math.min(width, 100 - left)}%`, background: color, minWidth: 6 }} />
                      </div>
                    </li>
                  )
                })}
              </ol>
              <div className="relative mt-3 h-4 border-t border-line2 text-[10.5px] text-ink3" aria-hidden>
                {ticks.map((t) => (
                  <span key={+t} className="t-num absolute top-1 -translate-x-1/2" style={{ left: `${x(t)}%` }}>
                    {t.getFullYear()}
                    {span < 3 * 365 * 86_400_000 ? `-${String(t.getMonth() + 1).padStart(2, '0')}` : ''}
                  </span>
                ))}
              </div>
              {e.aliases.length > 0 && (
                <p className="mt-6 text-[12.5px] text-ink2">
                  Also written as: {e.aliases.join('; ')}
                </p>
              )}
              <p className="mt-6 text-[12px] leading-relaxed text-ink3">Each bar runs from the gazette that placed the institution under the ministry to the one that moved or dropped it. Links open the gazette at the page.</p>
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}
