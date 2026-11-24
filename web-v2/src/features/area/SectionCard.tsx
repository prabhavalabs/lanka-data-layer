import { ArrowRight, BarChart3, Table2 } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { Eyebrow, Reveal, SampleMark } from '@/components/common/bits'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'

export type View = 'chart' | 'table'

export function ViewToggle({ value, onChange, label }: { value: View; onChange: (v: View) => void; label: string }) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => v && onChange(v as View)} aria-label={`${label}: chart or table`} className="shrink-0 max-md:h-12">
      <ToggleGroupItem value="chart">
        <BarChart3 />
        Chart
      </ToggleGroupItem>
      <ToggleGroupItem value="table">
        <Table2 />
        Table
      </ToggleGroupItem>
    </ToggleGroup>
  )
}

/** One thematic section: finding-style title, charts, then a mono source + vintage line. */
export function SectionCard({
  id,
  n,
  kicker,
  title,
  lead,
  actions,
  children,
  source,
  href,
  hrefLabel = 'See indicator',
  sample = false,
}: {
  id: string
  n: string
  kicker: string
  title: ReactNode
  lead?: ReactNode
  actions?: ReactNode
  children: ReactNode
  source: ReactNode
  href: string
  hrefLabel?: string
  /** the whole section still shows bundled sample values */
  sample?: boolean
}) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className="scroll-mt-24">
      <Reveal>
        <div className="surface overflow-hidden shadow-[var(--shadow-sm)]">
          <header className="flex flex-wrap items-start justify-between gap-x-6 gap-y-4 px-5 pt-5 sm:px-7 sm:pt-6">
            <div className="max-w-[660px] min-w-0">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
                <Eyebrow n={n}>{kicker}</Eyebrow>
                {sample && <SampleMark />}
              </div>
              <h2 id={`${id}-title`} className="t-h3 mt-3">
                {title}
              </h2>
              {lead && <p className="mt-1.5 text-[14px] leading-relaxed text-ink2">{lead}</p>}
            </div>
            {actions}
          </header>
          <div className="px-5 pt-6 pb-6 sm:px-7 sm:pb-7">{children}</div>
          <footer className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-t border-line bg-bg3/45 px-5 py-3 sm:px-7">
            <p className="t-label leading-relaxed">{source}</p>
            <Link to={href} className="group inline-flex items-center gap-1 text-[13px] font-medium text-brand-ink">
              <span className="link-draw">{hrefLabel}</span>
              <ArrowRight className="size-3.5 transition-transform duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] group-hover:translate-x-0.5" />
            </Link>
          </footer>
        </div>
      </Reveal>
    </section>
  )
}

export function ChartTitle({ children, sub, className, sample = false }: { children: ReactNode; sub?: ReactNode; className?: string; sample?: boolean }) {
  return (
    <div className={cn('mb-3', className)}>
      <h3 className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] font-medium tracking-[-0.005em] text-ink">
        {children}
        {sample && <SampleMark />}
      </h3>
      {sub && <p className="mt-0.5 text-[12px] text-ink3">{sub}</p>}
    </div>
  )
}

/** Small key figure used beside charts. */
export function Fact({ label, value, note }: { label: string; value: ReactNode; note?: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="t-label">{label}</dt>
      <dd className="t-num mt-1 text-[19px] leading-none font-semibold tracking-[-0.02em] text-ink">{value}</dd>
      {note && <dd className="t-num mt-1 text-[12px] text-ink2">{note}</dd>}
    </div>
  )
}

/** The numbers behind a chart. First column left-aligned, the rest right-aligned tabular. */
export function MiniTable({ head, rows, caption }: { head: string[]; rows: (string | number)[][]; caption: string }) {
  return (
    <div className="scroll-thin max-h-[420px] overflow-auto rounded-lg border border-line">
      <table className="w-full border-collapse text-[13px]">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr>
            {head.map((h, i) => (
              <th key={h} scope="col" className={cn('sticky top-0 z-10 border-b border-line2 bg-bg3 px-3 py-2 text-[11.5px] font-medium whitespace-nowrap text-ink2', i === 0 ? 'text-left' : 'text-right')}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, ri) => (
            <tr key={ri} className="border-b border-line last:border-0">
              {r.map((c, i) => (
                <td key={i} className={cn('px-3 py-[7px] whitespace-nowrap', i === 0 ? 'text-left text-ink' : 't-num text-right text-ink2')}>
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
