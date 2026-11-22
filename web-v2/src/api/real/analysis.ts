// Analyses on the engine (WP-29, ui-contract §6): plan, run, read outputs.
//
// A request in the document names its variables by document source; the engine wants indicator ids.
// `toEngine` rewrites one into the other, and returns null when a variable is not something the engine
// serves (the synthetic property sample, workspace data): that analysis then runs on the bundled mock.
// The result is adapted into the panel's existing shape (`x-data`), built only from what the engine
// returned — estimates and sentences verbatim, the table from the `bivariate`/`scatter` outputs.
import { ApiError, apiUrl, getJson, postJson } from '@/api/http'
import { indicatorMeta, LEVELS } from '@/mocks/layers'
import type { AnalysisRequest, AnalysisResult as Result, Guard, GroupCompareEstimates, MapDocument, RelateEstimates, Source, Variable } from '@/lib/map-spec'
import type { CompareResult, RelateData, RelateResult, VarInfo } from '@/mocks/analysis'
import { areaNames } from './names'

/* ---------------- request ---------------- */
type EngineVariable = { indicator: string; dims?: Record<string, string>; period?: string; transform?: string; label?: string }
export type EngineRequest = { spec: 'ldl-analysis/1'; method: string; pin: 'latest' | { release: string }; support: { kind: 'admin'; level: number; within?: string }; variables: { x?: EngineVariable; y: EngineVariable; group?: EngineVariable; controls?: EngineVariable[] }; params: Record<string, unknown>; override?: string[] }

function variable(v: Variable | undefined, sources: Record<string, Source>): EngineVariable | null {
  if (!v) return null
  const s = v.source ? sources[v.source] : undefined
  const indicator = v.indicator ?? (s?.kind === 'indicator' ? s.indicator : undefined)
  if (!indicator) return null
  const dims = v.dims ?? (s?.kind === 'indicator' ? s.dims : undefined)
  const meta = indicatorMeta(indicator, dims)
  if (!meta?.real) return null
  const period = v.period ?? (s?.kind === 'indicator' && typeof s.period === 'string' && s.period !== 'latest' ? s.period : undefined)
  return { indicator, ...(dims ? { dims: { ...dims } } : {}), ...(period ? { period } : {}), ...(v.transform && v.transform !== 'identity' ? { transform: v.transform } : {}), label: v.label ?? meta.label.toLowerCase() }
}
/** A zone's name in a few words: the document's own when it is short, else the variable's ("flood zone"). */
function zoneLabel(group: EngineVariable, label: string | undefined): string {
  if (label && label.length <= 24) return label
  if (group.indicator.startsWith('haz.flood')) return 'flood zone'
  const short = indicatorMeta(group.indicator, group.dims)?.short
  return `${(short ?? group.indicator.split('.').pop() ?? 'the').toLowerCase()} zone`
}
/** The request as the engine takes it, or null when it names something only the sample data has. */
export function toEngine(req: AnalysisRequest, sources: Record<string, Source>, pin: MapDocument['pin'] = 'latest'): EngineRequest | null {
  if (req.method === 'relate') {
    const x = variable(req.variables.x, sources), y = variable(req.variables.y, sources)
    const controls = (req.variables.controls ?? []).map((c) => variable(c, sources))
    if (!x || !y || controls.some((c) => !c)) return null
    const p = req.params
    // Lee's L and local statistics are WP-30: ask for none rather than be refused
    const params: Record<string, unknown> = { ...p, ...(p.spatial ? { spatial: { ...p.spatial, local: 'none' } } : {}) }
    const variables = { x, y, ...(controls.length ? { controls: controls as EngineVariable[] } : {}) }
    return { spec: 'ldl-analysis/1', method: 'relate', pin: req.pin ?? pin, support: { kind: 'admin', level: req.support.level, ...(req.support.within ? { within: req.support.within } : {}) }, variables, params, ...(req.override?.length ? { override: req.override } : {}) }
  }
  // group_compare runs on areas: a measured variable and a zone variable thresholded on the same units
  const y = variable(req.variables.y, sources)
  const zone = req.params.zone
  const group = variable((req.variables as { group?: Variable }).group ?? (zone.source ? { source: zone.source } : undefined), sources)
  const threshold = (zone as { threshold?: number }).threshold
  if (req.support.kind !== 'admin' || !y || !group || threshold === undefined) return null
  const level = req.support.level
  // the engine writes the groups as "inside {label} (≥20 %)": a short name of the zone, not the document's description of it
  const label = zoneLabel(group, (zone as { label?: string }).label)
  return {
    spec: 'ldl-analysis/1',
    method: 'group_compare',
    pin: req.pin ?? pin,
    support: { kind: 'admin', level },
    variables: { y, group },
    params: { zone: { threshold, label }, compare: 'inside_vs_outside', tests: req.params.tests ?? ['welch', 'mann_whitney'], bootstrap: req.params.bootstrap ?? 999 },
  }
}

