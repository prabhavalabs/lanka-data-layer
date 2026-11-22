// Data access for the map workspace: TanStack Query option factories and mutation functions.
//
// Every function here stands for one call of the real API (named in its comment). Today they
// resolve against ../mocks with a little latency; swapping the bodies for `fetch` changes nothing
// above this file.
import { keepPreviousData, queryOptions } from '@tanstack/react-query'
import type { Theme } from '@/lib/hooks'
import type { AnalysisRequest, GroupCompareRequest, MapDocument, RelateRequest, Source, TimeConfig, Variable } from '@/lib/map-spec'
import { resolvePeriod } from '@/lib/map-spec'
import type { AnalysisResult, CompareResult, RelateResult } from '@/mocks/analysis'
import * as layers from '@/mocks/layers'
import type { FeaturesPayload, FramePayload, LayerData } from '@/mocks/layers'
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
/** `GET /v2/catalog?mappable=true` */
export const catalogueQuery = (scheme: Theme) => queryOptions({ queryKey: ['map-catalogue', scheme], queryFn: () => served(160, () => layers.catalogue(scheme)), staleTime: HOUR })

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
/** `POST /v2/analyses` (or `/app/v1/ws/{ws}/analyses` when an input is workspace data) → 303 → the result. */
export function analysisQuery<T extends AnalysisRequest>(req: T | null | undefined, sources: Record<string, Source>) {
  const resolved = req ? resolveRequest(req, sources) : null
  return queryOptions({
    queryKey: ['analysis', resolved ? JSON.stringify({ ...resolved, result: undefined }) : null],
    queryFn: () => served(380, () => runAnalysis(resolved as RelateRequest | GroupCompareRequest)) as Promise<T extends RelateRequest ? RelateResult : T extends GroupCompareRequest ? CompareResult : AnalysisResult>,
    enabled: !!resolved,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  })
}

/* ---------------- layer data: frames, grids, features ---------------- */
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
 */
export function layerDataQuery(source: Source | undefined, doc: Pick<MapDocument, 'sources' | 'analyses'>, time: TimeConfig | null) {
  const key: unknown[] = ['layer-data']
  let load: () => Promise<LayerData> = () => Promise.reject(new Error('No source'))
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
    const resolved = req ? resolveRequest(req, doc.sources) : null
    key.push('analysis', resolved ? JSON.stringify({ ...resolved, result: undefined }) : null, source.output)
    load = async () => analysisOutput(await runAnalysis(resolved as RelateRequest | GroupCompareRequest), source.output)
  }
  return queryOptions({ queryKey: key, queryFn: () => served(source?.kind === 'analysis' ? 320 : 140, load), enabled: !!source, staleTime: Infinity, gcTime: 10 * 60 * 1000, placeholderData: keepPreviousData })
}
/** The terrain model's shading, shared by hillshade layers and relief-shaded rasters. */
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
export { DEFAULT_MAP, glance, isStarter } from '@/mocks/maps'
export type { MapSummary } from '@/mocks/maps'
export type { AnalysisResult, CompareResult, RelateData, RelateResult } from '@/mocks/analysis'
export type { CatalogueItem, FeaturesPayload, FieldMeta, FrameColumn, FramePayload, GridPayload, BoundariesPayload, LayerData, Preset } from '@/mocks/layers'
export { featureMeta, gridMeta, indicatorMeta, INDICATOR_META, GRID_META, LEVELS } from '@/mocks/layers'
