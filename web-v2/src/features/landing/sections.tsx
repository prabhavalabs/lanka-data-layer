import { useQuery } from '@tanstack/react-query'
import { ArrowRight, ArrowUpRight, Bot, Braces, CircleHelp, Download, FileSearch, Fingerprint, MapPin, Terminal } from 'lucide-react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { areaProfileQuery, datasetsQuery, provenanceQuery, statsQuery, themesQuery } from '@/api/queries'
import { isSample } from '@/api/types'
import { LocatorMap } from '@/components/charts/LocatorMap'
import { Sparkline } from '@/components/charts/Sparkline'
import { Eyebrow, FreshnessDot, LicenceLight, PcodePlate, Reveal, SampleMark } from '@/components/common/bits'
import { CodeBlock } from '@/components/common/CodeBlock'
import { PageCrop, ProvenanceDrawer } from '@/components/common/ProvenanceDrawer'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { fmtCompact, fmtInt, fmtValue } from '@/lib/format'
import { useInView, useReducedMotion, useTheme } from '@/lib/hooks'
import { ramp } from '@/lib/palettes'
import { SITE } from '@/lib/site'
import { CLI_SESSION, MCP_CALL, MCP_CONFIG, MCP_RESULT, REST_REQUEST, REST_RESPONSE } from '@/lib/snippets'
import { cn } from '@/lib/utils'

const Section = ({ children, className, id }: { children: ReactNode; className?: string; id?: string }) => (
  <section id={id} className={cn('mx-auto max-w-[1240px] px-5 py-20 sm:px-8 sm:py-28', className)}>
    {children}
  </section>
)

