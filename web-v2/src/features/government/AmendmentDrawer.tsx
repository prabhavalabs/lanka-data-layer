// What one amending gazette changed, change by change, in plain words: grouped by ministry, each with the page it is
// printed on and whether it was applied. Amendments that are not applied yet say why.
import { useQuery } from '@tanstack/react-query'
import { ArrowLeftRight, CircleAlert, CircleCheck, CircleDashed, GitCompareArrows, Languages } from 'lucide-react'
import { amendmentChangesQuery, gazetteNumber, plain, type AmendmentChange } from '@/api/civic'
import { SampleMark } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { cn } from '@/lib/utils'
import { Evidence, fmtDay, StateBox } from './shared'

const OP: Record<AmendmentChange['op'], string> = {
  omit: 'Removed',
  insert: 'Added',
  substitute: 'Reworded',
  renumber: 'Renumbered',
  replace_column: 'Column replaced',
  replace_columns: 'All columns replaced',
  rename_heading: 'Ministry renamed',
  replace_heading: 'Ministry replaced',
  insert_heading: 'New ministry',
  omit_heading: 'Ministry removed',
  renumber_headings: 'Ministries renumbered',
  unparsed: 'Not read yet',
}
const COL: Record<NonNullable<AmendmentChange['col']>, string> = {
  function: 'Subjects and functions',
  institution: 'Institutions',
  law: 'Laws',
}

type State = 'applied' | 'review' | 'skipped' | 'other' | 'pending'
const stateOf = (c: AmendmentChange): State =>
  c.applied === null ? 'pending' : c.applied && c.ok ? 'applied' : c.applied ? 'review' : c.ok ? 'other' : 'skipped'
const STATE: Record<State, { label: string; cls: string; Icon: typeof CircleCheck }> = {
  applied: { label: 'Applied', cls: 'text-ok', Icon: CircleCheck },
  review: { label: 'Applied, to check', cls: 'text-warn', Icon: CircleAlert },
  skipped: { label: 'Not applied', cls: 'text-bad', Icon: CircleAlert },
  other: { label: 'Sinhala or Tamil text only', cls: 'text-ink3', Icon: Languages },
  pending: { label: 'Not reached', cls: 'text-ink3', Icon: CircleDashed },
}

const dayBefore = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number)
  const t = new Date(Date.UTC(y, m - 1, d) - 86_400_000)
  return t.toISOString().slice(0, 10)
}

