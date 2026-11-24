// A chart for a statistical dataset: the right chart for the data (ranking, trend, spread, makeup), a plain
// sentence about it, the numbers as a table, and where they come from. Data comes from `useDatasetSeries`.
import { ArrowUpRight, CircleAlert, RotateCcw, X } from 'lucide-react'
import { useMemo, useState, type ReactNode } from 'react'
import { MAX_COMPARE, useDatasetSeries, type ChartView } from './useDatasetSeries'
import { humanise, valueLabel, LEVEL_PLURAL, LEVEL_SHORT, LEVEL_SINGULAR, periodDate, periodLabel, rankingReadout, trendReadout, valueFormat } from './format'
import { SampleMark } from '@/components/common/bits'
import { Distribution } from '@/components/charts/Distribution'
import { MultiLine, type LineSeries } from '@/components/charts/MultiLine'
import { RankBars } from '@/components/charts/RankBars'
import { StackBar } from '@/components/charts/StackBar'
import { Legend } from '@/components/charts/chartKit'
import { Badge } from '@/components/ui/badge'
import { Picker, Seg } from '@/features/workspace/inspector/controls'
import { MiniTable, ViewToggle, type View } from '@/features/area/SectionCard'
import { NATIONAL } from '@/api/real/series'
import { useTheme } from '@/lib/hooks'
import { categorical } from '@/lib/palettes'
import { cn } from '@/lib/utils'

const VIEW_LABEL: Record<ChartView, string> = { ranking: 'Ranking', trend: 'Trend', spread: 'Spread', makeup: 'Makeup', number: 'Figure' }
const VIEW_HINT: Record<ChartView, string> = {
  ranking: 'Every unit, highest first',
  trend: 'Change over time',
  spread: 'How values are spread across units',
  makeup: 'What the total is made of',
  number: 'The latest figure',
}

function Field({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div className={cn('min-w-0', className)}>
      <div className="t-label mb-1">{label}</div>
      {children}
    </div>
  )
}

/** Compare colours: the categorical set without its blue, which the plain bars use. */
function palette(theme: 'dark' | 'light') {
  const c = categorical(theme)
  return { base: c[3], compare: [c[0], c[1], c[2], c[4], c[5], c[6], c[7]] }
}

function Message({ icon, title, children, action }: { icon?: ReactNode; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex min-h-[200px] flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-line2 px-5 py-8 text-center">
      {icon}
      <p className="text-[14px] font-medium text-ink">{title}</p>
      {children && <p className="max-w-[52ch] text-[13px] leading-relaxed text-ink2">{children}</p>}
      {action}
    </div>
  )
}

function Skeleton() {
  return (
    <div aria-busy="true" aria-label="Loading chart" className="space-y-3">
      <div className="skeleton h-4 w-3/4" />
      {Array.from({ length: 8 }, (_, i) => (
        <div key={i} className="grid grid-cols-[110px_1fr] gap-3">
          <div className="skeleton h-4" />
          <div className="skeleton h-4" style={{ width: `${88 - i * 9}%` }} />
        </div>
      ))}
    </div>
  )
}