/* ---------------- stats ---------------- */
function CountUp({ to, format = fmtInt }: { to: number; format?: (n: number) => string }) {
  const [ref, inView] = useInView<HTMLSpanElement>()
  const reduced = useReducedMotion()
  const done = useRef(false)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    if (!inView) {
      if (!done.current) el.textContent = format(0)
      return
    }
    if (done.current) return
    done.current = true
    if (reduced) {
      el.textContent = format(to)
      return
    }
    const t0 = performance.now()
    const D = 1100
    let raf = 0
    const tick = (t: number) => {
      const k = Math.min(1, (t - t0) / D)
      // writes straight to the node: no React render per frame
      el.textContent = format(Math.round(to * (1 - Math.pow(1 - k, 4))))
      if (k < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [inView, to, format, reduced, ref])
  // no React children: the node's text is owned by the effect above
  return <span ref={ref} className="t-num" aria-label={format(to)} />
}

export function StatsStrip() {
  const { data: s } = useQuery(statsQuery())
  const items = [
    { key: 'datasets', label: 'Datasets', value: s?.datasets ?? 0, note: 'each with licence status and limits' },
    { key: 'areas', label: 'Areas with a p-code', value: s?.areas ?? 0, note: 'province to GN division' },
    { key: 'cells', label: 'Grid cells, 100 m', value: s?.cells ?? 0, note: 'one canonical grid for every raster' },
    { key: 'sources', label: 'Source documents', value: s?.sources ?? 0, note: 'stored by SHA-256, watched on a schedule' },
  ]
  return (
    <div className="border-y border-line bg-bg2/40">
      <div className="mx-auto grid max-w-[1240px] grid-cols-2 px-5 sm:px-8 lg:grid-cols-4">
        {items.map((it, i) => (
          <div key={it.label} className={cn('py-7 pr-4 lg:py-9', i % 2 === 1 && 'border-l border-line pl-5 lg:pl-8', i >= 2 && 'max-lg:border-t max-lg:border-line', i === 2 && 'lg:border-l lg:border-line lg:pl-8')}>
            <div className="text-[clamp(1.7rem,1.1rem+2.2vw,2.75rem)] leading-none font-semibold tracking-[-0.04em] text-ink">{s ? <CountUp to={it.value} /> : <span className="skeleton inline-block h-9 w-28 align-bottom" />}</div>
            <div className="mt-2.5 text-[13.5px] font-medium text-ink">{it.label}</div>
            <div className="mt-0.5 text-[12.5px] text-ink3">{it.note}</div>
            {isSample(s?.sample, it.key) && <SampleMark className="mt-2" />}
          </div>
        ))}
      </div>
    </div>
  )
}

/* ---------------- value proposition ---------------- */
const PILLARS = [
  { n: '01', Icon: MapPin, t: 'Place first', d: 'Every observation is keyed to an official p-code, a coded geography or a grid cell. Names are labels, never keys.' },
  { n: '02', Icon: FileSearch, t: 'Evidence you can open', d: 'Each number links to its document, page and cell, the value as printed, and the hash of the file we read.' },
  { n: '03', Icon: Download, t: 'Take it with you', d: 'SQLite, Parquet, GeoParquet, PMTiles, CSV. Versioned and checksummed, with a diff against the last release.' },
  { n: '04', Icon: Bot, t: 'Machines are first-class', d: 'REST with OpenAPI, an anonymous MCP server and a CLI that works offline. No account for anything read-only.' },
]

export function Pillars() {
  return (
    <Section>
      <div className="grid grid-cols-1 gap-12 lg:grid-cols-[1fr_1.25fr] lg:gap-20">
        <Reveal>
          <Eyebrow>What it is</Eyebrow>
          <h2 className="t-h1 mt-5">What is true of this place, where did the number come from, and can I have the file?</h2>
          <p className="t-lead mt-6 max-w-[46ch]">
            Lanka Data Layer is infrastructure, not a publication. It answers those three questions for any unit of the island, in three languages, for people and for programs.
          </p>
          <p className="mt-6 text-sm text-ink3">Self-hosted, open infrastructure: our own servers, Postgres and Rust, no third-party lock-in.</p>
        </Reveal>
        <ol className="grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2">
          {PILLARS.map((p, i) => (
            <Reveal as="li" key={p.n} i={i} className="bg-bg p-6 sm:p-7">
              <div className="flex items-center justify-between">
                <p.Icon className="size-5 text-ink2" strokeWidth={1.6} />
                <span className="t-num font-mono text-[11px] text-ink3">{p.n}</span>
              </div>
              <h3 className="t-h3 mt-8">{p.t}</h3>
              <p className="mt-2 text-[14.5px] leading-relaxed text-ink2">{p.d}</p>
            </Reveal>
          ))}
        </ol>
      </div>
    </Section>
  )
}

/* ---------------- provenance showcase ---------------- */
const STEPS = [
  { t: 'The value we serve', d: 'One number on an area profile, a map or an API response.' },
  { t: 'The document it came from', d: 'Publisher, title and date, fetched on a schedule and kept.' },
  { t: 'The page and the cell', d: 'A rendered crop with the printed value highlighted, kept verbatim.' },
  { t: 'The file hash', d: 'SHA-256 of the exact file we read, so anyone can check our work.' },
]

export function ProvenanceShowcase() {
  const obs = 'obs_9a41c07e'
  const { data: p } = useQuery(provenanceQuery(obs))
  const [step, setStep] = useState(0)
  const [auto, setAuto] = useState(true)
  const [open, setOpen] = useState(false)
  const [ref, inView] = useInView<HTMLDivElement>({ once: false, margin: '-20% 0px' })
  const reduced = useReducedMotion()

  useEffect(() => {
    if (!auto || !inView || reduced || open) return
    const id = window.setInterval(() => setStep((s) => (s + 1) % STEPS.length), 3200)
    return () => window.clearInterval(id)
  }, [auto, inView, reduced, open])

  const on = (i: number) => (step === i ? 'opacity-100' : 'opacity-55')
  const ring = (i: number) => (step === i ? 'border-[var(--ink3)] shadow-soft' : 'border-line')
  const pick = (i: number) => {
    setAuto(false)
    setStep(i)
  }

  return (
    <div className="border-y border-line bg-bg2/40">
      <Section>
        <div ref={ref} className="grid grid-cols-1 gap-12 lg:grid-cols-[0.9fr_1.2fr] lg:gap-16">
          <Reveal>
            <Eyebrow>Every number traceable</Eyebrow>
            <h2 className="t-h1 mt-5">Each number opens to the page it was printed on.</h2>
            <p className="t-lead mt-6 max-w-[44ch]">Not a link to a homepage. The document, the page, the cell, the value as printed and the hash of the file.</p>
            <ol className="mt-9">
              {STEPS.map((s, i) => (
                <li key={s.t}>
                  <button
                    type="button"
                    onClick={() => pick(i)}
                    onMouseEnter={() => pick(i)}
                    aria-current={step === i}
                    className="group relative grid w-full grid-cols-[28px_1fr] gap-x-3 border-t border-line py-4 text-left"
                  >
                    <span
                      aria-hidden
                      className={cn('absolute -top-px left-0 h-px origin-left bg-brand', step === i ? (auto && !reduced ? 'w-full [animation:prov-fill_3.2s_linear]' : 'w-full') : 'w-0')}
                    />
                    <span className={cn('t-num pt-0.5 font-mono text-[11.5px] transition-colors duration-200', step === i ? 'text-brand-ink' : 'text-ink3')}>0{i + 1}</span>
                    <span>
                      <span className={cn('block text-[15.5px] font-medium tracking-[-0.01em] transition-colors duration-200', step === i ? 'text-ink' : 'text-ink2')}>{s.t}</span>
                      <span className="mt-0.5 block text-[13.5px] text-ink3">{s.d}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ol>
            <Button variant="outline" className="mt-6" onClick={() => setOpen(true)}>
              Open the provenance drawer <ArrowUpRight />
            </Button>
          </Reveal>

          <Reveal i={1} className="ticks relative self-center rounded-2xl border border-line bg-bg p-4 sm:p-6">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_1.15fr] sm:gap-4">
              <div className={cn('rounded-xl border bg-bg2 p-4 transition-[opacity,border-color,box-shadow] duration-300', on(0), ring(0))}>
                <div className="flex items-center justify-between">
                  <span className="t-label">Population · 2024</span>
                  <PcodePlate code="LK11" size="sm" />
                </div>
                <div className="t-num mt-4 text-[clamp(1.8rem,1.2rem+2vw,2.6rem)] leading-none font-semibold tracking-[-0.04em]">{p?.value ?? '2,374,461'}</div>
                <div className="mt-2 flex items-center gap-2 text-[12.5px] text-ink2">
                  Colombo district
                  <span className="inline-flex items-center gap-1 text-ink3">
                    <span className="size-1.5 rounded-full border border-current" /> provisional
                  </span>
                </div>
              </div>
              <div className={cn('rounded-xl border bg-bg2 p-4 transition-[opacity,border-color,box-shadow] duration-300', on(1), ring(1))}>
                <span className="t-label">Source document</span>
                <div className="mt-3 text-[14px] leading-snug font-medium text-ink">{p?.document.title ?? 'Census of Population and Housing 2024 — Preliminary Report'}</div>
                <div className="mt-1.5 text-[12.5px] text-ink2">
                  {p?.document.publisher ?? 'Department of Census and Statistics'} · {p?.document.published ?? '2025-04-07'} · PDF, {p?.document.pages ?? 112} pages
                </div>
              </div>
            </div>
            <div className={cn('mt-3 transition-opacity duration-300 sm:mt-4', on(2))}>
              {p ? <PageCrop p={p} className={cn('transition-shadow duration-300', step === 2 && 'shadow-soft')} /> : <div className="skeleton h-44" />}
            </div>
            <div className={cn('mt-3 flex items-start gap-3 rounded-xl border bg-bg2 p-3.5 transition-[opacity,border-color,box-shadow] duration-300 sm:mt-4', on(3), ring(3))}>
              <Fingerprint className="mt-0.5 size-4 shrink-0 text-ink3" />
              <div className="min-w-0">
                <div className="t-label">sha256</div>
                <div className="mt-1 font-mono text-[11.5px] leading-relaxed break-all text-ink2">{p?.sha256 ?? '································································'}</div>
              </div>
            </div>
          </Reveal>
        </div>
      </Section>
      <ProvenanceDrawer observationId={open ? obs : null} onClose={() => setOpen(false)} title="Population, Colombo district" />
    </div>
  )
}

/* ---------------- interfaces ---------------- */
export function Interfaces() {
  return (
    <Section>
      <Reveal className="mx-auto max-w-[760px] text-center">
        <Eyebrow className="justify-center [&>span:last-child]:hidden">For programs and agents</Eyebrow>
        <h2 className="t-h1 mt-5">One dataset, three ways in.</h2>
        <p className="t-lead mx-auto mt-5 max-w-[52ch]">The same observations, the same provenance, whether you call an endpoint, ask a model, or work offline at a terminal.</p>
      </Reveal>
      <Reveal i={1} className="mt-12">
        <Tabs defaultValue="rest" className="items-center gap-6">
          <TabsList className="h-10">
            <TabsTrigger value="rest" className="px-4">
              <Braces /> REST API
            </TabsTrigger>
            <TabsTrigger value="mcp" className="px-4">
              <Bot /> MCP server
            </TabsTrigger>
            <TabsTrigger value="cli" className="px-4">
              <Terminal /> lanka CLI
            </TabsTrigger>
          </TabsList>
          <TabsContent value="rest" className="w-full">
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-[0.8fr_1.2fr]">
              <div className="flex min-w-0 flex-col gap-4">
                <CodeBlock title="request" code={REST_REQUEST} />
                <Facts items={[['No key', 'Anonymous for every read. Rate-limited per IP.'], ['One envelope', 'success, payload, meta. meta carries source, licence and completeness.'], ['OpenAPI 3.1', 'Generated from the server, with a typed client on npm.']]} />
              </div>
              <CodeBlock title="200 application/json · 14 ms" code={REST_RESPONSE} lang="json" />
            </div>
          </TabsContent>
          <TabsContent value="mcp" className="w-full">
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-[0.8fr_1.2fr]">
              <div className="flex min-w-0 flex-col gap-4">
                <CodeBlock title="connect" code={MCP_CONFIG} />
                <CodeBlock title="tools/call" code={MCP_CALL} lang="json" />
                <Facts items={[['Ten tools, fixed', 'Read-only, idempotent, with ready-to-send next calls.'], ['Provenance in every result', 'A model can cite the page, not just the site.']]} />
              </div>
              <CodeBlock title="result" code={MCP_RESULT} lang="json" />
            </div>
          </TabsContent>
          <TabsContent value="cli" className="w-full">
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1.2fr_0.8fr]">
              <CodeBlock title="zsh" code={CLI_SESSION} />
              <div className="flex min-w-0 flex-col gap-4">
                <Facts
                  items={[
                    ['geocode-csv', 'Attach official p-codes to a spreadsheet of coordinates. Over a million rows a second on a laptop.'],
                    ['Offline first', 'Every command reads local artifacts. Only pull touches the network.'],
                    ['lanka mcp', 'The same MCP tools over stdio, against your own copy of the data.'],
                  ]}
                />
              </div>
            </div>
          </TabsContent>
        </Tabs>
      </Reveal>
      <div className="mt-8 text-center">
        <Button variant="link" asChild>
          <Link to="/developers">
            Read the developer guide <ArrowRight />
          </Link>
        </Button>
      </div>
    </Section>
  )
}

function Facts({ items }: { items: [string, string][] }) {
  return (
    <dl className="surface flex-1 divide-y divide-line px-5">
      {items.map(([t, d]) => (
        <div key={t} className="py-4">
          <dt className="text-[14px] font-medium text-ink">{t}</dt>
          <dd className="mt-1 text-[13.5px] leading-relaxed text-ink2">{d}</dd>
        </div>
      ))}
    </dl>
  )
}

/* ---------------- themes ---------------- */
export function Themes() {
  const { data: themes = [] } = useQuery(themesQuery())
  const [theme] = useTheme()
  return (
    <div className="border-y border-line bg-bg2/40">
      <Section>
        <Reveal className="flex flex-wrap items-end justify-between gap-6">
          <div>
            <Eyebrow>Themes</Eyebrow>
            <h2 className="t-h2 mt-4 max-w-[20ch]">Eight themes, one set of place keys.</h2>
          </div>
          <p className="max-w-[40ch] text-[14.5px] text-ink2">Each theme has its own colour ramp, checked for colour-vision deficiency. The accent colour is never used for data.</p>
        </Reveal>
        <ul className="mt-10 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {themes.map((t, i) => (
            <Reveal as="li" key={t.id} i={i % 4}>
              <Link to={`/data?theme=${t.id}`} className="pressable group surface flex h-full flex-col p-5 hover:border-line2 hover:bg-bg2">
                <div className="flex gap-[3px]" aria-hidden>
                  {ramp(t.ramp, theme).map((c) => (
                    <span key={c} className="h-1.5 flex-1 rounded-full" style={{ background: c }} />
                  ))}
                </div>
                <h3 className="mt-5 flex items-center justify-between text-[16px] font-semibold tracking-[-0.015em]">
                  {t.label}
                  <ArrowUpRight className="size-4 text-ink3 transition-transform duration-200 ease-[var(--ease-out)] group-hover:translate-x-0.5 group-hover:-translate-y-0.5 group-hover:text-ink" />
                </h3>
                <p className="mt-1.5 flex-1 text-[13.5px] leading-relaxed text-ink2">{t.blurb}</p>
                <div className="t-num mt-5 font-mono text-[11px] text-ink3">
                  {t.datasets} datasets{t.indicators ? ` · ${t.indicators} indicators` : ''}
                </div>
              </Link>
            </Reveal>
          ))}
        </ul>
      </Section>
    </div>
  )
}

