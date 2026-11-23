// Data access for the map workspace: TanStack Query option factories and mutation functions.
//
// Layer data comes from the engine whenever it serves the source: the document's sources are resolved
// (`POST /app/v1/maps/resolve`) and each one loads what it resolved to (./real/layers). A source the
// engine does not serve — the synthetic property sample, the drive-time surface — and every source when
// the engine cannot be reached, loads the bundled sample data instead, marked `origin: 'sample'`.
import { keepPreviousData, queryOptions } from '@tanstack/react-query'
import { ApiError, apiAvailable, getJson } from './http'
import { adminLevels, adminTiles, engineSource, loadResolved, resolvedKey, resolveKey, resolveSources, servable, type Resolved, type ResolvedSource } from './real/layers'
import { outputs as engineOutputs, planAnalysis, runAnalysisOnEngine, toEngine, type EngineRequest } from './real/analysis'
import { realCatalogue } from './real/registry'
import type { Theme } from '@/lib/hooks'
import type { AnalysisRequest, GroupCompareRequest, MapDocument, RelateRequest, Source, TimeConfig, Variable } from '@/lib/map-spec'
import { resolvePeriod } from '@/lib/map-spec'
import type { AnalysisResult, CompareResult, RelateResult } from '@/mocks/analysis'
import * as layers from '@/mocks/layers'
import type { CatalogueItem, FeaturesPayload, FramePayload, LayerData } from '@/mocks/layers'
import * as maps from '@/mocks/maps'
import { units } from '@/mocks/synth'

const HOUR = 60 * 60 * 1000
/** The analysis engine (and the statistics behind it) loads the first time a map asks for a result. */
const runAnalysis = async (req: RelateRequest | GroupCompareRequest) => (await import('@/mocks/analysis')).runAnalysis(req)
const net = (ms: number) => new Promise((r) => setTimeout(r, ms + Math.random() * ms * 0.6))
async function served<T>(ms: number, work: () => T | Promise<T>): Promise<T> {
  const [value] = await Promise.all([work(), net(ms)])
  return value
}

/* ---------------- catalogue ---------------- */
/**
 * What "Add layer" lists: the engine's mappable datasets (dates from `GET /v2/status/datasets`), then the
 * sample datasets it has no counterpart for. Without the engine, the whole sample catalogue.
 */
export const catalogueQuery = (scheme: Theme) =>
  queryOptions({
    queryKey: ['map-catalogue', scheme],
    queryFn: async (): Promise<CatalogueItem[]> => {
      const sample = layers.catalogue(scheme).map((c) => ({ ...c, sample: true }))
      if (!(await apiAvailable())) return sample
      const status = await getJson<{ dataset_id: string; state: string; published_at: string | null }[]>('/v2/status/datasets').catch(() => [])
      const published = new Map(status.filter((d) => d.state === 'published').map((d) => [d.dataset_id, d.published_at?.slice(0, 10) ?? '']))
      const real = realCatalogue(scheme)
        .filter((c) => !status.length || published.has(c.dataset ?? c.id))
        .map((c) => ({ ...c, updated: published.get(c.dataset ?? c.id) || c.updated }))
      return [...real, ...sample.filter((c) => layers.SAMPLE_ONLY.has(c.id))]
    },
    staleTime: HOUR,
  })
/** `POST /app/v1/maps/resolve` for the document's sources; null = the engine is unreachable. */
export const resolveQuery = (sources: Record<string, Source>, pin: MapDocument['pin']) =>
  queryOptions({ queryKey: ['resolve', resolveKey(sources, pin)], queryFn: () => resolveSources(sources, pin), staleTime: 5 * 60 * 1000, retry: false, placeholderData: keepPreviousData })
export type { Resolved, ResolvedSource }

/* ---------------- analyses ---------------- */
/** A request whose variables point at document sources, rewritten to name the indicators themselves. */
export function resolveRequest<T extends AnalysisRequest>(req: T, sources: Record<string, Source>): T {
  if (req.method !== 'relate') return req
  const fill = (v: Variable): Variable => {
    const s = v.source ? sources[v.source] : undefined
    return s?.kind === 'indicator' ? { ...v, indicator: s.indicator } : v
  }
  return { ...req, variables: { x: fill(req.variables.x), y: fill(req.variables.y), controls: req.variables.controls?.map(fill) } }
}
/** The request as the engine runs it, or null when it names sample data (then the mock runs it). */
export const engineRequest = (req: AnalysisRequest | null | undefined, sources: Record<string, Source>): EngineRequest | null => (req ? toEngine(req, sources) : null)
/**
 * `POST /v2/analyses` → 303 → the result, when the engine can run the request; otherwise the mock on the
 * synthetic sample. The engine's refusals (400 with JSON-pointer problems, 422, 429, 503) surface as errors.
 */
