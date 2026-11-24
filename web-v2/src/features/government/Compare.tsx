// Two dates side by side: what moved between ministries, what was added, what was removed.
import { ArrowLeftRight, ArrowRight, X } from 'lucide-react'
import { useState } from 'react'
import { gazetteNumber, plain, shortOf, type Added, type Diff, type Instrument, type Moved, type Removed } from '@/api/civic'
import { SampleMark } from '@/components/common/bits'
import { Button } from '@/components/ui/button'
import { useTheme } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { FlowDiagram, matchesSel, pairsOf } from './FlowDiagram'
import { Evidence, fmtDay, KindDot, ministryColor } from './shared'

export function CompareControls({ baselines, from, to, onChange }: { baselines: Instrument[]; from: string; to: string; onChange: (from: string, to: string) => void }) {
  const opts = (value: string) => {
    const has = baselines.some((b) => b.effective_from === value)
    return (
      <>
        {!has && <option value={value}>{fmtDay(value)}</option>}
        {baselines.map((b) => (
          <option key={b.doc_id} value={b.effective_from}>
            {fmtDay(b.effective_from)} · Gazette {b.number}{b.kind === 'supplement' ? ' (supplement)' : ''}
          </option>
        ))}
      </>
    )
  }
  const sel = 'h-10 min-w-0 flex-1 rounded-xl border border-line2 bg-bg2 px-3 text-[16px] text-ink outline-none focus-visible:border-brand focus-visible:ring-2 focus-visible:ring-brand/30 sm:text-[14px]'
  return (
    <div className="flex flex-wrap items-end gap-3">
      <label className="flex min-w-[220px] flex-1 flex-col gap-1.5 text-[12px] font-medium text-ink2 sm:max-w-[320px]">
        From
        <select className={sel} value={from} onChange={(e) => onChange(e.target.value, to)}>
          {opts(from)}
        </select>
      </label>
      <button type="button" onClick={() => onChange(to, from)} aria-label="Swap the two dates" className="pressable grid size-10 shrink-0 place-items-center rounded-xl border border-line2 bg-bg2 text-ink2 hover:text-ink">
        <ArrowLeftRight className="size-4" />
      </button>
      <label className="flex min-w-[220px] flex-1 flex-col gap-1.5 text-[12px] font-medium text-ink2 sm:max-w-[320px]">
        To
        <select className={sel} value={to} onChange={(e) => onChange(from, e.target.value)}>
          {opts(to)}
        </select>
      </label>
    </div>
  )
}

function Tile({ label, value, tone }: { label: string; value: number; tone?: 'brand' }) {
  return (
    <div className="surface px-4 py-3">
      <div className={cn('t-num text-[28px] leading-none font-semibold tracking-[-0.03em]', tone === 'brand' ? 'text-brand-ink' : 'text-ink')}>{value}</div>
      <div className="mt-1.5 text-[12.5px] text-ink2">{label}</div>
    </div>
  )
}

type Row = { entity_key: string; name: string; kind: Moved['kind']; from?: { page?: number | null }; to?: { page?: number | null } }
function Group({ title, rows, fromDoc, toDoc, onOpen, theme }: { title: React.ReactNode; rows: Row[]; fromDoc?: string; toDoc?: string; onOpen: (k: string) => void; theme: 'dark' | 'light' }) {
  return (
    <div className="surface p-4">
      <h4 className="flex flex-wrap items-center gap-x-2 text-[14px] font-semibold tracking-[-0.01em] text-ink">
        {title}
        <span className="t-num font-normal text-ink3">{rows.length}</span>
      </h4>
      <ul className="mt-2 -mx-1.5 flex flex-col">
        {rows.map((r) => (
          <li key={r.entity_key} className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-bg3">
            <KindDot kind={r.kind} theme={theme} />
            <button type="button" onClick={() => onOpen(r.entity_key)} className="min-w-0 flex-1 rounded text-left text-[13px] leading-snug text-ink hover:text-brand-ink">
              {r.name}
            </button>
            {fromDoc && r.from && <Evidence docId={fromDoc} page={r.from.page} label={r.from.page ? `before p.${r.from.page}` : 'before'} />}
            {toDoc && r.to && <Evidence docId={toDoc} page={r.to.page} label={r.to.page ? `now p.${r.to.page}` : 'now'} />}
          </li>
        ))}
      </ul>
    </div>
  )
}