/* ---------------- area profile teaser ---------------- */
export function AreaTeaser() {
  const { data: a } = useQuery(areaProfileQuery('LK11'))
  return (
    <Section>
      <div className="grid grid-cols-1 items-center gap-12 lg:grid-cols-[0.85fr_1.15fr] lg:gap-16">
        <Reveal>
          <Eyebrow>Area profiles</Eyebrow>
          <h2 className="t-h1 mt-5">A page for every one of 14,417 places.</h2>
          <p className="t-lead mt-6 max-w-[44ch]">Province, district, DS division, GN division. Population, access, land, climate and elections, with rank among neighbours and every source listed.</p>
          <ul className="mt-7 space-y-2.5 text-[14.5px] text-ink2">
            {['Names in English, සිංහල and தமிழ்', 'The p-code as the stable address', 'CSV and boundary download for the unit'].map((x) => (
              <li key={x} className="flex items-center gap-3">
                <span className="h-px w-4 bg-ink3" />
                {x}
              </li>
            ))}
          </ul>
          <div className="mt-8 flex flex-wrap gap-2.5">
            <Button asChild>
              <Link to="/area/LK11">
                Open Colombo <ArrowRight />
              </Link>
            </Button>
            <Button variant="outline" asChild>
              <Link to="/area/LK">Start from the country</Link>
            </Button>
          </div>
        </Reveal>

        <Reveal i={1}>
          <Link to="/area/LK11" className="pressable group block overflow-hidden rounded-2xl border border-line bg-bg2 shadow-soft hover:border-line2" aria-label="Open the Colombo area profile">
            <div className="grid grid-cols-1 sm:grid-cols-[1fr_190px]">
              <div className="p-6 sm:p-7">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[11.5px] text-ink3">
                  <span>Sri Lanka</span>
                  <span>/</span>
                  <span>Western</span>
                  <span>/</span>
                  <span className="text-ink2">Colombo</span>
                </div>
                <div className="mt-4 flex flex-wrap items-center gap-3">
                  <span className="text-[34px] leading-none font-semibold tracking-[-0.04em]">Colombo</span>
                  <PcodePlate code="LK11" />
                </div>
                <div className="mt-2 flex gap-3 text-[17px] text-ink2">
                  <span lang="si">කොළඹ</span>
                  <span lang="ta">கொழும்பு</span>
                </div>
                <dl className="mt-7 grid grid-cols-2 gap-x-6 gap-y-5">
                  {(a?.headline ?? Array.from({ length: 4 }, () => null)).map((h, i) => (
                    <div key={h?.id ?? i} className="border-t border-line pt-3">
                      <dt className="text-[12px] text-ink3">{h ? `${h.label} · ${h.year}` : <span className="skeleton inline-block h-3 w-20" />}</dt>
                      <dd className="t-num mt-1 text-[21px] leading-tight font-semibold tracking-[-0.03em]">{h ? fmtValue(h.value, h.unit) : <span className="skeleton inline-block h-6 w-24" />}</dd>
                    </div>
                  ))}
                </dl>
              </div>
              <div className="hairline-grid relative border-t border-line bg-bg p-4 sm:border-t-0 sm:border-l">
                <LocatorMap pcode="LK11" width={160} height={250} className="mx-auto max-w-[170px]" />
              </div>
            </div>
            <div className="flex items-center justify-between gap-4 border-t border-line px-6 py-3.5 sm:px-7">
              <div className="flex items-center gap-3 text-[12.5px] text-ink2">
                Residents, 2012 to 2024
                {isSample(a?.sample, 'popSeries') && <SampleMark />}
                {a && <Sparkline data={a.popSeries.map((d) => d.value)} width={110} height={24} color="var(--ink2)" />}
              </div>
              <span className="inline-flex items-center gap-1 text-[13px] font-medium text-ink">
                Open profile <ArrowRight className="size-3.5 transition-transform duration-200 ease-[var(--ease-out)] group-hover:translate-x-0.5" />
              </span>
            </div>
          </Link>
        </Reveal>
      </div>
    </Section>
  )
}

