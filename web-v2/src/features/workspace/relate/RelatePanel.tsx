// "Relate layers": put two variables on a common support, test the association, and say what it does
// and does not mean. The request lives in the document (`analyses`), so every layer drawn from the
// result follows it; this panel only edits the request and reads the result.
//
// Rendering rules are the contract's (ui-contract-explore §6.4): guards above estimates, warnings open,
// NOT_CAUSAL always visible, a blocked result shows no number, the server's sentences verbatim.
import { useQuery } from '@tanstack/react-query'
import { ArrowDownUp, Ban, Check, CircleAlert, Info, Layers2, Plus, Quote } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { analysisQuery, INDICATOR_META, indicatorMeta, LEVELS, type CompareResult, type RelateResult } from '@/api/workspace'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useCopy } from '@/lib/hooks'
import { allLayers, type AnalysisOutput, type AnalysisRequest, type Guard, type GuardCode, type MapDocument, type RelateRequest, type Source, type Variable } from '@/lib/map-spec'
import { bivariateColors } from '@/lib/palettes'
import { cn } from '@/lib/utils'
import { useWorkspace } from '../context'
import { fmt, fmtWith } from '../format'
import { Field, Picker, Section, Seg } from '../inspector/controls'
import { actions, useDoc } from '../store'
import { Scatter } from './Scatter'

/** Variables this mockup can align below district level (Western Province). */
const VARS = INDICATOR_META.filter((m) => m.levels.includes(4))
const indicatorOf = (v: Variable, sources: Record<string, Source>) => {
  const s = v.source ? sources[v.source] : undefined
  return v.indicator ?? (s?.kind === 'indicator' ? s.indicator : '')
}
const fmtP = (p: number | undefined) => (p === undefined || Number.isNaN(p) ? '—' : p < 0.001 ? '< 0.001' : p.toFixed(p < 0.01 ? 3 : 2))
/** A permutation p-value cannot go below 1 / (permutations + 1): at that floor it is a bound, not a value. */
const fmtPerm = (p: number, n: number) => (p <= 1 / (n + 1) + 1e-9 ? `≤ ${(1 / (n + 1)).toFixed(3)}` : fmtP(p))
const signed = (v: number, d = 2) => `${v < 0 ? '−' : v > 0 ? '+' : ''}${Math.abs(v).toFixed(d)}`
/** What a guard is called on its chip. The explanation is the server's `message`, verbatim. */
const GUARD: Partial<Record<GuardCode, string>> = {
  NOT_CAUSAL: 'Not a cause',
  MIN_N: 'Too few units',
  SMALL_N: 'Few units',
  MAUP: 'Depends on the units',
  MAUP_UNSTABLE: 'Unstable across units',
  ECOLOGICAL: 'Areas, not households',
  RESIDUAL_AUTOCORR: 'Spatially clustered',
  ESTIMATED_INPUT: 'Modelled inputs',
  SYNTHETIC_INPUT: 'Synthetic sample',
  NO_CONTROLS: 'Nothing held constant',
}

export const DEFAULT_RELATE: RelateRequest = {
  spec: 'ldl-analysis/1',
  method: 'relate',
  support: { kind: 'admin', level: 4, within: 'LK1' },
  variables: { x: { indicator: 'exp.flood_pop_share' }, y: { indicator: '@demo/re.asking_price_per_perch_median', transform: 'log' }, controls: [{ indicator: 'est.pop_density', transform: 'log' }, { indicator: 'acc.travel_time_city' }] },
  params: { correlation: ['pearson', 'spearman'], regression: 'ols', robust: true, spatial: { weights: 'knn6', local: 'lee', permutations: 199 }, bivariate: { classes: 3, method: 'quantile' }, scale_sensitivity: [3, 4] },
}

function Stat({ label, value, sub, hint }: { label: string; value: ReactNode; sub?: ReactNode; hint?: string }) {
  const body = (
    <div className="min-w-0 rounded-xl border border-line bg-bg2/40 px-3 py-2.5">
      <div className="t-label truncate">{label}</div>
      <div className="t-num mt-1.5 font-mono text-[19px] leading-none font-medium tracking-[-0.03em] text-ink">{value}</div>
      {sub && <div className="t-num mt-1.5 truncate font-mono text-[10.5px] text-ink3">{sub}</div>}
    </div>
  )
  return hint ? (
    <Tooltip>
      <TooltipTrigger asChild>{body}</TooltipTrigger>
      <TooltipContent className="max-w-[240px] leading-[1.45]">{hint}</TooltipContent>
    </Tooltip>
  ) : (
    body
  )
}

