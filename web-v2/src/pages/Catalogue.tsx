import { useQuery } from '@tanstack/react-query'
import { ArrowUpRight, Search, SlidersHorizontal, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { datasetsQuery, themesQuery } from '@/api/queries'
import type { Dataset, LicenceStatus } from '@/api/types'
import { Sparkline } from '@/components/charts/Sparkline'
import { Eyebrow, FreshnessDot, LicenceLight } from '@/components/common/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Sheet, SheetContent, SheetDescription, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { fmtCompact, fmtInt } from '@/lib/format'
import { cn } from '@/lib/utils'

type FacetKey = 'theme' | 'licence' | 'level' | 'publisher' | 'format'
type Filters = Record<FacetKey, string[]>
const EMPTY: Filters = { theme: [], licence: [], level: [], publisher: [], format: [] }

const LICENCE_LABEL: Record<LicenceStatus, string> = { open: 'Open', attribution: 'Open with attribution', unclear: 'Permission unclear' }
const PUBLISHER_LABEL: Record<Dataset['publisherType'], string> = { government: 'Government', multilateral: 'Multilateral', community: 'Community', research: 'Research', derived: 'Derived by us' }
const LEVELS = ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4', 'Grid', 'Point', 'Line', 'Polygon']

const levelsOf = (d: Dataset) => LEVELS.filter((l) => d.levels.some((x) => x.startsWith(l) || (x.startsWith('ADM0–') && l.startsWith('ADM'))))
const facetValues = (d: Dataset): Filters => ({ theme: [d.theme], licence: [d.licence.status], level: levelsOf(d), publisher: [d.publisherType], format: d.formats })
const matches = (d: Dataset, f: Filters, skip?: FacetKey) => {
  const v = facetValues(d)
  return (Object.keys(f) as FacetKey[]).every((k) => k === skip || f[k].length === 0 || f[k].some((x) => v[k].includes(x)))
}

function Facets({ data, filters, toggle, themeLabel }: { data: Dataset[]; filters: Filters; toggle: (k: FacetKey, v: string) => void; themeLabel: (id: string) => string }) {
  const groups: { key: FacetKey; title: string; options: string[]; label: (v: string) => React.ReactNode }[] = [
    { key: 'theme', title: 'Theme', options: [...new Set(data.map((d) => d.theme))], label: themeLabel },
    { key: 'licence', title: 'Licence status', options: ['open', 'attribution', 'unclear'], label: (v) => <LicenceLight status={v as LicenceStatus} className="bg-transparent p-0" /> },
    { key: 'level', title: 'Geography', options: LEVELS, label: (v) => v },
    { key: 'publisher', title: 'Publisher type', options: Object.keys(PUBLISHER_LABEL), label: (v) => PUBLISHER_LABEL[v as Dataset['publisherType']] },
    { key: 'format', title: 'Format', options: [...new Set(data.flatMap((d) => d.formats))].sort(), label: (v) => <span className="font-mono text-[12.5px]">{v}</span> },
  ]
  return (
    <div className="divide-y divide-line">
      {groups.map((g) => (
        <fieldset key={g.key} className="py-5 first:pt-0">
          <legend className="t-label mb-3">{g.title}</legend>
          <ul className="space-y-0.5">
            {g.options.map((o) => {
              // counts respect every other facet, so a choice never leads to an empty list by surprise
              const count = data.filter((d) => matches(d, filters, g.key) && facetValues(d)[g.key].includes(o)).length
              const id = `f-${g.key}-${o}`
              return (
                <li key={o}>
                  <label htmlFor={id} className={cn('flex min-h-8 cursor-pointer items-center gap-2.5 rounded-md px-1.5 text-[13.5px] text-ink2 hover:bg-bg3 hover:text-ink', count === 0 && 'opacity-45')}>
                    <Checkbox id={id} checked={filters[g.key].includes(o)} onCheckedChange={() => toggle(g.key, o)} />
                    <span className="flex-1 truncate">{g.label(o)}</span>
                    <span className="t-num font-mono text-[11px] text-ink3">{count}</span>
                  </label>
                </li>
              )
            })}
          </ul>
        </fieldset>
      ))}
    </div>
  )
}

export default function Catalogue() {
  const { data = [], isPending } = useQuery(datasetsQuery())
  const { data: themes = [] } = useQuery(themesQuery())
  const [params, setParams] = useSearchParams()
  const [filters, setFilters] = useState<Filters>(() => ({ ...EMPTY, theme: params.get('theme') ? [params.get('theme')!] : [] }))
  const [q, setQ] = useState('')
  const [sort, setSort] = useState('updated')

  const themeLabel = (id: string) => themes.find((t) => t.id === id)?.label ?? id
  const toggle = (k: FacetKey, v: string) => {
    setFilters((f) => ({ ...f, [k]: f[k].includes(v) ? f[k].filter((x) => x !== v) : [...f[k], v] }))
    if (k === 'theme' && params.has('theme')) setParams({}, { replace: true })
  }
  const active = (Object.keys(filters) as FacetKey[]).flatMap((k) => filters[k].map((v) => [k, v] as const))

  const rows = useMemo(() => {
    const needle = q.trim().toLowerCase()
    const out = data.filter((d) => matches(d, filters) && (!needle || `${d.title} ${d.summary} ${d.publisher} ${d.id}`.toLowerCase().includes(needle)))
    const by: Record<string, (a: Dataset, b: Dataset) => number> = {
      updated: (a, b) => b.updated.localeCompare(a.updated),
      title: (a, b) => a.title.localeCompare(b.title),
      rows: (a, b) => b.rows - a.rows,
    }
    return out.sort(by[sort])
  }, [data, filters, q, sort])

  const facets = <Facets data={data} filters={filters} toggle={toggle} themeLabel={themeLabel} />

  return (
    <div className="mx-auto max-w-[1240px] px-5 pt-28 pb-24 sm:px-8 sm:pt-32">
      <header className="grid grid-cols-1 gap-8 border-b border-line pb-10 lg:grid-cols-[1.2fr_1fr] lg:items-end">
        <div>
          <Eyebrow>Data catalogue</Eyebrow>
          <h1 className="t-h1 mt-5">Every dataset, with its licence and its limits.</h1>
          <p className="t-lead mt-5 max-w-[52ch]">
            {data.length || 20} datasets keyed to official p-codes or the canonical grid. Each one states where it came from, what you may do with it, and what it cannot tell you.
          </p>
        </div>
        <dl className="grid grid-cols-3 gap-px overflow-hidden rounded-xl border border-line bg-line text-sm">
          {[
            ['Rows held', fmtCompact(data.reduce((s, d) => s + d.rows, 0))],
            ['Indicators', fmtInt(data.reduce((s, d) => s + d.indicators, 0))],
            ['Fresh sources', `${data.filter((d) => d.freshness === 'fresh').length} of ${data.length}`],
          ].map(([k, v]) => (
            <div key={k} className="bg-bg2 px-4 py-3.5">
              <dt className="text-[12px] text-ink3">{k}</dt>
              <dd className="t-num mt-1 text-[19px] font-semibold tracking-[-0.02em]">{isPending ? <span className="skeleton inline-block h-5 w-14" /> : v}</dd>
            </div>
          ))}
        </dl>
      </header>

      <div className="mt-8 grid grid-cols-1 gap-10 lg:grid-cols-[232px_1fr]">
        <aside className="hidden lg:block">
          <div className="scroll-thin sticky top-24 max-h-[calc(100dvh-7.5rem)] overflow-y-auto pr-2">{facets}</div>
        </aside>

        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2.5">
            <label className="relative min-w-0 flex-1 basis-56">
              <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink3" />
              <input
                value={q}
                onChange={(e) => setQ(e.target.value)}
                type="search"
                enterKeyHint="search"
                placeholder="Filter by name, publisher or id"
                aria-label="Filter datasets"
                className="h-10 w-full rounded-lg border border-line2 bg-bg2 pr-3 pl-9 text-[14px] text-ink outline-none placeholder:text-ink3 focus-visible:border-ink3"
              />
            </label>
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="outline" className="h-10 lg:hidden">
                  <SlidersHorizontal /> Filters{active.length ? ` · ${active.length}` : ''}
                </Button>
              </SheetTrigger>
              <SheetContent side="bottom" aria-describedby={undefined}>
                <SheetTitle className="px-5 pt-5 text-[17px] font-semibold">Filters</SheetTitle>
                <SheetDescription className="sr-only">Narrow the dataset list</SheetDescription>
                <div className="scroll-thin overflow-y-auto px-5 pt-4 pb-6">{facets}</div>
              </SheetContent>
            </Sheet>
            <div className="w-[168px]">
              <Select value={sort} onValueChange={setSort}>
                <SelectTrigger className="h-10" aria-label="Sort">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="updated">Recently updated</SelectItem>
                  <SelectItem value="title">Name</SelectItem>
                  <SelectItem value="rows">Most rows</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="mt-4 flex min-h-7 flex-wrap items-center gap-1.5">
            <span className="t-num mr-1 text-[13px] text-ink2">
              {isPending ? 'Loading…' : `${rows.length} of ${data.length} datasets`}
            </span>
            {active.map(([k, v]) => (
              <button key={k + v} type="button" onClick={() => toggle(k, v)} className="pressable inline-flex h-6 items-center gap-1 rounded-full border border-line2 bg-bg3 pr-1.5 pl-2.5 text-[12px] text-ink hover:border-ink3" aria-label={`Remove filter ${v}`}>
                {k === 'theme' ? themeLabel(v) : k === 'licence' ? LICENCE_LABEL[v as LicenceStatus] : k === 'publisher' ? PUBLISHER_LABEL[v as Dataset['publisherType']] : v}
                <X className="size-3 text-ink3" />
              </button>
            ))}
            {active.length > 0 && (
              <button type="button" onClick={() => setFilters(EMPTY)} className="ml-1 text-[12.5px] text-brand-ink hover:underline">
                Clear all
              </button>
            )}
          </div>

          <ul className="mt-4 overflow-hidden rounded-2xl border border-line bg-bg2">
            {isPending &&
              Array.from({ length: 6 }, (_, i) => (
                <li key={i} className="border-b border-line p-5 last:border-0">
                  <div className="skeleton h-5 w-64" />
                  <div className="skeleton mt-3 h-4 w-[70%]" />
                </li>
              ))}
            {rows.map((d) => (
              <li key={d.id} className="border-b border-line last:border-0">
                <Link to={`/data/${d.id}`} className="group grid grid-cols-1 gap-x-6 gap-y-3 p-5 transition-colors duration-150 hover:bg-bg3/50 md:grid-cols-[1fr_210px]">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="text-[16px] font-semibold tracking-[-0.015em] text-ink">{d.title}</h2>
                      <ArrowUpRight className="size-4 text-ink3 transition-transform duration-200 ease-[var(--ease-out)] group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-ink" />
                    </div>
                    <p className="mt-1.5 max-w-[68ch] text-[14px] leading-relaxed text-ink2">{d.summary}</p>
                    <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 text-[12.5px] text-ink3">
                      <LicenceLight status={d.licence.status} label={d.licence.label} />
                      <span>{d.publisher}</span>
                      <span className="text-line2">·</span>
                      <span>{d.vintage}</span>
                    </div>
                    <div className="mt-3 flex flex-wrap gap-1.5">
                      <Badge variant="outline">{themeLabel(d.theme)}</Badge>
                      {d.levels.map((l) => (
                        <Badge key={l} variant="mono">
                          {l}
                        </Badge>
                      ))}
                    </div>
                  </div>
                  <dl className="grid grid-cols-2 content-start gap-x-4 gap-y-2.5 border-t border-line pt-3 text-[12.5px] md:border-t-0 md:border-l md:pt-0 md:pl-6">
                    <div>
                      <dt className="text-ink3">Rows</dt>
                      <dd className="t-num mt-0.5 text-[14px] font-medium text-ink">{fmtCompact(d.rows)}</dd>
                    </div>
                    <div>
                      <dt className="text-ink3">Updated</dt>
                      <dd className="t-num mt-0.5 text-[13px] text-ink">{d.updated}</dd>
                    </div>
                    <div className="col-span-2 flex items-center justify-between">
                      <FreshnessDot state={d.freshness} />
                      <Sparkline data={d.spark} width={76} height={20} dot={false} color="var(--ink3)" />
                    </div>
                    <div className="col-span-2 truncate font-mono text-[11px] text-ink3">{d.formats.join(' · ')}</div>
                  </dl>
                </Link>
              </li>
            ))}
            {!isPending && rows.length === 0 && (
              <li className="px-5 py-16 text-center">
                <p className="text-[15px] font-medium">No dataset matches those filters.</p>
                <p className="mt-1 text-sm text-ink2">We may simply not hold it yet. Clear a filter, or check the roadmap.</p>
                <Button variant="outline" className="mt-5" onClick={() => (setFilters(EMPTY), setQ(''))}>
                  Clear filters
                </Button>
              </li>
            )}
          </ul>
        </div>
      </div>
    </div>
  )
}
