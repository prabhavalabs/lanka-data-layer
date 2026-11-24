// Small things the Government view's parts share: colours, dates, the gazette PDF link, the sample note.
import { FileText } from 'lucide-react'
import type { ReactNode } from 'react'
import { gazettePdf, KIND_LABEL, KINDS, type EntityKind } from '@/api/civic'
import { categorical } from '@/lib/palettes'
import type { Theme } from '@/lib/hooks'
import { cn } from '@/lib/utils'

/** One colour per kind, fixed order (the contract's order), from the categorical set. */
export const kindColor = (kind: EntityKind, theme: Theme) => categorical(theme)[Math.max(0, KINDS.indexOf(kind))]

/** A ministry's colour: stable from its key, so a ministry keeps its colour across the flow, the lists and the drawer. */
export function ministryColor(key: string, theme: Theme) {
  let h = 0
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0
  const set = categorical(theme)
  return set[h % set.length]
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
/** "25 Nov 2024" from "2024-11-25". */
export function fmtDay(iso: string | null | undefined) {
  if (!iso) return 'today'
  const [y, m, d] = iso.slice(0, 10).split('-').map(Number)
  return `${d} ${MONTHS[m - 1]} ${y}`
}
export const isoDay = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
/** Local-noon date from an ISO day, so day arithmetic never crosses a DST edge. */
export const parseDay = (iso: string) => {
  const [y, m, d] = iso.slice(0, 10).split('-').map(Number)
  return new Date(y, m - 1, d, 12)
}
export const isDay = (s: string | null): s is string => !!s && /^\d{4}-\d{2}-\d{2}$/.test(s) && !Number.isNaN(parseDay(s).getTime())

/** Small link to the gazette PDF, opened at the page the item is printed on. */
export function Evidence({ docId, page, label, className }: { docId: string; page?: number | null; label?: string; className?: string }) {
  const text = label ?? (page ? `p.${page}` : 'PDF')
  return (
    <a
      href={gazettePdf(docId, page)}
      target="_blank"
      rel="noreferrer"
      title={page ? `Gazette PDF, page ${page}` : 'Gazette PDF'}
      aria-label={`Gazette PDF${page ? `, page ${page}` : ''} (opens in a new tab)`}
      className={cn('pressable inline-flex shrink-0 items-center gap-1 rounded-md px-1 py-0.5 font-mono text-[10.5px] text-ink3 hover:bg-bg3 hover:text-brand-ink', className)}
    >
      <FileText className="size-3" aria-hidden />
      {text}
    </a>
  )
}

export function KindDot({ kind, theme }: { kind: EntityKind; theme: Theme }) {
  return <span className="inline-block size-2 shrink-0 rounded-full" style={{ background: kindColor(kind, theme) }} title={KIND_LABEL[kind]} role="img" aria-label={KIND_LABEL[kind]} />
}

/** Text with the part matching `q` marked. */
export function Mark({ text, q }: { text: string; q: string }): ReactNode {
  const needle = q.trim().toLowerCase()
  if (needle.length < 1) return text
  const at = text.toLowerCase().indexOf(needle)
  if (at < 0) return text
  return (
    <>
      {text.slice(0, at)}
      <mark className="rounded-[3px] bg-warn-soft px-0.5 text-ink [box-decoration-break:clone]">{text.slice(at, at + needle.length)}</mark>
      {text.slice(at + needle.length)}
    </>
  )
}

export function StateBox({ title, children, tone = 'neutral' }: { title: string; children?: ReactNode; tone?: 'neutral' | 'bad' }) {
  return (
    <div role={tone === 'bad' ? 'alert' : 'status'} className={cn('rounded-2xl border border-dashed p-8 text-center', tone === 'bad' ? 'border-bad/40 bg-bad-soft' : 'border-line2 bg-bg2')}>
      <p className="text-[15px] font-semibold tracking-tight text-ink">{title}</p>
      {children && <div className="mx-auto mt-2 max-w-[56ch] text-[13.5px] leading-relaxed text-ink2">{children}</div>}
    </div>
  )
}