/* ---------------- plan and run ---------------- */
export type Plan = { id: string; support: { kind: string; level: number; n_expected?: number }; cost: { mode: 'sync' | 'async'; expected_ms: number; within_quota: boolean }; guards: Guard[]; alignment: unknown[] }
/** `POST /v2/analyses/plan`: called whenever the selection changes; it gates the Run button. */
export async function planAnalysis(req: EngineRequest): Promise<Plan> {
  return (await postJson<Plan>('/v2/analyses/plan', req)).payload
}
/** `POST /v2/analyses` → 303 → the stored result (fetch follows the redirect as a GET). */
export async function runEngine(req: EngineRequest): Promise<Result> {
  return (await postJson<Result>('/v2/analyses', req)).payload
}
const output = <T>(result: Result, id: string) => {
  const o = result.outputs.find((x) => x.id === id)
  return o ? getJson<T>(apiUrl(o.href)) : Promise.resolve(null)
}

/* ---------------- adapters to the panel's shape ---------------- */
type FrameOut = { codes: string[]; columns: Record<string, (number | string | null)[]>; x_breaks?: number[]; y_breaks?: number[]; breaks_on?: string }
type ScatterOut = { codes: string[]; x: number[]; y: number[]; fit?: { intercept: number; slope: number; kind: string } }
const varInfo = (v: EngineVariable, alignment: string): VarInfo => {
  const m = indicatorMeta(v.indicator, v.dims)
  return { indicator: v.indicator, label: m?.label ?? v.label ?? v.indicator, short: m?.short ?? '', unit: m?.unit ?? '', transform: (v.transform ?? 'identity') as VarInfo['transform'], alignment }
}
const tf = (v: number, t: string) => (t === 'log' ? Math.log(v) : t === 'log1p' ? Math.log1p(v) : t === 'sqrt' ? Math.sqrt(v) : v)
const untf = (v: number, t: string) => (t === 'log' ? Math.exp(v) : t === 'log1p' ? Math.expm1(v) : t === 'sqrt' ? v * v : v)
const classIndex = (v: number, breaks: number[]) => {
  let i = 0
  while (i < breaks.length && v >= breaks[i]) i++
  return i
}
/** Outputs of the analyses, kept for the map layers bound to them (`analysis` sources). */
export const outputs = new Map<string, { bivariate: FrameOut | null; residuals: FrameOut | null }>()

