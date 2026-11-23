import { useQuery } from '@tanstack/react-query'
import { ArrowUpRight, ChevronLeft, ChevronRight, LayoutGrid, List, Search, SlidersHorizontal, X } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { datasetsQuery, themesQuery } from '@/api/queries'
import { isSample, type Dataset, type LicenceStatus } from '@/api/types'
import { Sparkline } from '@/components/charts/Sparkline'
import { Eyebrow, FreshnessDot, LicenceLight, SampleMark } from '@/components/common/bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Sheet, SheetContent, SheetDescription, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { fmtCompact, fmtInt } from '@/lib/format'
import { cn } from '@/lib/utils'

type FacetKey = 'theme' | 'licence' | 'level' | 'publisher' | 'format'
type Filters = Record<FacetKey, string[]>
const EMPTY: Filters = { theme: [], licence: [], level: [], publisher: [], format: [] }

const LICENCE_LABEL: Record<LicenceStatus, string> = { open: 'Open', attribution: 'Open with attribution', unclear: 'Permission unclear' }
const PUBLISHER_LABEL: Record<Dataset['publisherType'], string> = { government: 'Government', multilateral: 'Multilateral', community: 'Community', research: 'Research', derived: 'Derived by us' }
const LEVELS = ['ADM0', 'ADM1', 'ADM2', 'ADM3', 'ADM4', 'Grid', 'Point', 'Line', 'Polygon']

/** 'ADM1–ADM3' covers ADM1, ADM2 and ADM3; 'Grid 100 m' covers Grid. */
const covers = (x: string, l: string) => {
  const r = /^ADM(\d)–ADM(\d)$/.exec(x)
  const n = /^ADM(\d)$/.exec(l)
  return r && n ? Number(n[1]) >= Number(r[1]) && Number(n[1]) <= Number(r[2]) : x.startsWith(l)
}
const levelsOf = (d: Dataset) => LEVELS.filter((l) => d.levels.some((x) => covers(x, l)))
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
        <div key={g.key} role="group" aria-labelledby={`fg-${g.key}`} className="pt-5 pb-4 first:pt-0">
          <h3 id={`fg-${g.key}`} className="t-label mb-1.5 px-1.5">
            {g.title}
          </h3>
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
        </div>
      ))}
    </div>
  )
}

type View = 'list' | 'grid'

function ListRow({ d, themeLabel }: { d: Dataset; themeLabel: (id: string) => string }) {
  return (
    <li className="border-b border-line last:border-0">
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
  )
}

/** A card: the same facts as a row, with the tags up front, for scanning many at once. */
function GridCard({ d, themeLabel }: { d: Dataset; themeLabel: (id: string) => string }) {
  const levels = d.levels.slice(0, 3)
  return (
    <li className="min-w-0">
      <Link to={`/data/${d.id}`} className="group flex h-full flex-col rounded-2xl border border-line bg-bg2 p-4 transition-[border-color,background-color] duration-150 hover:border-line2 hover:bg-bg3/40">
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge variant="outline">{themeLabel(d.theme)}</Badge>
          {levels.map((l) => (
            <Badge key={l} variant="mono">
              {l}
            </Badge>
          ))}
          {d.levels.length > levels.length && <Badge variant="mono">+{d.levels.length - levels.length}</Badge>}
        </div>
        <h2 className="mt-3 flex items-start gap-1.5 text-[15px] leading-snug font-semibold tracking-[-0.015em] text-ink">
          <span className="line-clamp-2 min-w-0 flex-1">{d.title}</span>
          <ArrowUpRight className="mt-0.5 size-4 shrink-0 text-ink3 transition-transform duration-200 ease-[var(--ease-out)] group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-ink" />
        </h2>
        <p className="mt-1.5 line-clamp-3 text-[13px] leading-relaxed text-ink2">{d.summary}</p>
        <div className="mt-auto pt-4">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1.5 text-[12px] text-ink3">
            <LicenceLight status={d.licence.status} label={d.licence.label} />
            <FreshnessDot state={d.freshness} />
          </div>
          <div className="mt-3 flex items-center justify-between gap-3 border-t border-line pt-3 text-[12px]">
            <span className="min-w-0 truncate text-ink3">{d.publisher}</span>
            <span className="t-num shrink-0 font-medium text-ink">{fmtCompact(d.rows)} rows</span>
          </div>
        </div>
      </Link>
    </li>
  )
}

/** Pages of the list: first, last, the current one and its neighbours, with gaps between. */
function Pager({ page, pages, total, size, onPage }: { page: number; pages: number; total: number; size: number; onPage: (n: number) => void }) {
  const shown = [...new Set([1, page - 1, page, page + 1, pages])].filter((n) => n >= 1 && n <= pages).sort((a, b) => a - b)
  return (
    <nav aria-label="Pages" className="mt-6 flex flex-wrap items-center justify-between gap-3">
      <span className="t-num text-[12.5px] text-ink3">
        {(page - 1) * size + 1}–{Math.min(page * size, total)} of {total}
      </span>
      <div className="flex items-center gap-1">
        <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => onPage(page - 1)} aria-label="Previous page" className="h-8 w-8 p-0 max-md:h-10 max-md:w-10">
          <ChevronLeft />
        </Button>
        {shown.map((n, i) => (
          <span key={n} className="flex items-center gap-1">
            {i > 0 && n - shown[i - 1] > 1 && <span className="px-1 text-[12.5px] text-ink3">…</span>}
            <Button variant={n === page ? 'primary' : 'outline'} size="sm" onClick={() => onPage(n)} aria-current={n === page ? 'page' : undefined} className="t-num h-8 min-w-8 px-2 text-[12.5px] max-md:h-10 max-md:min-w-10">
              {n}
            </Button>
          </span>
        ))}
        <Button variant="outline" size="sm" disabled={page >= pages} onClick={() => onPage(page + 1)} aria-label="Next page" className="h-8 w-8 p-0 max-md:h-10 max-md:w-10">
          <ChevronRight />
        </Button>
      </div>
    </nav>
  )
}

