import { useQuery } from '@tanstack/react-query'
import { ArrowUpRight, Check, ChevronRight, CircleHelp, Copy, Download, Minus, Plus, TriangleAlert } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link, useParams } from 'react-router'
import { datasetQuery, themesQuery } from '@/api/queries'
import { isSample, type DatasetDetail } from '@/api/types'
import { FreshnessDot, LicenceLight, SampleMark } from '@/components/common/bits'
import { CodeBlock } from '@/components/common/CodeBlock'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { fmtInt } from '@/lib/format'
import { useCopy } from '@/lib/hooks'
import { API, DOWNLOADS } from '@/lib/site'
import { cn } from '@/lib/utils'

function Block({ id, title, kicker, children, className }: { id: string; title: string; kicker?: string; children: ReactNode; className?: string }) {
  return (
    <section id={id} className={cn('scroll-mt-28 border-t border-line pt-8', className)}>
      <div className="flex items-baseline justify-between gap-4">
        <h2 className="t-h3">{title}</h2>
        {kicker && <span className="t-label">{kicker}</span>}
      </div>
      <div className="mt-5">{children}</div>
    </section>
  )
}

function snippets(d: DatasetDetail) {
  return {
    api: `$ curl "${API}/datasets/${d.id}"\n$ curl "${API}/query" \\\n    -d '{"dataset":"${d.id}","filter":{"level":2},"limit":25}'`,
    mcp: `{\n  "method": "tools/call",\n  "params": {\n    "name": "describe_dataset",\n    "arguments": { "dataset_id": "${d.id}" }\n  }\n}`,
    cli: `$ lanka pull core\n$ lanka export ${d.id} --format parquet --where "level = 2"\n$ lanka diff ${(d.versions[1] ?? d.versions[0])?.dsv ?? ''} ${d.versions[0]?.dsv ?? ''} --dataset ${d.id}`,
    sql: `select *\nfrom '${DOWNLOADS}/${d.id}.parquet'\nlimit 10;`,
  }
}

function Cite({ text }: { text: string }) {
  const [copied, copy] = useCopy()
  return (
    <div className="surface p-4">
      <div className="flex items-center justify-between">
        <span className="t-label">Cite as</span>
        <button type="button" onClick={() => copy(text)} className="pressable inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[12px] text-ink2 hover:bg-bg3 hover:text-ink">
          {copied ? <Check className="size-3.5 text-ok" /> : <Copy className="size-3.5" />}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <p className="mt-2 text-[13px] leading-relaxed text-ink2">{text}</p>
    </div>
  )
}