export function AmendmentDrawer({ docId, onClose, onCompare }: { docId: string | null; onClose: () => void; onCompare: (from: string, to: string) => void }) {
  const q = useQuery(amendmentChangesQuery(docId))
  const a = q.data?.data
  const served = a && (a.status === 'validated' || a.status === 'approved') && a.changes.some((c) => c.applied)
  const groups: [string, AmendmentChange[]][] = []
  for (const c of a?.changes ?? []) {
    const key = c.heading_title ? plain(c.heading_title) : 'Whole schedule'
    const g = groups.find(([k]) => k === key)
    if (g) g[1].push(c)
    else groups.push([key, [c]])
  }
  const count = (s: State) => a?.changes.filter((c) => stateOf(c) === s).length ?? 0
  const failed = a?.checks.filter((c) => !c.ok && c.check !== 'review') ?? []

  return (
    <Sheet open={!!docId} onOpenChange={(o) => !o && onClose()}>
      <SheetContent aria-describedby="am-desc" className="overflow-y-auto sm:max-w-[560px]">
        <div className="px-5 pt-5 pr-14">
          <p className="t-label">What this gazette changed</p>
          <SheetTitle className="mt-1.5 text-[20px] leading-tight font-semibold tracking-[-0.02em] text-ink">Gazette {a?.number ?? (docId ? gazetteNumber(docId) : '')}</SheetTitle>
          <SheetDescription id="am-desc" className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-ink2">
            {a ? (
              <>
                Amends Gazette {a.amends ? gazetteNumber(a.amends) : 'not named'} from {fmtDay(a.effective_from)}
                {docId && <Evidence docId={docId} label="PDF" />}
                {q.data?.sample && <SampleMark />}
              </>
            ) : q.isError ? (
              'This gazette is not an amendment in the record.'
            ) : (
              'Loading its changes.'
            )}
          </SheetDescription>
        </div>
        <div className="px-5 pt-4 pb-8">
          {q.isPending && docId && (
            <div className="flex flex-col gap-3" aria-busy>
              <div className="skeleton h-14 w-full rounded-xl" />
              <div className="skeleton h-4 w-2/3" />
              <div className="skeleton h-4 w-full" />
            </div>
          )}
          {q.isError && <StateBox title="No changes to show" tone="bad">The record has not read this gazette as an amendment.</StateBox>}
          {a && (
            <>
              <div className={cn('rounded-xl border px-3.5 py-3 text-[13px]', served ? 'border-ok/30 bg-ok-soft' : 'border-warn/30 bg-warn-soft')} role="status">
                <p className="font-medium text-ink">{served ? 'Applied: the lists for this date include these changes.' : 'Not applied yet: the lists for this date do not include these changes.'}</p>
                {!served && failed.length > 0 && (
                  <ul className="mt-1.5 flex list-disc flex-col gap-1 pl-4 text-ink2">
                    {failed.map((c) => (
                      <li key={c.check}>{c.detail}</li>
                    ))}
                  </ul>
                )}
              </div>
              <dl className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
                {(['applied', 'review', 'skipped', 'other'] as const).map((s) => (
                  <div key={s} className="rounded-xl border border-line bg-bg2 px-3 py-2">
                    <dt className="text-[11.5px] leading-tight text-ink3">{STATE[s].label}</dt>
                    <dd className="t-num mt-0.5 text-[18px] font-semibold text-ink">{count(s)}</dd>
                  </div>
                ))}
              </dl>
              {served && (
                <Button variant="outline" size="sm" className="mt-4" onClick={() => onCompare(dayBefore(a.effective_from), a.effective_from)}>
                  <GitCompareArrows /> Compare the day before with the day after
                </Button>
              )}
              <ol className="mt-6 flex flex-col gap-6">
                {groups.map(([title, cs]) => (
                  <li key={title}>
                    <h3 className="text-[14px] font-semibold tracking-tight text-ink">{title}</h3>
                    <ul className="mt-2 flex flex-col divide-y divide-line rounded-xl border border-line">
                      {cs.map((c) => {
                        const st = STATE[stateOf(c)]
                        return (
                          <li key={c.change_no} className="px-3 py-2.5">
                            <div className="flex items-start justify-between gap-2">
                              <p className="min-w-0 text-[13px] leading-snug text-ink">
                                <span className="font-medium">{OP[c.op]}</span>
                                {c.col && <span className="text-ink2"> · {COL[c.col]}</span>}
                                {c.note && <span className="block text-ink2">{c.note}</span>}
                              </p>
                              <span className="flex shrink-0 items-center gap-1">
                                <span className={cn('inline-flex items-center gap-1 text-[11.5px] font-medium', st.cls)}>
                                  <st.Icon className="size-3.5" aria-hidden />
                                  {st.label}
                                </span>
                                {docId && <Evidence docId={docId} page={c.page} />}
                              </span>
                            </div>
                            <details className="group/t mt-1">
                              <summary className="pressable inline-flex cursor-pointer list-none items-center gap-1 rounded text-[11.5px] text-ink3 hover:text-ink2 [&::-webkit-details-marker]:hidden">
                                <ArrowLeftRight className="size-3" aria-hidden /> As printed, clause {c.clause}
                              </summary>
                              <p className="mt-1 text-[12px] leading-relaxed text-ink2">{c.text || '(no sentence: the schedule itself)'}</p>
                            </details>
                          </li>
                        )
                      })}
                    </ul>
                  </li>
                ))}
              </ol>
              <p className="mt-6 text-[12px] leading-relaxed text-ink3">
                Each change is read from the gazette and applied to the structure in force on its date. A change is applied only when the item or ministry it names is there; when the number and the text it prints disagree, or a change cannot be placed, the gazette waits for a person to check it.
              </p>
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}