export function analysisQuery<T extends AnalysisRequest>(req: T | null | undefined, sources: Record<string, Source>) {
  const engine = engineRequest(req, sources)
  const resolved = req ? resolveRequest(req, sources) : null
  type R = T extends RelateRequest ? RelateResult : T extends GroupCompareRequest ? CompareResult : AnalysisResult
  return queryOptions({
    queryKey: ['analysis', engine ? ['engine', JSON.stringify(engine)] : resolved ? JSON.stringify({ ...resolved, result: undefined }) : null],
    queryFn: () => (engine ? (runAnalysisOnEngine(engine) as Promise<R>) : (served(380, () => runAnalysis(resolved as RelateRequest | GroupCompareRequest)) as Promise<R>)),
    enabled: !!resolved,
    staleTime: Infinity,
    retry: false,
    placeholderData: keepPreviousData,
  })
}
/** `POST /v2/analyses/plan`: support, n expected, cost and the guards known in advance; gates Run. Null = sample data (mock, no plan). */
export function planQuery(req: AnalysisRequest | null | undefined, sources: Record<string, Source>) {
  const engine = engineRequest(req, sources)
  return queryOptions({ queryKey: ['analysis', 'plan', engine ? JSON.stringify(engine) : null], queryFn: () => planAnalysis(engine!), enabled: !!engine, staleTime: 10 * 60 * 1000, retry: false })
}