/** Guards come before the numbers they qualify: blocks and warnings open, NOT_CAUSAL always shown, other notes one tap away. */
function Guards({ guards, onAction }: { guards: Guard[]; onAction?: (request: Partial<AnalysisRequest>) => void }) {
  const [open, setOpen] = useState<string | null>(null)
  const loud = guards.filter((g) => g.severity !== 'note' || g.code === 'NOT_CAUSAL')
  const quiet = guards.filter((g) => g.severity === 'note' && g.code !== 'NOT_CAUSAL')
  const shown = quiet.find((g) => g.code === open)
  return (
    <div className="space-y-2">
      <ul className="space-y-1.5">
        {loud.map((g) => {
          const Icon = g.severity === 'block' ? Ban : g.severity === 'warn' ? CircleAlert : Info
          return (
            <li key={g.code} className={cn('flex gap-2.5 rounded-xl px-3 py-2.5', g.severity === 'block' ? 'bg-bad-soft' : g.severity === 'warn' ? 'bg-warn-soft' : 'bg-bg3/60')}>
              <Icon className={cn('mt-[1px] size-3.5 shrink-0', g.severity === 'block' ? 'text-bad' : g.severity === 'warn' ? 'text-warn' : 'text-ink3')} />
              <div className="min-w-0">
                <div className="text-[12.5px] leading-tight font-medium text-ink">{GUARD[g.code] ?? g.code}</div>
                <p className="mt-1 text-[12px] leading-[1.5] text-ink2">{g.message}</p>
                {g.action?.request && onAction && (
                  <Button variant="outline" size="sm" onClick={() => onAction(g.action!.request!)} className="mt-2 h-7 px-2.5 text-[12.5px] max-md:h-9">
                    {g.action.label ?? 'Fix'}
                  </Button>
                )}
              </div>
            </li>
          )
        })}
      </ul>
      {quiet.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {quiet.map((g) => (
            <button
              key={g.code}
              type="button"
              aria-expanded={g.code === open}
              onClick={() => setOpen(g.code === open ? null : g.code)}
              className={cn('pressable inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[12px] outline-none max-md:h-9', g.code === open ? 'border-ink/50 bg-ink/[0.07] text-ink' : 'border-line2 text-ink2 [@media(hover:hover)_and_(pointer:fine)]:hover:border-ink3/70 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink')}
            >
              <Info className="size-3.5 shrink-0 text-ink3" />
              {GUARD[g.code] ?? g.code}
            </button>
          ))}
        </div>
      )}
      {shown && (
        <p key={shown.code} className="ws-fade border-l-2 border-line2 pl-3 text-[12px] leading-[1.55] text-ink2">
          {shown.message}
        </p>
      )}
    </div>
  )
}

