// The ministries in force on a date: one card each, with the institutions in the minister's charge.
import { useQuery } from '@tanstack/react-query'
import { ChevronDown, Search, X } from 'lucide-react'
import { useId, useMemo, useState } from 'react'
import { entitySearchQuery, gazetteNumber, KIND_LABEL, KINDS, shortOf, type EntityKind, type Structure, type StructurePortfolio } from '@/api/civic'
import { SampleMark } from '@/components/common/bits'
import { useTheme } from '@/lib/hooks'
import { fmtInt } from '@/lib/format'
import { cn } from '@/lib/utils'
import { Evidence, fmtDay, KindDot, kindColor, Mark } from './shared'

const COLLAPSED = 8
const listAnd = (xs: string[]) => (xs.length > 1 ? `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}` : (xs[0] ?? ''))
const short = (p: { title: string; short_title?: string }) => shortOf(p)

function KindBar({ items, theme }: { items: StructurePortfolio['institutions']; theme: 'dark' | 'light' }) {
  const counts = KINDS.map((k) => [k, items.filter((i) => i.kind === k).length] as const).filter(([, n]) => n > 0)
  const label = counts.map(([k, n]) => `${n} ${KIND_LABEL[k].toLowerCase()}`).join(', ')
  return (
    <div className="flex h-1.5 gap-[2px] overflow-hidden rounded-full" role="img" aria-label={label || 'No institutions'} title={label}>
      {counts.map(([k, n]) => (
        <span key={k} style={{ flex: `${n} 1 0%`, background: kindColor(k, theme) }} className="min-w-[3px]" />
      ))}
    </div>
  )
}

function Card({ p, q, onOpen, theme }: { p: StructurePortfolio; q: string; onOpen: (key: string) => void; theme: 'dark' | 'light' }) {
  const [all, setAll] = useState(false)
  const needle = q.trim().toLowerCase()
  const titleHit = !!needle && short(p).toLowerCase().includes(needle)
  const rows = needle && !titleHit ? p.institutions.filter((i) => i.text.toLowerCase().includes(needle)) : p.institutions
  const visible = needle || all ? rows : rows.slice(0, COLLAPSED)
  const listId = useId()
  return (
    <section className="surface flex min-w-0 flex-col p-4 sm:p-5" aria-labelledby={`${listId}-h`}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <span className="t-label">{String(p.seq).padStart(2, '0')}</span>
          <h3 id={`${listId}-h`} className="mt-1 text-[16px] leading-snug font-semibold tracking-[-0.015em] text-ink">
            <Mark text={short(p)} q={q} />
          </h3>
        </div>
        <Evidence docId={p.doc_id} page={p.page} className="mt-0.5" />
      </div>
      <p className="t-num mt-2 text-[12.5px] text-ink2">
        <span className="font-semibold text-ink">{fmtInt(p.institutions.length)}</span> {p.institutions.length === 1 ? 'institution' : 'institutions'}
      </p>
      <div className="mt-2.5">
        <KindBar items={p.institutions} theme={theme} />
      </div>
      <ul id={listId} className={cn('mt-3 -mx-1.5 flex flex-col', all && rows.length > 16 && 'scroll-thin max-h-[26rem] overflow-y-auto')} tabIndex={all && rows.length > 16 ? 0 : undefined}>
        {visible.map((i) => (
          <li key={i.item_no} className="group flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-bg3">
            <KindDot kind={i.kind} theme={theme} />
            {i.entity_key ? (
              <button type="button" onClick={() => onOpen(i.entity_key!)} className="min-w-0 flex-1 rounded text-left text-[13px] leading-snug text-ink hover:text-brand-ink" aria-label={`${i.text}: history`}>
                <Mark text={i.text} q={q} />
              </button>
            ) : (
              <span className="min-w-0 flex-1 text-[13px] leading-snug text-ink">{i.text}</span>
            )}
            <Evidence docId={p.doc_id} page={i.page} />
          </li>
        ))}
        {!rows.length && <li className="px-1.5 py-1 text-[12.5px] text-ink3">No institutions listed.</li>}
      </ul>
      {!needle && rows.length > COLLAPSED && (
        <button type="button" onClick={() => setAll(!all)} aria-expanded={all} aria-controls={listId} className="pressable mt-1.5 inline-flex items-center gap-1 self-start rounded-md px-1.5 py-1 text-[12.5px] font-medium text-brand-ink hover:bg-brand-soft">
          {all ? 'Show fewer' : `Show all ${rows.length}`}
          <ChevronDown className={cn('size-3.5 transition-transform', all && 'rotate-180')} />
        </button>
      )}
      {p.functions.length > 0 && (
        <details className="group/f mt-3 border-t border-line pt-3">
          <summary className="pressable flex cursor-pointer list-none items-center gap-1.5 rounded-md text-[12.5px] font-medium text-ink2 hover:text-ink [&::-webkit-details-marker]:hidden">
            <ChevronDown className="size-3.5 -rotate-90 transition-transform group-open/f:rotate-0" />
            Subjects and functions ({p.functions.length})
          </summary>
          <ol className="mt-2 flex list-decimal flex-col gap-1.5 pl-5 text-[12.5px] leading-relaxed text-ink2 marker:text-ink3">
            {p.functions.map((f) => (
              <li key={f.item_no}>{f.text}</li>
            ))}
          </ol>
        </details>
      )}
    </section>
  )
}