/* ---------------- layer data: frames, grids, features ---------------- */
/** An engine output frame on the boundary tiles of its support (`…/analyses/{id}/outputs/{output}`). */
async function engineOutput(req: EngineRequest, output: string): Promise<LayerData> {
  const [result, tiles] = await Promise.all([runAnalysisOnEngine(req), adminTiles(req.support.level, req.support.within)])
  const out = engineOutputs.get(result.id)
  const frame = output === 'bivariate' ? out?.bivariate : output === 'residuals' ? out?.residuals : null
  const level = req.support.level
  const empty = { kind: 'features' as const, origin: 'api' as const, geometry: 'point' as const, collection: { type: 'FeatureCollection' as const, features: [] }, fields: [], class: 'estimated' as const }
  if (!frame) return empty
  const d = (result as RelateResult)['x-data']
  const num = (k: string) => (frame.columns[k] ?? []).map((v) => (typeof v === 'number' ? v : null))
  const col = (id: string, label: string, unit: string, breaks?: number[]) => ({ id, indicator: id, period: 'latest', unit, label, class: 'estimated' as const, values: num(id), ...(breaks ? { breaks } : {}) })
  const columns =
    output === 'bivariate'
      ? [col('x', d?.x.label ?? 'x', d?.x.unit ?? '', d?.breaks.x), col('y', d?.y.label ?? 'y', d?.y.unit ?? '', d?.breaks.y)]
      : [col('residual', `Residual (${d?.y.transform === 'log' ? 'log ' : ''}${d?.y.label.toLowerCase() ?? 'y'})`, ''), col('fitted', 'Fitted', ''), col('cooks', 'Cook’s distance', '')]
  return {
    kind: 'frame',
    origin: 'api',
    support: { scheme: 'admin', level, parent: req.support.within, n: frame.codes.length },
    codes: frame.codes,
    names: [],
    columns,
    geometry: { type: 'FeatureCollection', features: [] },
    centroids: [],
    fields: columns.map((c) => ({ id: c.id, label: c.label, kind: 'number' as const, unit: c.unit })),
    tiles,
  } satisfies FramePayload
}
/** An analysis output, in the delivery shape the layer type expects (`…/analyses/{id}/outputs/{output}`). */
async function analysisOutput(result: AnalysisResult, output: string): Promise<LayerData> {
  const empty = { kind: 'features' as const, geometry: 'point' as const, collection: { type: 'FeatureCollection' as const, features: [] }, fields: [], class: 'estimated' as const }
  if (result.method.id === 'group_compare') {
    const d = (result as CompareResult)['x-data']
    const groups = (result as CompareResult).estimates?.groups ?? []
    if (!d) return empty
    return {
      kind: 'features', geometry: 'point', collection: d.points, title: 'ring', class: 'synthetic',
      fields: [
        { id: 'ring', label: 'Position', kind: 'category', categories: groups.slice(0, -1).map((g) => g.id) },
        { id: 'price_per_perch', label: 'Asking price', kind: 'number', unit: 'LKR/perch', domain: [100_000, 12_000_000] },
        { id: 'depth_m', label: 'Flood depth', kind: 'number', unit: 'm', domain: [0, 4] },
      ],
    } satisfies FeaturesPayload
  }
  const d = (result as RelateResult)['x-data']
  // a blocked analysis has no outputs: the layer stays, empty, until the request is one that can run
  if (!d) return empty
  const u = await units(d.support.level)
  const at = new Map(u.codes.map((c, i) => [c, i]))
  const idx = d.table.codes.map((c) => at.get(c)!)
  if (output === 'local_lee')
    return {
      kind: 'features', geometry: 'polygon', title: 'name', class: 'estimated',
      collection: { type: 'FeatureCollection', features: idx.map((i, k) => ({ ...u.features[i], properties: { ...u.features[i].properties, L: +d.local.L[k].toFixed(3), p_fdr: +d.local.pFdr[k].toFixed(4), cluster: d.local.cluster[k] } })) },
      fields: [
        { id: 'cluster', label: 'Cluster', kind: 'category', categories: ['HH', 'HL', 'LH', 'LL'] },
        { id: 'p_fdr', label: 'Adjusted p', kind: 'number', domain: [0, 1] },
      ],
    } satisfies FeaturesPayload
  const col = (id: string, v: typeof d.x, values: number[]) => ({ id, indicator: v.indicator, period: '2026', unit: v.unit, label: v.label, class: 'estimated' as const, values })
  return {
    kind: 'frame',
    support: { scheme: 'admin', level: d.support.level, parent: d.support.within, n: idx.length },
    codes: d.table.codes,
    names: d.table.names,
    columns: [
      { ...col('x', d.x, d.table.x) },
      { ...col('y', d.y, d.table.y), n: d.table.n, basis: d.table.estimated.map((e) => (e ? 'estimated' : 'observed')) },
    ],
    geometry: { type: 'FeatureCollection', features: idx.map((i) => u.features[i]) },
    centroids: idx.map((i) => u.centroids[i]),
    fields: [
      { id: 'x', label: d.x.label, kind: 'number', unit: d.x.unit },
      { id: 'y', label: d.y.label, kind: 'number', unit: d.y.unit },
    ],
  } satisfies FramePayload
}

/**
 * What a source resolves to. The key carries only what changes the bytes: a frame holds every period,
 * so stepping time does not refetch it (10 §2.7); a grid is one period per fetch.
 *
 * `resolved` is the source's entry in the resolved document: undefined while resolution is pending (the
 * query waits, so a layer never flashes sample data before its real data), null when the engine does not
 * serve it (sample data).
 */