async function adaptRelate(result: Result<RelateEstimates>, req: EngineRequest): Promise<RelateResult> {
  const level = req.support.level
  const support = { level, label: LEVELS[level]?.plural ?? `units`, within: req.support.within ?? 'LK' }
  const align = (role: string) => (result.alignment.find((a) => a.role === role)?.method ?? 'identity').replace(/_/g, ' ')
  const x = varInfo(req.variables.x!, align('x')), y = varInfo(req.variables.y, align('y'))
  if (result.state !== 'ready' || !result.estimates) return { ...result, 'x-data': null } as RelateResult
  const [biv, sc, res, names] = await Promise.all([output<FrameOut>(result, 'bivariate'), output<ScatterOut>(result, 'scatter'), output<FrameOut>(result, 'residuals'), areaNames(level).catch(() => new Map<string, string>())])
  outputs.set(result.id, { bivariate: biv, residuals: res })
  const codes = biv?.codes ?? sc?.codes ?? []
  const xs = (biv?.columns.x ?? []).map(Number), ys = (biv?.columns.y ?? []).map(Number)
  const basis = (biv?.columns.basis ?? []) as (string | null)[]
  // the map's thirds, in the units the scatter is drawn in
  const bx = (biv?.x_breaks ?? []).map((b) => (biv?.breaks_on === 'transformed' ? untf(b, x.transform) : b))
  const by = (biv?.y_breaks ?? []).map((b) => (biv?.breaks_on === 'transformed' ? untf(b, y.transform) : b))
  const cls = xs.map((vx, i) => classIndex(ys[i], by) * 3 + classIndex(vx, bx))
  const e = result.estimates
  const ols = e.ols
  const xc = ols?.coef.find((c) => c.term === 'x')
  const ic = ols?.coef.find((c) => c.term === '(intercept)')
  const tx = xs.map((v) => tf(v, x.transform))
  const n = tx.length
  const xMean = tx.reduce((a, b) => a + b, 0) / (n || 1)
  const sxx = tx.reduce((a, b) => a + (b - xMean) ** 2, 0)
  const sigma = (ols as { diagnostics?: { sigma?: number } } | undefined)?.diagnostics?.sigma ?? 0
  const robust = e.robust?.coef
  const fit: RelateData['fit'] = {
    slope: sc?.fit?.slope ?? xc?.b ?? 0,
    intercept: sc?.fit?.intercept ?? ic?.b ?? 0,
    r2: ols?.r2 ?? 0,
    se: xc?.se_hc3 ?? 0,
    n,
    xMean,
    sxx,
    sigma,
    slopeCi: (xc?.ci ?? [NaN, NaN]) as [number, number],
    robust: { slope: robust?.find((c) => c.term === 'x')?.b ?? xc?.b ?? 0, intercept: robust?.find((c) => c.term === '(intercept)')?.b ?? ic?.b ?? 0 },
    per10: 0,
  }
  const moran = e.residual_moran as (RelateEstimates['residual_moran'] & { permutations?: number; weights?: string }) | undefined
  const spatial = (req.params.spatial ?? {}) as { weights?: string; permutations?: number }
  const data: RelateData = {
    support,
    x,
    y,
    controls: (req.variables.controls ?? []).map((c) => varInfo(c, align('control'))),
    table: { codes, names: codes.map((c) => names.get(c) ?? c), x: xs, y: ys, n: [], estimated: basis.map((b) => b === 'estimated'), cls },
    breaks: { x: bx, y: by },
    fit,
    local: { codes: [], L: [], p: [], pFdr: [], cluster: [] },
    significant: 0,
    weights: (moran?.weights ?? spatial.weights ?? 'knn6').replace(/^knn(\d+)$/, '$1 nearest neighbours'),
    permutations: moran?.permutations ?? spatial.permutations ?? 999,
  }
  return { ...(result as RelateResult), 'x-data': data, 'x-engine': true } as RelateResult
}

async function adaptCompare(result: Result<GroupCompareEstimates>, req: EngineRequest): Promise<CompareResult> {
  const y = req.variables.y
  const meta = indicatorMeta(y.indicator, y.dims)
  const zone = (req.params.zone ?? {}) as { threshold?: number }
  const t = (result.estimates as (GroupCompareEstimates & { transform?: string }) | null)?.transform ?? y.transform ?? 'identity'
  return { ...(result as CompareResult), 'x-data': result.estimates ? { unit: meta?.unit ?? '', threshold: zone.threshold ?? 0, within: null, points: { type: 'FeatureCollection', features: [] }, transform: t, label: meta?.label ?? y.label ?? y.indicator } : null, 'x-engine': true } as CompareResult
}

const runs = new Map<string, Promise<RelateResult | CompareResult>>()
/** Runs the request on the engine and adapts the result for the panel; the panel and the map layers share one run. */
export function runAnalysisOnEngine(req: EngineRequest): Promise<RelateResult | CompareResult> {
  const key = JSON.stringify(req)
  let p = runs.get(key)
  if (!p) {
    p = runEngine(req).then((result): Promise<RelateResult | CompareResult> => (req.method === 'relate' ? adaptRelate(result as Result<RelateEstimates>, req) : adaptCompare(result as Result<GroupCompareEstimates>, req)))
    p.catch(() => runs.delete(key))
    runs.set(key, p)
  }
  return p
}
export const isRefusal = (e: unknown): e is ApiError => e instanceof ApiError && e.status >= 400 && e.status < 500