export default function Catalogue() {
  const { data = [], isPending } = useQuery(datasetsQuery())
  const { data: themes = [] } = useQuery(themesQuery())
  const [params, setParams] = useSearchParams()
  const [filters, setFilters] = useState<Filters>(() => ({ ...EMPTY, theme: params.get('theme') ? [params.get('theme')!] : [] }))
  const [q, setQ] = useState('')
  const [sort, setSort] = useState('updated')
  // the view and the page ride in the address, so a link (and the back button) lands where it was
  const view: View = params.get('view') === 'grid' ? 'grid' : 'list'
  const size = view === 'grid' ? 24 : 20
  const top = useRef<HTMLDivElement>(null)
  const setParam = (k: string, v: string | null) =>
    setParams(
      (p) => {
        const n = new URLSearchParams(p)
        if (v === null) n.delete(k)
        else n.set(k, v)
        return n
      },
      { replace: true },
    )

  const themeLabel = (id: string) => themes.find((t) => t.id === id)?.label ?? id
  const toggle = (k: FacetKey, v: string) => {
    setFilters((f) => ({ ...f, [k]: f[k].includes(v) ? f[k].filter((x) => x !== v) : [...f[k], v] }))
    if (k === 'theme' && params.has('theme')) setParam('theme', null)
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
  const pages = Math.max(1, Math.ceil(rows.length / size))
  const page = Math.min(pages, Math.max(1, Number(params.get('page')) || 1))
  const pageRows = rows.slice((page - 1) * size, page * size)
  const goTo = (n: number) => {
    setParam('page', n > 1 ? String(n) : null)
    top.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }
  // a narrower list starts again from its first page
  const narrowed = `${JSON.stringify(filters)}|${q}|${sort}|${view}`
  const last = useRef(narrowed)
  useEffect(() => {
    if (last.current !== narrowed && params.has('page')) setParam('page', null)
    last.current = narrowed
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [narrowed])

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
          <div className="scroll-thin sticky top-24 max-h-[calc(100dvh-7.5rem)] overflow-x-hidden overflow-y-auto pr-2">{facets}</div>
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
                <div className="scroll-thin overflow-x-hidden overflow-y-auto px-5 pt-4 pb-6">{facets}</div>
              </SheetContent>
            </Sheet>
            <ToggleGroup type="single" value={view} onValueChange={(v) => v && setParam('view', v === 'grid' ? 'grid' : null)} aria-label="Layout" className="h-10 w-[84px] shrink-0">
              <ToggleGroupItem value="list" aria-label="List" title="List">
                <List />
              </ToggleGroupItem>
              <ToggleGroupItem value="grid" aria-label="Grid" title="Grid">
                <LayoutGrid />
              </ToggleGroupItem>
            </ToggleGroup>
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
              {isPending ? 'Loading…' : rows.length > size ? `${(page - 1) * size + 1}–${Math.min(page * size, rows.length)} of ${rows.length} datasets${rows.length < data.length ? ` (${data.length} in all)` : ''}` : `${rows.length} of ${data.length} datasets`}
            </span>
            {data.some((d) => isSample(d.sample, 'spark')) && (
              <span className="order-last ml-auto inline-flex items-center gap-1.5 text-[12px] text-ink3">
                Sparklines <SampleMark title="Trend sparklines are illustrative: per-dataset API read counts are not served yet" />
              </span>
            )}
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

          <div ref={top} className="scroll-mt-28" />
          {!isPending && rows.length === 0 ? (
            <div className="mt-4 rounded-2xl border border-line bg-bg2 px-5 py-16 text-center">
              <p className="text-[15px] font-medium">No dataset matches those filters.</p>
              <p className="mt-1 text-sm text-ink2">We may simply not hold it yet. Clear a filter, or check the roadmap.</p>
              <Button variant="outline" className="mt-5" onClick={() => (setFilters(EMPTY), setQ(''))}>
                Clear filters
              </Button>
            </div>
          ) : view === 'list' ? (
            <ul className="mt-4 overflow-hidden rounded-2xl border border-line bg-bg2">
              {isPending &&
                Array.from({ length: 6 }, (_, i) => (
                  <li key={i} className="border-b border-line p-5 last:border-0">
                    <div className="skeleton h-5 w-64" />
                    <div className="skeleton mt-3 h-4 w-[70%]" />
                  </li>
                ))}
              {pageRows.map((d) => (
                <ListRow key={d.id} d={d} themeLabel={themeLabel} />
              ))}
            </ul>
          ) : (
            <ul className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {isPending && Array.from({ length: 6 }, (_, i) => <li key={i} className="skeleton h-[228px] rounded-2xl" />)}
              {pageRows.map((d) => (
                <GridCard key={d.id} d={d} themeLabel={themeLabel} />
              ))}
            </ul>
          )}
          {pages > 1 && <Pager page={page} pages={pages} total={rows.length} size={size} onPage={goTo} />}
        </div>
      </div>
    </div>
  )
}