export function layerDataQuery(source: Source | undefined, doc: Pick<MapDocument, 'sources' | 'analyses'>, time: TimeConfig | null, resolved: ResolvedSource | null | undefined = null) {
  const key: unknown[] = ['layer-data']
  let load: () => Promise<LayerData> = () => Promise.reject(new Error('No source'))
  const waiting = resolved === undefined && source?.kind !== 'analysis'
  if (source && source.kind !== 'analysis' && servable(resolved ?? undefined)) {
    const r = resolved as ResolvedSource
    key.push('api', ...resolvedKey(r))
    // a refusal (400: a dim the indicator does not have, a value it has none for) is final; only a failure is retried
    return queryOptions({ queryKey: key, queryFn: () => loadResolved(source, r), enabled: true, staleTime: Infinity, gcTime: 10 * 60 * 1000, placeholderData: keepPreviousData, retry: (n, e) => n < 1 && !(e instanceof ApiError && e.status < 500) })
  }
  // the engine has this source but could not resolve it as written: the layer fails with the engine's reason, not with sample data
  if (source && source.kind !== 'analysis' && resolved?.problem && engineSource(source)) {
    const problem = resolved.problem
    key.push('problem', problem)
    return queryOptions({ queryKey: key, queryFn: (): Promise<LayerData> => Promise.reject(new Error(problem)), enabled: true, staleTime: Infinity, retry: false })
  }
  if (source?.kind === 'indicator') {
    const ids = source.indicators?.length ? source.indicators : [source.indicator]
    key.push('frame', ids.join('|'), source.level, source.parent ?? null)
    load = () => layers.frame(ids, source.level, source.parent)
  } else if (source?.kind === 'grid') {
    const meta = layers.gridMeta(source.grid)
    const period = resolvePeriod(source.period, time, meta?.periods ?? ['2026'])
    key.push('grid', source.grid, period)
    load = () => layers.grid(source.grid, period)
  } else if (source?.kind === 'features') {
    key.push('features', source.dataset)
    load = () => layers.features(source.dataset)
  } else if (source?.kind === 'boundaries') {
    key.push('boundaries', source.level, source.parent ?? null)
    load = () => layers.boundaries(source.level, source.parent)
  } else if (source?.kind === 'analysis') {
    const req = doc.analyses[source.ref]
    const engine = engineRequest(req, doc.sources)
    const resolved = req ? resolveRequest(req, doc.sources) : null
    if (engine) {
      key.push('analysis', 'engine', JSON.stringify(engine), source.output)
      return queryOptions({ queryKey: key, queryFn: () => engineOutput(engine, source.output), enabled: true, staleTime: Infinity, gcTime: 10 * 60 * 1000, placeholderData: keepPreviousData, retry: false })
    }
    key.push('analysis', resolved ? JSON.stringify({ ...resolved, result: undefined }) : null, source.output)
    load = async () => ({ ...(await analysisOutput(await runAnalysis(resolved as RelateRequest | GroupCompareRequest), source.output)), origin: 'sample' as const })
  }
  const sample = async () => ({ ...(await load()), origin: 'sample' as const })
  return queryOptions({ queryKey: key, queryFn: () => served(source?.kind === 'analysis' ? 320 : 140, source?.kind === 'analysis' ? load : sample), enabled: !!source && !waiting, staleTime: Infinity, gcTime: 10 * 60 * 1000, placeholderData: keepPreviousData })
}
/** The terrain model's shading, shared by hillshade layers and relief-shaded rasters. */
/** Which admin levels have boundary tiles, and from what zoom; null without the engine (the sample has all four, from z0). */
export const adminLevelsQuery = () => queryOptions({ queryKey: ['admin-levels'], queryFn: async () => ((await apiAvailable()) ? adminLevels() : null), staleTime: Infinity, retry: false })
export const reliefQuery = (exaggeration: number, enabled = true) => queryOptions({ queryKey: ['relief', exaggeration], queryFn: () => layers.relief(exaggeration), staleTime: Infinity, enabled })

/* ---------------- map documents ---------------- */
/** `GET /app/v1/workspaces/{ws}/maps` */
export const mapsQuery = () => queryOptions({ queryKey: ['maps'], queryFn: () => served(120, () => maps.listMaps()), staleTime: 0 })
/** `GET /app/v1/maps/{id}` */
export const mapQuery = (id: string | null, scheme: Theme) => queryOptions({ queryKey: ['map', id, maps.isStarter(id ?? undefined) ? scheme : null], queryFn: () => served(90, () => maps.getMap(id!, scheme)), enabled: !!id, staleTime: 0, gcTime: 0 })
/** `PUT /app/v1/maps/{id}` (appends a version), or `POST …/maps` for a new or forked one. */
export const saveMap = (doc: MapDocument) => served(260, () => maps.saveMap(doc))
/** `POST /app/v1/maps/{id}/fork` */
export const forkMap = (id: string, scheme: Theme) => served(200, () => maps.forkMap(id, scheme))
/** `DELETE /app/v1/maps/{id}` */
export const deleteMap = (id: string) => served(140, () => maps.deleteMap(id))
export { BASE_MAP, baseMap, DEFAULT_MAP, glance, isStarter } from '@/mocks/maps'
export type { MapSummary } from '@/mocks/maps'
export type { AnalysisResult, CompareResult, RelateData, RelateResult } from '@/mocks/analysis'
export type { CatalogueItem, DemPayload, FeaturesPayload, FieldMeta, FrameColumn, FramePayload, GridPayload, BoundariesPayload, LayerData, Preset, ValuesPayload } from '@/mocks/layers'
export { featureMeta, gridMeta, indicatorMeta, INDICATOR_META, GRID_META, LEVELS } from '@/mocks/layers'
