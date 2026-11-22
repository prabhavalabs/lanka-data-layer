import { useQuery } from '@tanstack/react-query'
import { ArrowUpRight, FileText, Fingerprint, ScanSearch, UserCheck } from 'lucide-react'
import { provenanceQuery } from '@/api/queries'
import type { Provenance } from '@/api/types'
import { LicenceLight } from '@/components/common/bits'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { useIsDesktop } from '@/lib/hooks'
import { cn } from '@/lib/utils'

/** A rendered "crop" of the source page: the printed table with the cell we read highlighted. */
export function PageCrop({ p, className }: { p: Pick<Provenance, 'crop' | 'locator' | 'document'>; className?: string }) {
  return (
    <figure className={cn('overflow-hidden rounded-lg border border-line2 bg-[#f4f1ea] text-[#2a2620] shadow-[inset_0_0_40px_rgba(120,100,60,0.12)]', className)}>
      <div className="flex items-center justify-between border-b border-black/10 px-3 py-1.5 font-mono text-[10px] text-black/45">
        <span>{p.locator.table} · Population by district and sex</span>
        <span>{p.locator.page ? `p. ${p.locator.page}` : p.locator.table}</span>
      </div>
      <table className="w-full border-collapse font-serif text-[12.5px] leading-tight">
        <thead>
          <tr className="border-b border-black/25">
            {p.crop.header.map((h, i) => (
              <th key={h} className={cn('px-3 py-1.5 font-semibold', i === 0 ? 'text-left' : 'text-right')}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {p.crop.rows.map((r) => (
            <tr key={r.label} className="border-b border-black/[0.07] last:border-0">
              <td className="px-3 py-[5px]">{r.label}</td>
              {r.cells.map((c, i) => (
                <td key={i} className="t-num px-3 py-[5px] text-right">
                  <span className={cn('relative', r.hit === i && 'font-semibold')}>
                    {r.hit === i && <span className="absolute -inset-x-1.5 -inset-y-0.5 rounded-[3px] bg-[#d8446b]/20 ring-1 ring-[#d8446b]" />}
                    <span className="relative">{c}</span>
                  </span>
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  )
}

function Row({ icon: Icon, label, children }: { icon: typeof FileText; label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[20px_1fr] gap-x-3 py-3.5">
      <Icon className="mt-0.5 size-4 text-ink3" />
      <div className="min-w-0">
        <div className="t-label mb-1">{label}</div>
        <div className="text-[13.5px] text-ink">{children}</div>
      </div>
    </div>
  )
}

/** "Every number traceable": value → document → page and cell → file hash → review. */
export function ProvenanceDrawer({ observationId, onClose, title }: { observationId: string | null; onClose: () => void; title?: string }) {
  const desktop = useIsDesktop()
  const { data: p, isPending } = useQuery(provenanceQuery(observationId))
  return (
    <Sheet open={!!observationId} onOpenChange={(o) => !o && onClose()}>
      <SheetContent side={desktop ? 'right' : 'bottom'} aria-describedby={undefined}>
        <div className="border-b border-line px-5 pt-5 pb-4">
          <div className="t-label mb-1.5">Provenance · {observationId}</div>
          <SheetTitle className="t-h3">{title ?? p?.indicator ?? 'Observation'}</SheetTitle>
          <SheetDescription className="sr-only">Source document, locator, hash and review record for this value</SheetDescription>
        </div>
        <div className="scroll-thin flex-1 overflow-y-auto px-5 pb-6">
          {isPending || !p ? (
            <div className="space-y-3 pt-5">
              <div className="skeleton h-14" />
              <div className="skeleton h-40" />
              <div className="skeleton h-24" />
            </div>
          ) : (
            <div className="divide-y divide-line">
              <div className="flex items-end justify-between gap-4 py-4">
                <div>
                  <div className="t-label mb-1">Value we serve</div>
                  <div className="t-num text-[28px] leading-none font-semibold tracking-[-0.03em]">{p.value}</div>
                </div>
                <div className="text-right">
                  <div className="t-label mb-1">Printed in source</div>
                  <div className="t-num font-mono text-[15px] text-ink2">“{p.verbatim}”</div>
                </div>
              </div>
              <div className="py-4">
                <PageCrop p={p} />
                <p className="mt-2 text-xs text-ink3">
                  {p.locator.page ? `Rendered crop of page ${p.locator.page}.` : 'Source table.'} Row “{p.locator.row}”, column “{p.locator.column}”.
                </p>
              </div>
              <Row icon={FileText} label="Source document">
                <a href={p.document.url} target="_blank" rel="noreferrer" className="inline-flex items-start gap-1 font-medium hover:text-brand-ink">
                  {p.document.title} <ArrowUpRight className="mt-0.5 size-3.5 shrink-0" />
                </a>
                <div className="mt-0.5 text-ink2">
                  {p.document.publisher} · published {p.document.published} {p.document.pages ? `· ${p.document.pages} pages` : ''}
                </div>
              </Row>
              <Row icon={ScanSearch} label="Locator">
                <span className="font-mono text-[12.5px]">
                  {p.locator.page ? `page ${p.locator.page} › ` : ''}{p.locator.table} › row “{p.locator.row}” › col “{p.locator.column}”
                </span>
              </Row>
              <Row icon={Fingerprint} label="File hash (SHA-256)">
                <span className="font-mono text-[12px] break-all text-ink2">{p.sha256}</span>
                <div className="mt-1 text-xs text-ink3">Retrieved {p.retrieved.replace('T', ' ').replace('Z', ' UTC')}</div>
              </Row>
              <Row icon={UserCheck} label="Review">
                {p.review.method}
                <div className="mt-0.5 text-ink2">
                  Checked by {p.review.reviewer}, {p.review.date}
                </div>
              </Row>
              <div className="pt-4">
                <LicenceLight status={p.licence.status} label={p.licence.label} />
                <p className="mt-2 text-xs leading-relaxed text-ink3">
                  We show the source and its status as we found it. Where a publisher states no licence, we say so and keep the dataset out of the open bundle.
                </p>
              </div>
            </div>
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}