const GROUPS_SHOWN = 12
function Groups({ groups }: { groups: React.ReactNode[] }) {
  const [all, setAll] = useState(false)
  const shown = all ? groups : groups.slice(0, GROUPS_SHOWN)
  return (
    <>
      <div className="mt-3 grid grid-cols-1 items-start gap-3 lg:grid-cols-2">{shown}</div>
      {groups.length > GROUPS_SHOWN && (
        <Button variant="outline" size="sm" className="mt-3" onClick={() => setAll(!all)}>
          {all ? 'Show fewer' : `Show all ${groups.length} groups`}
        </Button>
      )}
    </>
  )
}

function groupBy<T>(xs: T[], key: (x: T) => string) {
  const m = new Map<string, T[]>()
  for (const x of xs) m.set(key(x), [...(m.get(key(x)) ?? []), x])
  return [...m.entries()].sort((a, b) => b[1].length - a[1].length)
}

export function CompareView({ diff, sample, onOpen }: { diff: Diff; sample: boolean; onOpen: (k: string) => void }) {
  const [theme] = useTheme()
  const [sel, setSel] = useState<string | null>(null)
  const pairs = pairsOf(diff.moved)
  const picked = pairs.filter((p) => matchesSel(p, sel))
  const pickedItems = picked.reduce((n, p) => n + p.items.length, 0)
  const mAdded = diff.portfolios.added.map(plain)
  const mRemoved = diff.portfolios.removed.map(plain)
  const renamed = diff.portfolios.kept.filter((k) => (k.relation ? k.relation === 'renamed' : k.renamed))
  // a split or merge: the new ministry took most of the old one's institutions, but is not the same ministry
  const successors = diff.portfolios.kept.filter((k) => k.relation === 'successor')
  const c = diff.counts
  const unchanged = !diff.moved.length && !diff.added.length && !diff.removed.length
  const pill = (t: string, k: string) => (
    <span key={k} className="inline-flex items-center gap-1.5 rounded-full border border-line2 bg-bg3 px-2.5 py-0.5 text-[12px] text-ink">
      <span className="size-2 rounded-full" style={{ background: ministryColor(k, theme) }} aria-hidden />
      {t}
    </span>
  )
  return (
    <div className="flex flex-col gap-8">
      <div>
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <h2 className="t-h3">
            {fmtDay(diff.from.effective_from)} <ArrowRight className="mx-0.5 inline size-4 align-[-2px] text-ink3" aria-label="to" /> {fmtDay(diff.to.effective_from)}
          </h2>
          {sample && <SampleMark />}
        </div>
        <p className="mt-1.5 text-[13.5px] text-ink2">
          Gazette {diff.from.number ?? gazetteNumber(diff.from.doc_id)} <Evidence docId={diff.from.doc_id} label="PDF" /> against Gazette {diff.to.number ?? gazetteNumber(diff.to.doc_id)} <Evidence docId={diff.to.doc_id} label="PDF" />
        </p>
        <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
          <Tile label="Institutions moved" value={c?.moved ?? diff.moved.length} tone="brand" />
          <Tile label="Institutions added" value={c?.added ?? diff.added.length} />
          <Tile label="Institutions removed" value={c?.removed ?? diff.removed.length} />
          <Tile label="Ministries added" value={c?.portfolios_added ?? mAdded.length} />
          <Tile label="Ministries removed" value={c?.portfolios_removed ?? mRemoved.length} />
          <Tile label="Ministries renamed" value={c?.portfolios_renamed ?? renamed.length} />
        </div>
        {(mAdded.length > 0 || mRemoved.length > 0 || renamed.length > 0 || successors.length > 0) && (
          <div className="mt-3 flex flex-col gap-2 text-[13px] text-ink2">
            {mAdded.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 font-medium text-ink">New ministries</span>
                {mAdded.map((t) => pill(t.replace(/^Minister of /, ''), t))}
              </div>
            )}
            {renamed.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 font-medium text-ink">Renamed</span>
                {renamed.map((k) => (
                  <span key={`${k.from_key}>${k.to_key}`} className="inline-flex items-center gap-1.5 rounded-full border border-line2 bg-bg3 px-2.5 py-0.5 text-[12px] text-ink">
                    {plain(k.from_title)} <ArrowRight className="size-3 text-ink3" aria-hidden /> {plain(k.to_title)}
                  </span>
                ))}
              </div>
            )}
            {successors.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 font-medium text-ink">Split or merged</span>
                {successors.map((k) => (
                  <span
                    key={`${k.from_key}>${k.to_key}`}
                    title={k.share != null ? `${Math.round(k.share * 100)}% of its institutions went to the new ministry` : undefined}
                    className="inline-flex items-center gap-1.5 rounded-full border border-dashed border-line2 bg-bg3 px-2.5 py-0.5 text-[12px] text-ink"
                  >
                    {plain(k.from_title)} <span className="text-ink3">succeeded by</span> {plain(k.to_title)}
                    {k.share != null && <span className="font-mono text-[11px] text-ink3">{Math.round(k.share * 100)}%</span>}
                  </span>
                ))}
              </div>
            )}
            {mRemoved.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 font-medium text-ink">Ministries no longer listed</span>
                {mRemoved.map((t) => pill(t.replace(/^Minister of /, ''), t))}
              </div>
            )}
          </div>
        )}
      </div>

      {unchanged && <div className="rounded-2xl border border-dashed border-line2 p-8 text-center text-[13.5px] text-ink2">No institution moved, was added or was removed between these two dates.</div>}

      {pairs.length > 0 && (
        <section aria-labelledby="flow-h">
          <h3 id="flow-h" className="t-h3">
            Where institutions moved
          </h3>
          <p className="mt-1 mb-3 text-[13px] text-ink2">
            Ministries before on the left, after on the right. A ribbon is as wide as the number of institutions that changed hands.
            <span className="md:hidden"> Scroll the diagram sideways; tap a ribbon to see its institutions.</span>
            <span className="max-md:hidden"> Hover for names, click to list them.</span>
          </p>
          <FlowDiagram pairs={pairs} selected={sel} onSelect={setSel} theme={theme} />
          <div aria-live="polite">
            {picked.length > 0 && (
              <div className="surface mt-3 p-4">
                <div className="flex items-start justify-between gap-3">
                  <h4 className="text-[14px] font-semibold text-ink">
                    {sel?.includes(':') ? `${pickedItems} moved ${sel.startsWith('L:') ? 'out of' : 'into'} ${sel.startsWith('L:') ? picked[0].from.title : picked[0].to.title}` : `${picked[0].from.title} → ${picked[0].to.title}`}
                  </h4>
                  <button type="button" onClick={() => setSel(null)} aria-label="Clear selection" className="pressable grid size-7 shrink-0 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink">
                    <X className="size-4" />
                  </button>
                </div>
                <div className="mt-2 flex flex-col gap-3">
                  {picked.map((p) => (
                    <div key={p.id}>
                      {picked.length > 1 && (
                        <p className="mb-1 text-[12px] font-medium text-ink2">
                          {p.from.title} → {p.to.title}
                        </p>
                      )}
                      <ul className="flex flex-wrap gap-x-5 gap-y-1">
                        {p.items.map((i) => (
                          <li key={i.entity_key} className="flex items-center gap-2 text-[13px]">
                            <KindDot kind={i.kind} theme={theme} />
                            <button type="button" onClick={() => onOpen(i.entity_key)} className="rounded text-left text-ink hover:text-brand-ink">
                              {i.name}
                            </button>
                            <Evidence docId={diff.to.doc_id} page={i.to.page} />
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </section>
      )}

      {diff.moved.length > 0 && (
        <section aria-labelledby="moved-h">
          <h3 id="moved-h" className="t-h3">
            Moved
          </h3>
          <Groups
            groups={groupBy(diff.moved, (m) => `${m.from.portfolio_key}>${m.to.portfolio_key}`).map(([k, rows]) => (
              <Group
                key={k}
                theme={theme}
                fromDoc={diff.from.doc_id}
                toDoc={diff.to.doc_id}
                onOpen={onOpen}
                rows={rows}
                title={
                  <span>
                    {shortOf(rows[0].from)} <ArrowRight className="mx-0.5 inline size-3.5 align-[-2px] text-ink3" aria-label="to" /> {shortOf(rows[0].to)}
                  </span>
                }
              />
            ))}
          />
        </section>
      )}
      {diff.added.length > 0 && (
        <section aria-labelledby="added-h">
          <h3 id="added-h" className="t-h3">
            Added to a ministry
          </h3>
          <Groups groups={groupBy<Added>(diff.added, (a) => shortOf(a.to)).map(([k, rows]) => <Group key={k} theme={theme} toDoc={diff.to.doc_id} onOpen={onOpen} rows={rows} title={`Added to ${k}`} />)} />
        </section>
      )}
      {diff.removed.length > 0 && (
        <section aria-labelledby="removed-h">
          <h3 id="removed-h" className="t-h3">
            Removed from a ministry
          </h3>
          <Groups groups={groupBy<Removed>(diff.removed, (a) => shortOf(a.from)).map(([k, rows]) => <Group key={k} theme={theme} fromDoc={diff.from.doc_id} onOpen={onOpen} rows={rows} title={`Removed from ${k}`} />)} />
        </section>
      )}
    </div>
  )
}