/** Inputs with their versions and licences, how each was brought onto the support, the method's sources, and how to cite. */
function Explain({ result }: { result: RelateResult | CompareResult }) {
  const [open, setOpen] = useState(false)
  const [copied, copy] = useCopy()
  return (
    <div>
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={() => setOpen((o) => !o)} aria-expanded={open} className="h-7 gap-1.5 px-2.5 text-[12.5px] max-md:h-9">
          <Info className="!size-3.5" />
          Explain
        </Button>
        <Button variant="outline" size="sm" onClick={() => copy(`${result.citation.text} ${result.citation.url}`)} className="h-7 gap-1.5 px-2.5 text-[12.5px] max-md:h-9">
          {copied ? <Check className="!size-3.5 text-ok" /> : <Quote className="!size-3.5" />}
          {copied ? 'Copied' : 'Cite'}
        </Button>
        <span className="t-label ml-auto truncate">{result.id.slice(0, 11)}…</span>
      </div>
      {open && (
        <div className="ws-fade mt-3 space-y-3 text-[12px] leading-[1.5] text-ink2">
          <div>
            <div className="t-label mb-1.5 tracking-[0.08em] uppercase">Inputs</div>
            <ul className="space-y-1.5">
              {result.inputs.map((i, k) => (
                <li key={k} className="flex gap-2">
                  <span className="w-12 shrink-0 font-mono text-[10.5px] text-ink3">{i.role}</span>
                  <span className="min-w-0">
                    <span className="block truncate text-ink">{i.indicator ?? i.field ?? i.dataset}</span>
                    <span className="block truncate font-mono text-[10.5px] text-ink3">
                      {i.dataset} · {i.dsv.slice(0, 12)} · {i.licence}
                      {i.synthetic ? ' · synthetic' : ''}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          </div>
          <div>
            <div className="t-label mb-1.5 tracking-[0.08em] uppercase">Alignment</div>
            <ul className="space-y-1.5">
              {result.alignment.map((a, k) => (
                <li key={k} className="flex gap-2">
                  <span className="w-12 shrink-0 font-mono text-[10.5px] text-ink3">{a.role}</span>
                  <span className="min-w-0">
                    {a.from} → {a.to}: {a.method}
                    <span className="block font-mono text-[10.5px] text-ink3">
                      coverage {Math.round(a.coverage * 100)}%{a.quality ? ` · quality ${a.quality}` : ''}
                      {a.dropped && Object.values(a.dropped).some(Boolean) ? ` · dropped ${Object.entries(a.dropped).map(([k2, v]) => `${v} ${k2.replace(/_/g, ' ')}`).join(', ')}` : ''}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          </div>
          <div>
            <div className="t-label mb-1.5 tracking-[0.08em] uppercase">Method</div>
            <p>
              {result.method.id} {result.method.version} · {result.provenance.engine} · seed {result.provenance.seed} · data {result.provenance.data_version}
            </p>
            {result.method.citations.map((c) => (
              <p key={c} className="mt-1 text-ink3">
                {c}
              </p>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

/** Medians with their intervals on one axis: inside the zone against the rings around it. */
function Intervals({ result }: { result: CompareResult }) {
  const unit = result['x-data']?.unit ?? ''
  const groups = (result.estimates?.groups ?? []).filter((g) => g.n > 0)
  const lo = Math.min(...groups.map((g) => g.ci[0])), hi = Math.max(...groups.map((g) => g.ci[1]))
  const pad = (hi - lo) * 0.08 || 1
  const at = (v: number) => `${((v - (lo - pad)) / (hi - lo + 2 * pad)) * 100}%`
  return (
    <div role="img" aria-label={`Median asking price by position: ${groups.map((g) => `${g.label} ${fmtWith(g.median, unit)}`).join(', ')}`}>
      {groups.map((g, i) => (
        <div key={g.id} className="flex h-9 items-center gap-3">
          <div className="w-[118px] shrink-0">
            <div className={cn('truncate text-[12.5px] leading-tight', i === 0 ? 'font-medium text-ink' : 'text-ink2')}>{g.label.replace(' outside', '')}</div>
            <div className="t-num font-mono text-[10px] text-ink3">n = {g.n.toLocaleString('en')}</div>
          </div>
          <div className="relative h-full min-w-0 flex-1">
            <span className="absolute inset-x-0 top-1/2 h-px bg-[var(--chart-grid)]" />
            <span className="absolute top-1/2 h-[2px] -translate-y-1/2 rounded-full bg-ink2" style={{ left: at(g.ci[0]), width: `calc(${at(g.ci[1])} - ${at(g.ci[0])})` }} />
            <span className={cn('absolute top-1/2 size-[9px] -translate-x-1/2 -translate-y-1/2 rounded-full ring-2 ring-bg2', i === 0 ? 'bg-ink' : 'bg-ink2')} style={{ left: at(g.median) }} />
          </div>
          <div className="t-num w-[52px] shrink-0 text-right font-mono text-[12px] text-ink">{fmt(g.median, unit)}</div>
        </div>
      ))}
      <div className="mt-1 flex justify-between pr-[64px] pl-[130px] font-mono text-[10px] text-ink3" aria-hidden>
        <span className="t-num">{fmt(lo - pad, unit)}</span>
        <span className="t-num">{fmt(hi + pad, unit)}</span>
      </div>
      <p className="t-label mt-1.5">median asking price, LKR per perch, with its 95% interval</p>
    </div>
  )
}

function Setup({ doc, req }: { doc: MapDocument; req: RelateRequest }) {
  const x = indicatorOf(req.variables.x, doc.sources), y = indicatorOf(req.variables.y, doc.sources)
  const controls = (req.variables.controls ?? []).map((c) => indicatorOf(c, doc.sources))
  const write = (next: RelateRequest) => actions.setAnalysis('rel', next)
  const setVar = (axis: 'x' | 'y', id: string) => {
    const other = axis === 'x' ? y : x
    const cur = req.variables[axis]
    // picking the other axis's variable swaps the two
    const vars = id === other ? { x: req.variables.y, y: req.variables.x } : { ...req.variables, [axis]: { indicator: id, transform: indicatorMeta(id)?.unit === 'LKR/perch' || id === 'est.pop_density' ? 'log' : (cur.transform ?? 'identity') } as Variable }
    write({ ...req, variables: { ...req.variables, ...vars, controls: (req.variables.controls ?? []).filter((c) => indicatorOf(c, doc.sources) !== id) } })
  }
  const options = VARS.map((m) => ({ value: m.id, label: m.label }))
  const Log = ({ axis }: { axis: 'x' | 'y' }) => {
    const on = req.variables[axis].transform === 'log'
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <button type="button" aria-pressed={on} aria-label={`Logarithm of ${axis}`} onClick={() => write({ ...req, variables: { ...req.variables, [axis]: { ...req.variables[axis], transform: on ? 'identity' : 'log' } } })} className={cn('pressable h-9 shrink-0 rounded-lg border px-2 font-mono text-[11px] outline-none max-md:h-10', on ? 'border-ink/60 bg-ink/10 text-ink' : 'border-line2 text-ink3 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink2')}>
            log
          </button>
        </TooltipTrigger>
        <TooltipContent>Take the logarithm: for prices and counts, where ratios matter more than differences</TooltipContent>
      </Tooltip>
    )
  }
  return (
    <Section title="Relate">
      <div className="relative space-y-2">
        {(['x', 'y'] as const).map((axis) => (
          <div key={axis} className="flex items-center gap-2">
            <span className="grid size-6 shrink-0 place-items-center rounded-md bg-bg4 font-mono text-[11px] text-ink2 uppercase">{axis}</span>
            <Picker value={axis === 'x' ? x : y} label={`${axis} variable`} className="h-9 min-w-0 flex-1 text-[13px]" options={options} onChange={(id) => setVar(axis, id)} />
            <Log axis={axis} />
          </div>
        ))}
        <Tooltip>
          <TooltipTrigger asChild>
            <button type="button" onClick={() => write({ ...req, variables: { ...req.variables, x: req.variables.y, y: req.variables.x } })} aria-label="Swap x and y" className="pressable absolute top-1/2 left-0 grid size-6 -translate-y-1/2 place-items-center rounded-full border border-line2 bg-bg2 text-ink3 outline-none [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink">
              <ArrowDownUp className="size-3" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="left">Swap</TooltipContent>
        </Tooltip>
      </div>
      <Field label="Hold constant" stack>
        <div className="flex flex-wrap gap-1.5">
          {VARS.filter((m) => m.id !== x && m.id !== y).map((m) => {
            const on = controls.includes(m.id)
            return (
              <button
                key={m.id}
                type="button"
                aria-pressed={on}
                onClick={() => write({ ...req, variables: { ...req.variables, controls: on ? (req.variables.controls ?? []).filter((c) => indicatorOf(c, doc.sources) !== m.id) : [...(req.variables.controls ?? []), { indicator: m.id, transform: m.id === 'est.pop_density' ? 'log' : 'identity' }] } })}
                className={cn('pressable inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[12px] outline-none max-md:h-9', on ? 'border-ink/60 bg-ink/10 text-ink' : 'border-line2 text-ink3 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink2')}
              >
                {on ? <Check className="size-3" /> : <Plus className="size-3" />}
                {m.short}
              </button>
            )
          })}
        </div>
      </Field>
      <Field label="Align to" stack hint="within Western Province">
        <Seg
          value={String(req.support.level)}
          label="Common support"
          onChange={(l) => write({ ...req, support: { ...req.support, level: Number(l) } })}
          options={[2, 3, 4].map((l) => ({ value: String(l), label: `${LEVELS[l].name}s`, hint: l === 2 ? 'Three units: too few to estimate anything' : undefined }))}
        />
      </Field>
    </Section>
  )
}

export function RelatePanel() {
  const doc = useDoc()!
  const { scheme, engine } = useWorkspace()
  const req = doc.analyses.rel?.method === 'relate' ? doc.analyses.rel : null
  const zoneReq = doc.analyses.zone?.method === 'group_compare' ? doc.analyses.zone : null
  const rel = useQuery(analysisQuery(req, doc.sources))
  const zone = useQuery(analysisQuery(zoneReq, doc.sources))
  const [mode, setMode] = useState<'points' | 'hexbin' | null>(null)
  const [hover, setHover] = useState<string | null>(null)

  if (!req)
    return (
      <div className="flex h-full flex-col justify-center gap-4 px-6 pb-10">
        <div className="grid size-10 place-items-center rounded-xl border border-line2 text-ink2">
          <Layers2 className="size-5" strokeWidth={1.6} />
        </div>
        <div>
          <h3 className="text-[15px] font-semibold tracking-[-0.015em] text-ink">Relate two layers</h3>
          <p className="mt-1.5 text-[13px] leading-[1.55] text-ink2">Put two variables on the same units, see how they move together, and read what that does and does not show. In this mockup the units below district level exist for Western Province.</p>
        </div>
        <Button variant="primary" onClick={() => actions.setAnalysis('rel', DEFAULT_RELATE, 'Analysis added')} className="self-start max-md:h-11">
          Start with flood exposure and asking price
        </Button>
      </div>
    )

  const r = rel.data as RelateResult | undefined
  const z = zone.data as CompareResult | undefined
  const d = r?.['x-data'] ?? null
  const e = r?.estimates ?? null
  const layers = allLayers(doc.layers)
  const has = (ref: string, output: string) =>
    layers.find((l) => {
      const s = doc.sources[l.source]
      return s?.kind === 'analysis' && s.ref === ref && s.output === output
    })
  const link = (code: string | null) => {
    setHover(code)
    engine.current?.highlight(code)
  }
  /** Insert the output's suggested layer, bound to an analysis source of this document. */
  const add = (ref: string, o: AnalysisOutput) => {
    const existing = has(ref, o.id)
    if (existing) {
      actions.setVisible(existing.id, true)
      return actions.select(existing.id)
    }
    if (o.suggested_layer) actions.add([{ ...o.suggested_layer, source: { kind: 'analysis', ref, output: o.id }, filter: o.suggested_layer.filter ?? undefined }])
  }
  const outputs = [...(r?.outputs ?? []).map((o) => ['rel', o] as const), ...(z?.outputs ?? []).map((o) => ['zone', o] as const)].filter(([, o]) => o.suggested_layer)
  const shown = mode ?? ((d?.table.codes.length ?? 0) > 400 ? 'hexbin' : 'points')
  const partial = e?.['x-partial']
  const moran = e?.['x-moran']

  return (
    <div>
      <Setup doc={doc} req={req} />
      {!r ? (
        <div className="space-y-3 border-t border-glass-line p-4">
          <div className="skeleton h-14 w-full" />
          <div className="skeleton h-5 w-10/12" />
          <div className="skeleton h-3.5 w-full" />
          <div className="skeleton mt-4 h-[248px] w-full" />
          <p className="t-label pt-1">aligning {LEVELS[req.support.level].plural} and testing…</p>
        </div>
      ) : r.state === 'blocked' || !d || !e ? (
        // a blocked result shows no number: the guard, and the way out of it
        <section className="border-t border-glass-line px-4 pt-4 pb-5">
          <div className="t-label mb-2.5 tracking-[0.08em] uppercase">Not run</div>
          <Guards guards={r.guards} onAction={(patch) => actions.setAnalysis('rel', { ...req, ...patch } as RelateRequest)} />
        </section>
      ) : (
        <div className={cn('transition-opacity duration-200', rel.isPlaceholderData && 'opacity-55')}>
          <section className="border-t border-glass-line px-4 pt-4 pb-4">
            <div className="t-label mb-2.5 tracking-[0.08em] uppercase">Read first</div>
            <Guards guards={r.guards} />
          </section>

          <section className="border-t border-glass-line px-4 pt-4 pb-4">
            <div className="t-label mb-2 flex items-center gap-2 tracking-[0.08em] uppercase">
              Finding
              <span className="tracking-normal normal-case">
                · {r.n.used.toLocaleString('en')} {d.support.label}
              </span>
            </div>
            <p className="text-[15.5px] leading-[1.35] font-semibold tracking-[-0.015em] text-ink">{r.interpretation?.summary}</p>
            {e.ols && <p className="mt-2 text-[12.5px] leading-[1.55] text-ink2">{e.ols.reading}</p>}
          </section>

          <section className="border-t border-glass-line px-4 pt-3.5 pb-4">
            <div className="mb-1 flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <div className="truncate text-[12.5px] font-medium text-ink">{d.y.label}</div>
                <div className="t-label mt-0.5">
                  {d.y.unit === 'LKR/perch' ? 'LKR per perch' : d.y.unit}
                  {d.y.transform === 'log' ? ' · log scale' : ''}
                </div>
              </div>
              <Seg
                value={shown}
                onChange={setMode}
                label="Marks"
                className="h-7 w-[128px] shrink-0 max-md:h-9"
                options={[
                  { value: 'hexbin', label: 'Density' },
                  { value: 'points', label: 'Units' },
                ]}
              />
            </div>
            <Scatter data={d} mode={shown} colors={bivariateColors('ldl.biv.blue-orange', scheme)} scheme={scheme} hoverCode={hover} onHover={link} />
            <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px] text-ink2">
              <span className="inline-flex items-center gap-1.5">
                <span className="h-[2px] w-4 rounded-full bg-ink" />
                least squares, 95% band
              </span>
              <span className="inline-flex items-center gap-1.5">
                <span className="w-4 border-t border-dashed border-ink2" />
                robust (Theil–Sen)
              </span>
              <span className="inline-flex items-center gap-1.5">
                <span className="w-4 border-t border-dotted border-ink3" />
                map thirds
              </span>
            </div>
          </section>

          <section className="border-t border-glass-line px-4 pt-3.5 pb-4">
            <h3 className="t-label mb-2.5 tracking-[0.08em] uppercase">Association</h3>
            <div className="grid grid-cols-2 gap-2">
              {e.spearman && <Stat label="Spearman ρ" value={signed(e.spearman.rho)} sub={`95% ${signed(e.spearman.ci[0])} to ${signed(e.spearman.ci[1])}`} hint="Rank correlation: does one rise as the other falls, whatever the shape. The better summary when many units are zero." />}
              {e.pearson && <Stat label="Pearson r" value={signed(e.pearson.r)} sub={`95% ${signed(e.pearson.ci[0])} to ${signed(e.pearson.ci[1])}`} hint="Linear correlation of the (transformed) values. The interval allows for neighbouring units not being independent." />}
              {e.pearson && <Stat label="p, adjusted" value={fmtP(e.pearson.p_adjusted)} sub={`n effective ${e.pearson.n_effective?.toLocaleString('en')} of ${r.n.used.toLocaleString('en')}`} hint={`Adjusted for spatial dependence. Ignoring spatial dependence, p would be ${fmtP(e.pearson.p)}.`} />}
              {partial ? <Stat label="Partial r" value={signed(partial.r)} sub={`holding ${partial.controls.length} constant · p ${fmtP(partial.p_adjusted)}`} hint={`Correlation left once ${partial.controls.join(' and ')} are held constant.`} /> : e.ols && <Stat label="R²" value={e.ols.r2.toFixed(2)} sub={e.ols.formula} />}
            </div>
            <h3 className="t-label mt-4 mb-2.5 tracking-[0.08em] uppercase">Spatial structure</h3>
            <div className="grid grid-cols-3 gap-2">
              {moran && <Stat label="Moran I · x" value={moran.x.I.toFixed(2)} sub={`p ${fmtPerm(moran.x.p_perm, d.permutations)}`} hint={`How alike neighbouring units are in ${d.x.label.toLowerCase()}. 0 is no pattern, 1 is perfectly clustered.`} />}
              {moran && <Stat label="Moran I · y" value={moran.y.I.toFixed(2)} sub={`p ${fmtPerm(moran.y.p_perm, d.permutations)}`} hint={`How alike neighbouring units are in ${d.y.label.toLowerCase()}.`} />}
              {e.lee_l && <Stat label="Lee’s L" value={signed(e.lee_l.L)} sub={`p ${fmtPerm(e.lee_l.p_perm, d.permutations)}`} hint="Bivariate spatial association: the correlation of the two variables after each is smoothed over its neighbours." />}
            </div>
            <p className="t-label mt-2 leading-[1.5]">
              {d.weights} · {d.permutations} permutations · {d.significant.toLocaleString('en')} units locally significant at FDR 5%
              {e.residual_moran ? ` · residual Moran I ${e.residual_moran.I.toFixed(2)}` : ''}
            </p>
            {(e.scale_sensitivity?.length ?? 0) > 1 && (
              <>
                <h3 className="t-label mt-4 mb-2 tracking-[0.08em] uppercase">Same question, other units</h3>
                <table className="w-full text-[12.5px]">
                  <thead className="sr-only">
                    <tr>
                      <th>Units</th>
                      <th>n</th>
                      <th>Spearman</th>
                      <th>Pearson</th>
                    </tr>
                  </thead>
                  <tbody>
                    {e.scale_sensitivity!.map((s) => (
                      <tr key={s.level} className={cn('border-t border-line first:border-t-0', s.level === d.support.level ? 'text-ink' : 'text-ink2')}>
                        <td className="py-1.5 first-letter:uppercase">{LEVELS[s.level].plural}</td>
                        <td className="t-num py-1.5 text-right font-mono text-[11.5px] text-ink3">n {s.n.toLocaleString('en')}</td>
                        <td className="t-num w-[72px] py-1.5 text-right font-mono text-[11.5px]">ρ {signed(s['x-spearman'] ?? 0)}</td>
                        <td className="t-num w-[72px] py-1.5 text-right font-mono text-[11.5px]">r {signed(s.pearson_r)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
            <div className="mt-4">
              <Explain result={r} />
            </div>
          </section>

          {zoneReq && (
            <section className="border-t border-glass-line px-4 pt-3.5 pb-4">
              <h3 className="t-label mb-2.5 tracking-[0.08em] uppercase">Inside the flood zone, or just outside it</h3>
              {!z?.estimates ? (
                <div className="skeleton h-28 w-full" />
              ) : (
                <>
                  <Guards guards={z.guards} />
                  <p className="mt-3.5 text-[14px] leading-[1.4] font-semibold tracking-[-0.01em] text-ink">{z.interpretation?.summary}</p>
                  <div className="mt-3 mb-3 flex items-baseline gap-2.5">
                    <span className="t-num font-mono text-[26px] leading-none font-medium tracking-[-0.04em] text-ink">{signed(z.estimates.contrasts[0].diff, 1)}%</span>
                    <span className="t-num font-mono text-[11.5px] text-ink2">
                      95% {signed(z.estimates.contrasts[0].ci[0], 1)}% to {signed(z.estimates.contrasts[0].ci[1], 1)}%
                    </span>
                  </div>
                  <Intervals result={z} />
                  <p className="mt-3 text-[12px] leading-[1.55] text-ink3">
                    {z.estimates.contrasts[0].reading} Zone: modelled depth of {z['x-data']?.threshold} m or more. Welch p {fmtP(z.estimates.contrasts[0].p_welch)} · Mann–Whitney p {fmtP(z.estimates.contrasts[0].p_mann_whitney)}.
                  </p>
                  <div className="mt-3">
                    <Explain result={z} />
                  </div>
                </>
              )}
            </section>
          )}

          {outputs.length > 0 && (
            <section className="border-t border-glass-line px-4 pt-3.5 pb-5">
              <h3 className="t-label mb-2.5 tracking-[0.08em] uppercase">Add to the map</h3>
              <div className="grid grid-cols-2 gap-2 max-md:grid-cols-1">
                {outputs.map(([ref, o]) => (
                  <Button key={`${ref}.${o.id}`} variant="outline" size="sm" onClick={() => add(ref, o)} className="h-8 justify-start gap-1.5 px-2.5 text-[12.5px] max-md:h-10">
                    {has(ref, o.id) ? <Check className="!size-3.5 text-ok" /> : <Plus className="!size-3.5" />}
                    <span className="truncate">{o.suggested_layer!.name}</span>
                  </Button>
                ))}
              </div>
              <p className="mt-3 text-[12px] leading-[1.55] text-ink3">A layer added here is bound to this analysis: change the variables or the units above and it redraws.</p>
            </section>
          )}
        </div>
      )}
    </div>
  )
}