export function DatasetChart({
  datasetId,
  mapHref,
  sample = false,
  indicator,
  className,
}: {
  datasetId: string
  /** "Open on the map": the explore link for this dataset, left out when it has no map layer */
  mapHref?: string
  /** the values still come from bundled samples, not the engine */
  sample?: boolean
  /** start on this indicator */
  indicator?: string
  className?: string
}) {
  const s = useDatasetSeries(datasetId, { indicator })
  const [theme] = useTheme()
  const col = palette(theme)
  const [table, setTable] = useState<View>('chart')
  const [all, setAll] = useState<{ key: string; on: boolean }>({ key: '', on: false })
  const { ind } = s
  const f = useMemo(() => (ind ? valueFormat(ind) : null), [ind])

  if (s.metaError) return <Message title="No statistics on the live API for this dataset">{sample ? <SampleMark /> : 'Its tables and downloads are on this page.'}</Message>
  if (!s.loading && !ind) return <Message title="This dataset has no statistics to chart">It holds places or boundaries rather than values by area; see the map.</Message>
  if (!ind || !f || (s.loading && !s.view)) return <Skeleton />

  const view = s.view
  const key = `${ind.id}|${s.level}|${s.period}`
  const expanded = all.key === key && all.on
  const label = ind.label
  const lvlName = LEVEL_SINGULAR[s.level]
  const marks: Record<string, string> = Object.fromEntries(s.selected.map((id, i) => [id, col.compare[i % col.compare.length]]))
  const rankRows = s.sorted.map((r) => ({ id: r.pcode, label: r.name, value: r.value, href: s.level > 0 ? `/area/${r.pcode}` : undefined }))
  const reference = s.national !== null && s.level > 0 && s.values.length > 0 && s.national <= 1.6 * Math.max(...s.values) && s.national >= Math.min(...s.values) / 1.6 ? { value: s.national, label: 'Sri Lanka' } : null
  const canCompare = s.views.includes('trend') || s.views.includes('makeup')

  /* ---------------- the body, its sentence, its table, its summary ---------------- */
  let sentence = ''
  let summary = `${label}`
  let tableHead: string[] = []
  let tableRows: (string | number)[][] = []
  let body: ReactNode = null
  let empty: string | null = null

  if (view === 'ranking' || view === 'spread') {
    const have = s.values.length
    if (!s.rows.length || have === 0) empty = `No ${LEVEL_PLURAL[s.level]} have a value for ${label}${s.period ? ` in ${periodLabel(s.period)}` : ''}.`
    sentence = rankingReadout(s.rows, f, label, s.level, s.national, s.period ?? '')
    summary = `${label} by ${lvlName}, ${s.period ? periodLabel(s.period) : ''}. ${sentence}`
    tableHead = ['Rank', capitalise(lvlName), 'Code', `${label} (${f.unit})`]
    tableRows = s.sorted.slice(0, 2000).map((r, i) => [r.value === null ? '' : i + 1, r.name, r.pcode, r.value === null ? 'no data' : f.full(r.value)])
    if (!empty && view === 'ranking')
      body = (
        <RankBars
          rows={rankRows}
          short={f.short}
          color={col.base}
          marks={marks}
          reference={reference}
          onToggle={canCompare ? s.toggle : undefined}
          expanded={expanded}
          onExpand={() => setAll({ key, on: true })}
          unit={f.unit}
        />
      )
    const hint = canCompare && view === 'ranking' ? `Click a bar to compare units in the ${[s.views.includes('trend') && 'Trend', s.views.includes('makeup') && 'Makeup'].filter(Boolean).join(' and ')} view.` : ''
    if (hint && body) body = <>{body}<p className="mt-2 text-[12px] text-ink3">{hint}</p></>
    if (!empty && view === 'spread' && s.median !== null)
      body = (
        <Distribution values={s.values} median={s.median} reference={reference} fmt={f.short} color={col.base} unit={f.unit} noun={LEVEL_PLURAL[s.level]} ariaLabel={`Spread of ${label} across ${s.values.length} ${LEVEL_PLURAL[s.level]}: lowest ${f.short(Math.min(...s.values))}, median ${f.short(s.median)}, highest ${f.short(Math.max(...s.values))}`} />
      )
  } else if (view === 'number') {
    const v = s.rows[0]?.value ?? null
    if (v === null) empty = `There is no national value for ${label}${s.period ? ` in ${periodLabel(s.period)}` : ''}.`
    else {
      sentence = `${label} for Sri Lanka${s.period ? `, ${periodLabel(s.period)}` : ''}: ${f.say(v)}.`
      summary = sentence
      tableHead = ['Period', `${label} (${f.unit})`]
      tableRows = [[s.period ? periodLabel(s.period) : '', f.full(v)]]
      body = (
        <div className="py-6">
          <p className="t-num text-[clamp(2.4rem,8vw,4rem)] leading-none font-semibold tracking-[-0.04em] text-ink">{f.short(v)}</p>
          <p className="mt-2 text-[14px] text-ink2">
            {f.unit !== 'percent' && `${f.unit}, `}Sri Lanka{s.period ? `, ${periodLabel(s.period)}` : ''}
          </p>
          {s.periods.length > 1 && <p className="mt-1 text-[12.5px] text-ink3">{s.periods.length} periods are available; pick another above.</p>}
        </div>
      )
    }
  } else if (view === 'trend') {
    const { axis, series } = s.trend
    const lines: LineSeries[] = series.map((sr, i) => {
      const nat = sr.geo === NATIONAL
      const idx = s.selected.indexOf(sr.geo)
      return {
        id: sr.geo,
        label: sr.name,
        color: nat ? 'var(--ink2)' : idx >= 0 ? col.compare[idx % col.compare.length] : col.compare[i % col.compare.length],
        dashed: nat && series.length > 1,
        points: own(axis, sr.byPeriod).map((p) => ({ t: periodDate(p) ?? new Date(0), period: p, y: sr.byPeriod.get(p)?.value ?? null })),
      }
    })
    if (!lines.length || axis.length < 2 || lines.every((l) => l.points.length === 0)) empty = `There are no values over time for ${label} with this selection.`
    else {
      const first = series[0]
      sentence = [trendReadout(first.name, own(axis, first.byPeriod).map((p) => ({ period: p, value: first.byPeriod.get(p)?.value ?? null })), f, label)]
        .concat(series.slice(1, 2).map((x) => trendReadout(x.name, own(axis, x.byPeriod).map((p) => ({ period: p, value: x.byPeriod.get(p)?.value ?? null })), f, label)))
        .filter(Boolean)
        .join(' ')
      summary = `${label} over time, ${periodLabel(axis[0])} to ${periodLabel(axis[axis.length - 1])}, for ${series.map((x) => x.name).join(', ')}. ${sentence}`
      tableHead = ['Period', ...lines.map((l) => l.label)]
      const rows = [...axis].reverse().map((p) => [periodLabel(p), ...series.map((x) => { const v = x.byPeriod.get(p)?.value; return v === null || v === undefined ? 'no data' : f.full(v) })])
      tableRows = rows.slice(0, 600)
      body = (
        <>
          <MultiLine series={lines} unit={f.unit} yFmt={f.short} xLabel={periodLabel} zero={ind.measure_type === 'count' || ind.measure_type === 'sum'} ariaLabel={summary} />
          <Legend className="mt-3" items={lines.map((l) => ({ label: l.label, color: l.color, kind: l.dashed ? 'dashed' : 'line' }))} />
          {s.level > 0 && (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              {s.selected.map((id, i) => (
                <button key={id} type="button" onClick={() => s.toggle(id)} className="pressable inline-flex min-h-8 items-center gap-1.5 rounded-full border border-line2 bg-bg3 px-2.5 text-[12.5px] text-ink max-md:min-h-10" aria-label={`Remove ${s.names.get(id) ?? id} from the chart`}>
                  <span className="size-2 rounded-full" style={{ background: col.compare[i % col.compare.length] }} />
                  {s.names.get(id) ?? id}
                  <X className="size-3 text-ink3" />
                </button>
              ))}
              <span className="text-[12px] text-ink3">{s.selected.length === 0 ? `Pick up to ${MAX_COMPARE} ${LEVEL_PLURAL[s.level]} to compare from the ranking.` : s.selected.length >= MAX_COMPARE ? `Up to ${MAX_COMPARE} units at once.` : ''}</span>
            </div>
          )}
        </>
      )
    }
  } else if (view === 'makeup') {
    const sd = s.splitDim ?? ''
    const mk = s.makeup
    const sums = mk[0]?.segments.some((x) => x.value !== null)
    const additive = ind.measure_type !== 'share' || mk.every((m) => m.total > 95 && m.total < 105)
    if (!mk.length || !sums) empty = `No ${humanise(sd).toLowerCase()} breakdown is available for ${label}${s.period ? ` in ${periodLabel(s.period)}` : ''} here.`
    else if (!additive) empty = `The shares of ${label} by ${humanise(sd).toLowerCase()} do not add up to a whole, so they are not stacked. See the table.`
    const cats = mk[0]?.segments.map((x) => x.id) ?? []
    const colorOf = (id: string) => col.compare[cats.indexOf(id) % col.compare.length]
    tableHead = [humanise(sd), ...mk.map((m) => m.name)]
    tableRows = cats.map((c) => [valueLabel(c), ...mk.map((m) => { const v = m.segments.find((x) => x.id === c)?.value; return v === null || v === undefined ? 'no data' : `${f.full(v)}${m.total > 0 && v !== null ? ` (${((v / m.total) * 100).toFixed(1)}%)` : ''}` })])
    if (mk[0]) {
      const top = [...mk[0].segments].filter((x) => x.value !== null).sort((a, b) => (b.value as number) - (a.value as number))
      if (top.length >= 2 && mk[0].total > 0)
        sentence = `In ${mk[0].name}${s.period ? ` (${periodLabel(s.period)})` : ''}, ${valueLabel(top[0].label)} is the largest ${humanise(sd).toLowerCase()} group at ${(((top[0].value as number) / mk[0].total) * 100).toFixed(0)}% (${f.say(top[0].value as number)}), followed by ${valueLabel(top[1].label)} at ${(((top[1].value as number) / mk[0].total) * 100).toFixed(0)}%.`
      summary = `${label} by ${humanise(sd).toLowerCase()}. ${sentence}`
    }
    if (!empty)
      body = (
        <div className="space-y-5">
          {mk.map((m, i) => (
            <div key={m.id}>
              <div className="mb-1.5 flex flex-wrap items-baseline justify-between gap-x-3 text-[13px]">
                <span className="font-medium text-ink">{m.name}</span>
                <span className="t-num text-ink3">
                  total {f.short(m.total)}
                </span>
              </div>
              <StackBar
                segments={m.segments.filter((x) => x.value !== null && x.value > 0).map((x) => ({ id: x.id, label: valueLabel(x.label), share: x.value as number, color: colorOf(x.id) }))}
                axisLabel={null}
                showLabels={i === 0}
                labelMin={9}
                ariaLabel={`${m.name}: ${m.segments.map((x) => `${valueLabel(x.label)} ${x.value === null ? 'no data' : m.total ? ((x.value / m.total) * 100).toFixed(1) + '%' : ''}`).join(', ')}`}
              />
            </div>
          ))}
          <Legend items={cats.map((c) => ({ label: valueLabel(c), color: colorOf(c) }))} />
          {mk[0].segments.some((x) => x.value === null) && <p className="text-[12px] text-ink3">A group with no value is left out of the bar, not counted as zero.</p>}
          {s.level > 0 && <p className="text-[12px] text-ink3">{s.selected.length ? '' : 'Pick units in the ranking to compare their makeup.'}</p>}
        </div>
      )
  }

  /* ---------------- controls ---------------- */
  const shownDims = ind.dims.filter((d) => (s.dimValues[d]?.length ?? 0) > 1 && !(view === 'makeup' && d === s.splitDim))
  const periodOpts = [...s.periods].reverse().slice(0, 400).map((p) => ({ value: p, label: periodLabel(p) }))
  const showPeriod = s.periods.length > 1 && view !== 'trend'
  const caption = `${label} by ${lvlName}: the numbers behind the chart`
  const marker = s.basis && s.basis !== 'reported' ? s.basis : null
  const m = s.meta
  const publishers = [...new Set((m?.sources ?? []).map((x) => x.publisher))]

  return (
    <div className={cn('surface overflow-hidden', className)} data-dataset-chart>
      <div className="space-y-3 border-b border-line p-4 sm:p-5">
        {s.indicators.length > 1 && (
          <Field label="Indicator">
            <Picker value={ind.id} onChange={s.setIndicator} label="Indicator" options={s.indicators.map((i) => ({ value: i.id, label: i.label }))} className="w-full" />
          </Field>
        )}
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-[minmax(0,1.4fr)_repeat(2,minmax(0,1fr))]">
          {s.levels.length > 1 && (
            <Field label="Show by" className={s.levels.length > 3 ? 'sm:col-span-2 lg:col-span-1' : ''}>
              <Seg value={String(s.level)} onChange={(v) => s.setLevel(Number(v))} label="Admin level" className="max-md:h-12" options={s.levels.map((l) => ({ value: String(l), label: LEVEL_SHORT[l] }))} />
            </Field>
          )}
          {shownDims.map((d) => (
            <Field key={d} label={humanise(d)}>
              <Picker value={s.dims[d]} onChange={(v) => s.setDim(d, v)} label={humanise(d)} options={(s.dimValues[d] ?? []).map((v) => ({ value: v, label: valueLabel(v) }))} className="w-full" />
            </Field>
          ))}
          {showPeriod && (
            <Field label="Period">
              <Picker value={s.period} onChange={s.setPeriod} label="Period" options={periodOpts} className="w-full" />
            </Field>
          )}
          {view === 'makeup' && s.splitCandidates.length > 1 && (
            <Field label="Split by">
              <Picker value={s.splitDim} onChange={s.setSplit} label="Split by" options={s.splitCandidates.map((d) => ({ value: d, label: humanise(d) }))} className="w-full" />
            </Field>
          )}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3">
          {s.views.length > 1 ? (
            <Seg value={view ?? 'ranking'} onChange={(v) => s.setView(v as ChartView)} label="Kind of chart" className="w-full max-md:h-12 sm:w-auto sm:min-w-[320px]" options={s.views.map((v) => ({ value: v, label: VIEW_LABEL[v], hint: undefined }))} />
          ) : (
            <span className="t-label">{view ? VIEW_HINT[view] : ''}</span>
          )}
          <ViewToggle value={table} onChange={setTable} label={label} />
        </div>
      </div>

      <div className="p-4 sm:p-5">
        {sentence && (
          <p className="mb-4 max-w-[70ch] text-[14.5px] leading-relaxed text-ink" aria-live="polite">
            {sentence}
          </p>
        )}
        <div className={cn('transition-opacity duration-150', s.fetching && 'opacity-60')}>
          {s.error ? (
            <Message icon={<CircleAlert className="size-5 text-bad" />} title="These values could not be loaded" action={<button type="button" onClick={() => window.location.reload()} className="pressable inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-line2 px-3 text-[13px] text-ink hover:bg-bg3 max-md:min-h-10"><RotateCcw className="size-3.5" />Try again</button>}>
              {s.error instanceof Error ? s.error.message : 'The API did not answer.'}
            </Message>
          ) : s.loading && !body && !empty ? (
            <Skeleton />
          ) : empty ? (
            <Message icon={<CircleAlert className="size-5 text-ink3" />} title="Nothing to draw here">
              {empty}
            </Message>
          ) : table === 'table' ? (
            <MiniTable head={tableHead} rows={tableRows} caption={caption} />
          ) : (
            <div role="group" aria-label={summary}>
              {body}
            </div>
          )}
        </div>
        {table === 'table' && tableRows.length >= 600 && <p className="mt-2 text-[12px] text-ink3">The first {tableRows.length.toLocaleString('en')} rows. The full series is in the download.</p>}
      </div>

      <footer className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5 border-t border-line bg-bg3/45 px-4 py-3 sm:px-5">
        <div className="min-w-0 space-y-1.5">
          <p className="t-label leading-relaxed break-words">
            Source: {publishers.join('; ') || 'see below'}
            {m?.current && (
              <>
                {' · '}dataset version {m.current.dsv}
                {m.current.vintage ? ` (${m.current.vintage})` : ''}
              </>
            )}
            {s.dataVersion && <> · release {s.dataVersion}</>}
          </p>
          <div className="flex flex-wrap items-center gap-1.5">
            {sample && <SampleMark />}
            {marker && <Badge variant="warn" title="This figure was not reported by the publisher as it stands">{marker === 'estimated' ? 'estimated' : marker}</Badge>}
            {s.provisional && <Badge variant="warn">provisional</Badge>}
            {ind.class === 'derived' && <Badge variant="neutral">derived by Lanka Data Layer</Badge>}
            {ind.class === 'third_party' && <Badge variant="outline">third-party data</Badge>}
            <span className="t-label">{ind.id}</span>
          </div>
        </div>
        {mapHref && (
          <a href={mapHref} target="_blank" rel="noopener" className="group pressable inline-flex min-h-9 items-center gap-1 text-[13px] font-medium text-brand-ink max-md:min-h-10">
            <span className="link-draw">Open on the map</span>
            <ArrowUpRight className="size-3.5" />
          </a>
        )}
      </footer>
    </div>
  )
}

/** The periods from a series' first value to its last: before and after are not gaps, the series just does not cover them. */
function own(axis: string[], by: Map<string, { value: number | null }>): string[] {
  const has = axis.map((p) => by.get(p)?.value != null)
  const a = has.indexOf(true)
  const b = has.lastIndexOf(true)
  return a < 0 ? [] : axis.slice(a, b + 1)
}

const capitalise = (s: string) => s.charAt(0).toUpperCase() + s.slice(1)