/* ---------------- catalogue teaser ---------------- */
export function CatalogueTeaser() {
  const { data = [] } = useQuery(datasetsQuery())
  const FEATURED = ['dcs-census-2024-population', 'cod-ab-lka-boundaries', 'worldpop-pop-grid', 'worldcover-land-cover', 'worldclim-climate-normals', 'jrc-river-flood-hazard', 'census-2024', 'admin-boundaries', 'pop-grid-100m', 'access-health', 'rain-chirps', 'elections-results']
  const featured = data.filter((d) => FEATURED.includes(d.id)).sort((a, b) => FEATURED.indexOf(a.id) - FEATURED.indexOf(b.id))
  const rows = featured.length >= 6 ? featured.slice(0, 6) : data.slice(0, 6)
  return (
    <div className="border-y border-line bg-bg2/40">
      <Section>
        <Reveal className="flex flex-wrap items-end justify-between gap-6">
          <div>
            <Eyebrow>Data catalogue</Eyebrow>
            <h2 className="t-h2 mt-4">Download buttons on every dataset.</h2>
          </div>
          <Button variant="outline" asChild>
            <Link to="/data">
              Browse all {data.length || 20} datasets <ArrowRight />
            </Link>
          </Button>
        </Reveal>
        <Reveal i={1} className="mt-9 overflow-hidden rounded-2xl border border-line bg-bg">
          <div className="hidden grid-cols-[1.6fr_1fr_0.7fr_0.8fr_96px] gap-4 border-b border-line px-5 py-2.5 font-mono text-[11px] text-ink3 md:grid">
            <span>Dataset</span>
            <span>Licence</span>
            <span className="text-right">Rows</span>
            <span>Freshness</span>
            <span className="flex flex-col items-end gap-1 text-right">
              API reads
              {rows.some((d) => isSample(d.sample, 'spark')) && <SampleMark title="Illustrative: per-dataset API read counts are not served yet" />}
            </span>
          </div>
          <ul className="divide-y divide-line">
            {(rows.length ? rows : Array.from({ length: 6 }, () => null)).map((d, i) =>
              d ? (
                <li key={d.id}>
                  <Link to={`/data/${d.id}`} className="group grid grid-cols-1 items-center gap-x-4 gap-y-2 px-5 py-4 transition-colors duration-150 hover:bg-bg2 md:grid-cols-[1.6fr_1fr_0.7fr_0.8fr_96px]">
                    <span className="min-w-0">
                      <span className="flex items-center gap-2 text-[14.5px] font-medium text-ink">
                        <span className="truncate">{d.title}</span>
                        <ArrowUpRight className="size-3.5 shrink-0 text-ink3 opacity-0 transition-opacity duration-150 group-hover:opacity-100" />
                      </span>
                      <span className="mt-0.5 block truncate text-[12.5px] text-ink3">{d.publisher}</span>
                    </span>
                    <span>
                      <LicenceLight status={d.licence.status} label={d.licence.label} />
                    </span>
                    <span className="t-num text-[13px] text-ink2 md:text-right">{fmtCompact(d.rows)}</span>
                    <FreshnessDot state={d.freshness} />
                    <span className="hidden justify-end md:flex">
                      <Sparkline data={d.spark} width={84} height={22} dot={false} />
                    </span>
                  </Link>
                </li>
              ) : (
                <li key={i} className="px-5 py-4">
                  <div className="skeleton h-9" />
                </li>
              ),
            )}
          </ul>
        </Reveal>
      </Section>
    </div>
  )
}