export function StructureView({ s, sample, at, onOpen }: { s: Structure; sample: boolean; at: string; onOpen: (key: string) => void }) {
  const [theme] = useTheme()
  const [q, setQ] = useState('')
  const total = s.portfolios.reduce((n, p) => n + p.institutions.length, 0)
  const needle = q.trim().toLowerCase()
  const shown = useMemo(
    () => (needle ? s.portfolios.filter((p) => short(p).toLowerCase().includes(needle) || p.institutions.some((i) => i.text.toLowerCase().includes(needle))) : s.portfolios),
    [s.portfolios, needle],
  )
  const matches = needle ? shown.reduce((n, p) => n + (short(p).toLowerCase().includes(needle) ? p.institutions.length : p.institutions.filter((i) => i.text.toLowerCase().includes(needle)).length), 0) : 0
  const hits = useQuery(entitySearchQuery(q.trim()))
  const inView = new Set(s.portfolios.flatMap((p) => p.institutions.map((i) => i.entity_key)))
  const elsewhere = (hits.data?.data ?? []).filter((h) => !inView.has(h.entity_key))
  const counts = KINDS.map((k) => [k, s.portfolios.reduce((n, p) => n + p.institutions.filter((i) => i.kind === k).length, 0)] as const).filter(([, n]) => n > 0)
  const num = s.instrument.number
  const composed = s.composed_of ?? []

  return (
    <div>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <h2 className="t-h3 min-w-0">
          In force on {fmtDay(at)}: {composed.length > 1 ? `composed of gazettes ${listAnd(composed.map((c) => gazetteNumber(c.doc_id)))}` : `Gazette ${num}`}, {fmtInt(s.portfolios.length)} {s.portfolios.length === 1 ? 'ministry' : 'ministries'}, {fmtInt(total)} institutions
        </h2>
        {sample && <SampleMark />}
      </div>
      {composed.length > 1 ? (
        <p className="mt-2 flex flex-wrap items-center gap-x-1.5 text-[13.5px] text-ink2">
          <span>
            Each notice names the ministers just appointed; together they make one cabinet from {fmtDay(composed[0].effective_from)}.
          </span>
          {composed.map((c) => (
            <Evidence key={c.doc_id} docId={c.doc_id} label={`${gazetteNumber(c.doc_id)} PDF`} />
          ))}
        </p>
      ) : (
        <p className="mt-2 text-[13.5px] text-ink2">
          Schedule of {fmtDay(s.instrument.effective_from)} <Evidence docId={s.instrument.doc_id} label={`Gazette ${num} PDF`} />
        </p>
      )}
      {s.amendments_since.length > 0 && (
        <div className="mt-3 rounded-xl border border-warn/30 bg-warn-soft px-3.5 py-2.5 text-[13px] text-ink2">
          <p className="font-medium text-ink">
            {s.amendments_since.length === 1 ? 'One amendment' : `${s.amendments_since.length} amendments`} since, shown here but not yet applied to the lists below:
          </p>
          <ul className="mt-1 flex flex-col gap-0.5">
            {s.amendments_since.map((a) => (
              <li key={a.doc_id} className="flex flex-wrap items-center gap-x-2">
                <span>
                  Gazette {a.doc_id.replace(/^gze-/, '').replace('-', '/')}, effective {fmtDay(a.effective_from)}
                </span>
                <Evidence docId={a.doc_id} label="PDF" />
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="mt-5 flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="relative w-full sm:w-[340px]">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink3" aria-hidden />
          <input
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Find an institution or ministry"
            aria-label="Find an institution or ministry"
            className="h-10 w-full rounded-xl border border-line2 bg-bg2 pr-9 pl-9 text-[16px] text-ink outline-none placeholder:text-ink3 focus-visible:border-brand focus-visible:ring-2 focus-visible:ring-brand/30 sm:text-[14px] [&::-webkit-search-cancel-button]:hidden"
          />
          {q && (
            <button type="button" onClick={() => setQ('')} aria-label="Clear search" className="pressable absolute top-1/2 right-1.5 grid size-7 -translate-y-1/2 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink">
              <X className="size-4" />
            </button>
          )}
        </div>
        <ul className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[12px] text-ink2" aria-label="Kinds of institution">
          {counts.map(([k, n]) => (
            <li key={k} className="inline-flex items-center gap-1.5">
              <span className="size-2.5 rounded-[3px]" style={{ background: kindColor(k as EntityKind, theme) }} aria-hidden />
              {KIND_LABEL[k]}
              <span className="t-num text-ink3">{n}</span>
            </li>
          ))}
        </ul>
      </div>
      {needle && (
        <p className="mt-3 text-[13px] text-ink2" role="status">
          {matches ? `${matches} ${matches === 1 ? 'match' : 'matches'} in ${shown.length} ${shown.length === 1 ? 'ministry' : 'ministries'} on this date.` : 'Nothing on this date matches.'}
        </p>
      )}
      {elsewhere.length > 0 && (
        <div className="mt-2 text-[13px] text-ink2">
          <span>In the record, but not on this date: </span>
          {elsewhere.map((h, i) => (
            <span key={h.entity_key}>
              {i > 0 && ', '}
              <button type="button" onClick={() => onOpen(h.entity_key)} className="rounded text-brand-ink underline-offset-2 hover:underline">
                {h.name}
              </button>
            </span>
          ))}
        </div>
      )}

      {shown.length ? (
        <div className="mt-5 grid grid-cols-1 items-start gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {shown.map((p) => (
            <Card key={`${p.doc_id}-${p.seq}`} p={p} q={q} onOpen={onOpen} theme={theme} />
          ))}
        </div>
      ) : (
        <div className="mt-5 rounded-2xl border border-dashed border-line2 p-8 text-center text-[13.5px] text-ink2">No ministry or institution on {fmtDay(at)} matches “{q}”.</div>
      )}
    </div>
  )
}