export default function DatasetPage() {
  const { id = '' } = useParams()
  const { data: d, isPending } = useQuery(datasetQuery(id))
  const { data: themes = [] } = useQuery(themesQuery())

  if (isPending || !d)
    return (
      <div className="mx-auto max-w-[1240px] px-5 pt-32 pb-24 sm:px-8">
        <div className="skeleton h-4 w-40" />
        <div className="skeleton mt-6 h-11 w-[min(560px,90%)]" />
        <div className="skeleton mt-4 h-5 w-[min(680px,95%)]" />
        <div className="skeleton mt-10 h-24 w-full" />
        <div className="skeleton mt-8 h-80 w-full" />
      </div>
    )

  const s = snippets(d)
  const stats: [string, ReactNode][] = [
    ['Rows', fmtInt(d.rows)],
    ['Indicators', d.indicators ? fmtInt(d.indicators) : 'None'],
    ['Vintage', d.vintage],
    ['Refresh', d.cadence],
    ['Updated', d.updated],
    ['Bundle size', <span key="size" className="inline-flex flex-wrap items-center gap-x-2 gap-y-1">{`${d.sizeMb.toFixed(1)} MB`}{isSample(d.sample, 'sizeMb') && <SampleMark />}</span>],
  ]

  return (
    <article className="mx-auto max-w-[1240px] px-5 pt-28 pb-24 sm:px-8 sm:pt-32">
      <nav aria-label="Breadcrumb" className="flex flex-wrap items-center gap-1.5 font-mono text-[12px] text-ink3">
        <Link to="/data" className="hover:text-ink">
          Data
        </Link>
        <ChevronRight className="size-3" />
        <Link to={`/data?theme=${d.theme}`} className="hover:text-ink">
          {themes.find((t) => t.id === d.theme)?.label ?? d.theme}
        </Link>
        <ChevronRight className="size-3" />
        <span className="text-ink2">{d.id}</span>
      </nav>

      <header className="mt-6 grid grid-cols-1 gap-8 lg:grid-cols-[1fr_auto] lg:items-end">
        <div>
          <h1 className="t-h1 max-w-[20ch]">{d.title}</h1>
          <p className="t-lead mt-5 max-w-[62ch]">{d.summary}</p>
          <div className="mt-5 flex flex-wrap items-center gap-x-3 gap-y-2 text-[13px] text-ink2">
            <LicenceLight status={d.licence.status} label={d.licence.label} />
            <FreshnessDot state={d.freshness} />
            <span className="text-line2">·</span>
            <span>{d.publisher}</span>
            {isSample(d.sample, '*') && <SampleMark title="Every value on this page is illustrative: the live API is not reachable or does not hold this dataset" />}
          </div>
        </div>
        <div className="flex flex-wrap gap-2.5">
          <Button size="lg" asChild>
            <a href="#downloads">
              <Download /> Download
            </a>
          </Button>
          <Button size="lg" variant="outline" asChild>
            <Link to="/explore">
              View on the map <ArrowUpRight />
            </Link>
          </Button>
        </div>
      </header>

      <dl className="mt-10 grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-3 lg:grid-cols-6">
        {stats.map(([k, v]) => (
          <div key={k} className="bg-bg2 px-4 py-4">
            <dt className="text-[12px] text-ink3">{k}</dt>
            <dd className="t-num mt-1.5 text-[17px] leading-tight font-semibold tracking-[-0.02em] text-ink">{v}</dd>
          </div>
        ))}
      </dl>

      <div className="mt-12 grid grid-cols-1 gap-12 lg:grid-cols-[minmax(0,1fr)_340px] lg:gap-14">
        <div className="min-w-0 space-y-12">
          <section id="preview" className="scroll-mt-28">
            <Tabs defaultValue="preview">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <TabsList>
                  <TabsTrigger value="preview">Preview</TabsTrigger>
                  <TabsTrigger value="schema">Columns</TabsTrigger>
                </TabsList>
                <span className="t-label inline-flex flex-wrap items-center gap-2">
                  {d.previewNote ?? `first 10 of ${fmtInt(d.rows)} rows · Colombo district`}
                  {isSample(d.sample, 'preview') && <SampleMark />}
                </span>
              </div>
              <TabsContent value="preview">
                <div className="scroll-thin overflow-x-auto rounded-xl border border-line bg-bg2">
                  <table className="w-full min-w-[720px] border-collapse text-[12.5px]">
                    <thead>
                      <tr className="border-b border-line">
                        {(d.previewColumns ?? d.columns.map((c) => c.name)).map((name) => (
                          <th key={name} className="px-3.5 py-2.5 text-left font-mono text-[11px] font-normal whitespace-nowrap text-ink3">
                            {name}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {d.preview.map((r, i) => (
                        <tr key={i} className="border-b border-line last:border-0 hover:bg-bg3/50">
                          {r.map((cell, j) => (
                            <td key={j} className={cn('t-num px-3.5 py-2 whitespace-nowrap', typeof cell === 'number' ? 'text-right text-ink' : 'font-mono text-[12px] text-ink2')}>
                              {typeof cell === 'number' ? cell.toLocaleString('en') : cell}
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </TabsContent>
              <TabsContent value="schema">
                <div className="overflow-hidden rounded-xl border border-line bg-bg2">
                  {d.columns.map((c) => (
                    <div key={c.name} className="grid grid-cols-[150px_70px_1fr] gap-3 border-b border-line px-4 py-2.5 text-[13px] last:border-0">
                      <code className="text-[12.5px] text-ink">{c.name}</code>
                      <span className="font-mono text-[11.5px] text-ink3">{c.type}</span>
                      <span className="text-ink2">{c.note || '—'}</span>
                    </div>
                  ))}
                </div>
              </TabsContent>
            </Tabs>
          </section>

          <Block id="contains" title="What it contains">
            <p className="max-w-[68ch] text-[15px] leading-relaxed text-ink2">{d.description}</p>
            <ul className="mt-5 grid grid-cols-1 gap-x-8 gap-y-2.5 sm:grid-cols-2">
              {d.contains.map((c) => (
                <li key={c} className="grid grid-cols-[14px_1fr] gap-2.5 text-[14px] text-ink2">
                  <span className="mt-[9px] h-px w-3.5 bg-ink3" />
                  {c}
                </li>
              ))}
            </ul>
          </Block>

          <Block id="method" title="How it is made">
            <ol className="grid grid-cols-1 gap-px overflow-hidden rounded-xl border border-line bg-line sm:grid-cols-2">
              {d.method.map((m, i) => (
                <li key={m} className="grid grid-cols-[26px_1fr] gap-2 bg-bg2 p-4 text-[14px] leading-relaxed text-ink2">
                  <span className="t-num font-mono text-[11.5px] text-ink3">0{i + 1}</span>
                  {m}
                </li>
              ))}
            </ol>
          </Block>

          <Block id="limits" title="What it cannot tell you">
            <div className="rounded-xl border border-[color-mix(in_oklab,var(--warn)_35%,var(--line))] bg-warn-soft p-5">
              <div className="flex items-center gap-2 text-[13px] font-medium text-warn">
                <TriangleAlert className="size-4" /> Read before you use it
              </div>
              <ul className="mt-3.5 space-y-3">
                {d.limits.map((l) => (
                  <li key={l} className="grid grid-cols-[14px_1fr] gap-2.5 text-[14.5px] leading-relaxed text-ink">
                    <span className="mt-[10px] h-px w-3.5 bg-warn" />
                    {l}
                  </li>
                ))}
              </ul>
            </div>
          </Block>

          <Block id="sources" title="Sources" kicker={`${d.sources.length} documents`}>
            <ol className="space-y-3">
              {d.sources.map((src) => (
                <li key={src.n} className="surface grid grid-cols-[28px_1fr] gap-x-3 p-4">
                  <span className="t-num grid size-6 place-items-center rounded-md border border-line2 font-mono text-[11px] text-ink2">{src.n}</span>
                  <div className="min-w-0">
                    <a href={src.url} target="_blank" rel="noreferrer" className="inline-flex items-start gap-1 text-[14.5px] font-medium text-ink hover:text-brand-ink">
                      {src.title} <ArrowUpRight className="mt-0.5 size-3.5 shrink-0" />
                    </a>
                    <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[12.5px] text-ink2">
                      <span>{src.publisher}</span>
                      <LicenceLight status={src.status} label={src.licence} />
                      {src.retrieved && <span className="text-ink3">retrieved {src.retrieved}</span>}
                    </div>
                    {src.sha256 && <div className="mt-2 truncate font-mono text-[11px] text-ink3">sha256 {src.sha256}</div>}
                  </div>
                </li>
              ))}
            </ol>
            {d.licence.status === 'unclear' ? (
              <div className="mt-4 flex gap-2.5 rounded-xl border border-[color-mix(in_oklab,var(--warn)_35%,var(--line))] bg-warn-soft px-4 py-3 text-[13px] leading-relaxed text-ink">
                <CircleHelp className="mt-0.5 size-4 shrink-0 text-warn" />
                <p>
                  The publisher states no reuse licence. We republish this data with attribution in the public interest while we ask for written permission.{' '}
                  <Link to="/terms#takedown" className="font-medium whitespace-nowrap text-warn hover:underline">
                    Rights holder? Request removal
                  </Link>
                </p>
              </div>
            ) : (
              <p className="mt-3 text-xs text-ink3">
                Reuse under each source’s licence, with credit to the publisher.{' '}
                <Link to="/terms#licensing" className="link-draw text-ink2 hover:text-ink">
                  Licensing and removal requests
                </Link>
              </p>
            )}
          </Block>

          <Block id="versions" title="Version history" kicker="row-level diffs">
            <div className="overflow-hidden rounded-xl border border-line bg-bg2">
              {d.versions.map((v, i) => (
                <div key={v.dsv} className="grid grid-cols-1 items-center gap-x-4 gap-y-1 border-b border-line px-4 py-3 last:border-0 sm:grid-cols-[176px_92px_1fr_auto]">
                  <code className="flex items-center gap-2 text-[12.5px] text-ink">
                    {v.dsv}
                    {i === 0 && <Badge variant="brand">current</Badge>}
                  </code>
                  <span className="t-num text-[12.5px] text-ink3">{v.date}</span>
                  <span className="text-[13.5px] text-ink2">{v.note}</span>
                  <span className="t-num flex gap-3 font-mono text-[11.5px]">
                    <span className="inline-flex items-center gap-0.5 text-ok">
                      <Plus className="size-3" />
                      {fmtInt(v.added)}
                    </span>
                    <span className="text-ink2">~{fmtInt(v.changed)}</span>
                    <span className="inline-flex items-center gap-0.5 text-bad">
                      <Minus className="size-3" />
                      {fmtInt(v.removed)}
                    </span>
                  </span>
                </div>
              ))}
            </div>
            <p className="mt-2.5 text-xs text-ink3">Added, changed and removed rows against the previous version. Each diff is itself a download.</p>
          </Block>
        </div>

        <aside className="space-y-6 lg:sticky lg:top-24 lg:self-start">
          <div id="downloads" className="surface scroll-mt-28 overflow-hidden">
            <div className="flex items-center justify-between border-b border-line px-4 py-3">
              <h2 className="text-[14px] font-semibold">Downloads</h2>
              <span className="t-label inline-flex items-center gap-2">
                {isSample(d.sample, 'downloads') && <SampleMark title="File names and sizes are placeholders: downloads are not served by the live API yet" />}
                v {d.dataVersion ?? '20261118.1'}
              </span>
            </div>
            <ul>
              {d.downloads.map((f) => (
                <li key={f.format} className="border-b border-line last:border-0">
                  <a href={f.href} className="group flex items-center gap-3 px-4 py-2.5 transition-colors duration-150 hover:bg-bg3/60">
                    <span className="w-[86px] shrink-0 text-[13px] font-medium text-ink">{f.format}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-ink3">{f.label}</span>
                    <span className="t-num text-[12px] text-ink2">{f.size}</span>
                    <Download className="size-3.5 text-ink3 transition-transform duration-200 ease-[var(--ease-out)] group-hover:translate-y-0.5 group-hover:text-ink" />
                  </a>
                </li>
              ))}
            </ul>
            <p className="border-t border-line px-4 py-2.5 text-[11.5px] leading-relaxed text-ink3">Checksummed and immutable per release. {d.licence.status === 'unclear' ? 'Not part of the open bundle: the publisher states no licence.' : 'Included in the open bundle.'}</p>
          </div>

          <Tabs defaultValue="api">
            <TabsList className="w-full [&>button]:flex-1">
              <TabsTrigger value="api">API</TabsTrigger>
              <TabsTrigger value="mcp">MCP</TabsTrigger>
              <TabsTrigger value="cli">CLI</TabsTrigger>
              <TabsTrigger value="sql">DuckDB</TabsTrigger>
            </TabsList>
            <TabsContent value="api">
              <CodeBlock code={s.api} />
            </TabsContent>
            <TabsContent value="mcp">
              <CodeBlock code={s.mcp} lang="json" />
            </TabsContent>
            <TabsContent value="cli">
              <CodeBlock code={s.cli} />
            </TabsContent>
            <TabsContent value="sql">
              <CodeBlock code={s.sql} lang="sql" />
            </TabsContent>
          </Tabs>

          <Cite text={d.citation} />

          <div>
            <div className="t-label mb-2.5">Used in</div>
            <ul className="space-y-1.5">
              {d.usedIn.map((u) => (
                <li key={u.href}>
                  <Link to={u.href} className="link-draw inline-flex items-center gap-1 text-[13.5px] text-ink2 hover:text-ink">
                    {u.label}
                  </Link>
                </li>
              ))}
            </ul>
          </div>
        </aside>
      </div>
    </article>
  )
}