/* ---------------- honesty ---------------- */
export function Honesty() {
  const { data = [] } = useQuery(datasetsQuery())
  const n = (s: string) => data.filter((d) => d.licence.status === s).length
  const total = data.length || 1
  const parts = [
    { k: 'open', label: 'Open', n: n('open'), cls: 'bg-ok' },
    { k: 'attribution', label: 'Open with attribution', n: n('attribution'), cls: 'bg-ok/55' },
    { k: 'unclear', label: 'Permission unclear', n: n('unclear'), cls: 'bg-warn' },
  ]
  return (
    <Section>
      <div className="grid grid-cols-1 gap-12 lg:grid-cols-[1fr_1fr] lg:gap-20">
        <Reveal>
          <Eyebrow>Licence honesty</Eyebrow>
          <h2 className="t-h1 mt-5">We say what we do not know.</h2>
          <p className="t-lead mt-6 max-w-[46ch]">
            Much official data in Sri Lanka is published with no stated licence. We do not relabel it as open. Each dataset shows its status, and each dataset page has a section called “what it cannot tell you”.
          </p>
          <div className="mt-9">
            <div className="flex h-2.5 gap-[2px] overflow-hidden rounded-full" role="img" aria-label={parts.map((p) => `${p.label}: ${p.n}`).join(', ')}>
              {parts.map((p) => (
                <span key={p.k} className={cn('h-full rounded-[2px] first:rounded-l-full last:rounded-r-full', p.cls)} style={{ width: `${(p.n / total) * 100}%` }} />
              ))}
            </div>
            <ul className="mt-4 flex flex-wrap gap-x-6 gap-y-2 text-[13px] text-ink2">
              {parts.map((p) => (
                <li key={p.k} className="flex items-center gap-2">
                  <span className={cn('size-2 rounded-[2px]', p.cls)} />
                  {p.label}
                  <span className="t-num font-medium text-ink">{p.n}</span>
                </li>
              ))}
            </ul>
            <p className="mt-2 text-xs text-ink3">Datasets by licence status, of {data.length}. The open bundle leaves out everything marked unclear.</p>
          </div>
        </Reveal>
        <ul className="divide-y divide-line border-y border-line">
          {[
            ['Unclear means unclear', 'Where a publisher states no licence, the light is amber and the reason is written out. You decide whether your use is acceptable; we do not decide for you.'],
            ['Not held is not zero', 'A value that was never published at a level is shown as not held, with the reason. Gaps are never filled in to make a map look complete.'],
            ['Modelled is labelled', 'Population grids and travel times are models. They carry an estimate flag everywhere they appear, including in the API.'],
            ['Changes are published', 'Every release has a row-level diff. A revised number replaces the old one in the open, with the date and the cause.'],
          ].map(([t, d], i) => (
            <Reveal as="li" key={t} i={i} className="grid grid-cols-[24px_1fr] gap-x-4 py-5">
              <CircleHelp className="mt-0.5 size-[18px] text-ink3" strokeWidth={1.6} />
              <div>
                <h3 className="text-[15.5px] font-medium tracking-[-0.01em]">{t}</h3>
                <p className="mt-1 text-[14px] leading-relaxed text-ink2">{d}</p>
              </div>
            </Reveal>
          ))}
        </ul>
      </div>
    </Section>
  )
}

/* ---------------- closing ---------------- */
export function Closing() {
  return (
    <div className="relative overflow-hidden border-t border-line">
      <div aria-hidden className="hairline-grid absolute inset-0 [mask-image:radial-gradient(60%_80%_at_50%_100%,black,transparent)]" />
      <Section className="relative text-center">
        <Reveal>
          <h2 className="t-display mx-auto max-w-[16ch]">Start with a place.</h2>
          <p className="t-lead mx-auto mt-6 max-w-[44ch]">Search any town, p-code or postal code, or pull the whole dataset to your own machine.</p>
          <div className="mx-auto mt-9 max-w-[520px] text-left">
            <CodeBlock code={`$ curl -fsSL ${SITE}/install.sh | sh\n$ lanka pull core && lanka area LK11`} />
          </div>
          <div className="mt-7 flex flex-wrap justify-center gap-2.5">
            <Button size="lg" variant="brand" asChild>
              <Link to="/explore">
                Explore the map <ArrowRight />
              </Link>
            </Button>
            <Button size="lg" variant="outline" asChild>
              <Link to="/data">Browse the data</Link>
            </Button>
          </div>
        </Reveal>
      </Section>
    </div>
  )
}
